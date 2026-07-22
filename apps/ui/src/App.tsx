import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  Bell,
  CheckCircle2,
  ChevronLeft,
  Command,
  Gauge,
  Keyboard,
  LayoutDashboard,
  Menu,
  Moon,
  Pause,
  Play,
  Plus,
  RefreshCcw,
  Search,
  ScanSearch,
  Settings,
  Sun,
  Usb,
  X,
} from 'lucide-react';
import { Button, IconButton, LoadingScreen, Modal, StatusIcon, ToastRegion } from './components/common';
import { ProfileEditor } from './components/ProfileEditor';
import { makeId, makeProfile } from './data';
import { KeyForgeCommandError, keyforgeBridge } from './lib/bridge';
import {
  ActivityPage,
  DashboardPage,
  DevicesPage,
  KeyInspectorPage,
  ProfilesPage,
  SettingsPage,
} from './pages';
import type {
  ActionResult,
  AppPreferences,
  BootstrapPayload,
  EngineSettings,
  PageId,
  Profile,
  ResultStatus,
  Settings as KeyForgeSettings,
  ToastMessage,
} from './types';
import './styles.css';
import packageManifest from '../../../package.json';

const APP_VERSION = packageManifest.version;

const navigation: Array<{ id: PageId; label: string; icon: typeof LayoutDashboard }> = [
  { id: 'dashboard', label: '대시보드', icon: LayoutDashboard },
  { id: 'profiles', label: '프로필', icon: Keyboard },
  { id: 'key-inspector', label: '키 확인', icon: ScanSearch },
  { id: 'devices', label: '장치', icon: Usb },
  { id: 'activity', label: '활동 기록', icon: Activity },
  { id: 'settings', label: '설정', icon: Settings },
];

type SavePhase = 'idle' | 'validating' | 'writing' | 'applying' | 'saved' | 'error';

function useToastQueue() {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const timers = useRef(new Map<string, number>());
  const dismiss = useCallback((id: string) => {
    setToasts((items) => items.filter((item) => item.id !== id));
    const timer = timers.current.get(id);
    if (timer) window.clearTimeout(timer);
    timers.current.delete(id);
  }, []);
  const push = useCallback((title: string, status: ResultStatus = 'success', description?: string, action?: { label: string; run: () => void }) => {
    const id = makeId();
    setToasts((items) => [...items.slice(-3), { id, title, status, description, actionLabel: action?.label, onAction: action?.run }]);
    const timer = window.setTimeout(() => dismiss(id), status === 'error' ? 9000 : 5200);
    timers.current.set(id, timer);
  }, [dismiss]);
  useEffect(() => () => timers.current.forEach((timer) => window.clearTimeout(timer)), []);
  return { toasts, dismiss, push };
}

function CommandPalette({ open, onClose, onNavigate, onNew }: { open: boolean; onClose: () => void; onNavigate: (page: PageId) => void; onNew: () => void }) {
  const [query, setQuery] = useState('');
  const commands = [
    { label: '새 전역 프로필 만들기', detail: 'Ctrl + N', icon: Plus, run: onNew },
    ...navigation.map((item) => ({ label: `${item.label} 열기`, detail: '페이지', icon: item.icon, run: () => onNavigate(item.id) })),
  ];
  const filtered = commands.filter((item) => item.label.includes(query.trim()));
  useEffect(() => { if (open) setQuery(''); }, [open]);
  return <Modal open={open} onClose={onClose} title="명령 및 페이지 검색" description="원하는 기능으로 빠르게 이동하세요." size="small">
    <div className="command-palette">
      <label className="command-search"><Search size={18} /><input autoFocus aria-label="명령 검색" placeholder="명령 검색…" value={query} onChange={(event) => setQuery(event.target.value)} /><kbd>Esc</kbd></label>
      <div className="command-results">{filtered.map((item) => { const Icon = item.icon; return <button type="button" key={item.label} onClick={() => { item.run(); onClose(); }}><span><Icon size={17} /></span><strong>{item.label}</strong><small>{item.detail}</small></button>; })}</div>
    </div>
  </Modal>;
}

function NotificationCenter({ open, activity, onClose, onViewAll }: { open: boolean; activity: ActionResult[]; onClose: () => void; onViewAll: () => void }) {
  const [filter, setFilter] = useState<'all' | ResultStatus>('all');
  if (!open) return null;
  const items = activity.filter((item) => filter === 'all' || item.status === filter);
  return <div className="notification-overlay" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
    <aside className="notification-center" role="dialog" aria-modal="false" aria-label="알림 센터">
      <header><div><span className="page-eyebrow">ACTION RESULTS</span><h2>알림 센터</h2></div><IconButton label="알림 센터 닫기" onClick={onClose}><X size={19} /></IconButton></header>
      <div className="notification-filters">{([['all', '전체'], ['success', '성공'], ['warning', '경고'], ['error', '오류']] as const).map(([id, label]) => <button type="button" key={id} className={filter === id ? 'is-active' : ''} onClick={() => setFilter(id)}>{label}</button>)}</div>
      <div className="notification-list">{items.length ? items.map((item) => <article key={item.actionId} className={`notification-item notification-item--${item.status}`}><span className="notification-item__icon"><StatusIcon status={item.status} size={16} /></span><div><strong>{item.message}</strong><p>{new Date(item.timestamp).toLocaleString('ko-KR')}{item.revision ? ` · revision ${item.revision}` : ''}</p>{item.recovery && <small>{item.recovery.message ?? (item.recovery.attempted ? '복구 작업을 수행했습니다.' : '복구 작업이 필요합니다.')}</small>}<code>{item.actionId}</code></div></article>) : <div className="notification-empty"><Bell size={25} /><strong>표시할 알림이 없습니다.</strong></div>}</div>
      <footer><Button variant="ghost" onClick={() => { onViewAll(); onClose(); }}>전체 활동 기록 보기</Button></footer>
    </aside>
  </div>;
}

export default function App() {
  const [bootstrap, setBootstrap] = useState<BootstrapPayload | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [page, setPage] = useState<PageId>('dashboard');
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [commandOpen, setCommandOpen] = useState(false);
  const [editor, setEditor] = useState<{ profile: Profile; isNew: boolean } | null>(null);
  const [saving, setSaving] = useState(false);
  const [dataBusy, setDataBusy] = useState(false);
  const [savePhase, setSavePhase] = useState<SavePhase>('idle');
  const { toasts, dismiss, push } = useToastQueue();

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      setBootstrap(await keyforgeBridge.bootstrap());
    } catch (error) {
      setLoadError(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const navigate = useCallback((nextPage: PageId) => {
    setPage(nextPage);
    setSidebarOpen(false);
  }, []);

  const openNewProfile = useCallback(() => setEditor({ profile: makeProfile(), isNew: true }), []);

  useEffect(() => {
    const handleKeys = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setCommandOpen(true);
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'n' && !editor) {
        event.preventDefault();
        openNewProfile();
      }
    };
    window.addEventListener('keydown', handleKeys);
    return () => window.removeEventListener('keydown', handleKeys);
  }, [editor, openNewProfile]);

  const theme = bootstrap?.settings.preferences.theme ?? 'system';
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const applyTheme = () => {
      const resolved = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme;
      document.documentElement.dataset.theme = resolved;
      document.querySelector('meta[name="theme-color"]')?.setAttribute('content', resolved === 'dark' ? '#0d1117' : '#f4f7fb');
    };
    applyTheme();
    media.addEventListener('change', applyTheme);
    return () => media.removeEventListener('change', applyTheme);
  }, [theme]);

  const appendActivity = useCallback((result: ActionResult) => {
    setBootstrap((current) => current ? { ...current, activity: [result, ...current.activity.filter((item) => item.actionId !== result.actionId)] } : current);
  }, []);

  const persistSettings = useCallback(async (draft: KeyForgeSettings, successTitle?: string) => {
    if (!bootstrap || saving) return false;
    const previous = bootstrap.settings;
    setSaving(true);
    setSavePhase('validating');
    try {
      await new Promise((resolve) => setTimeout(resolve, 90));
      setSavePhase('writing');
      const responsePromise = keyforgeBridge.saveSettings(draft, previous.revision);
      await new Promise((resolve) => setTimeout(resolve, 110));
      setSavePhase('applying');
      const { settings, result } = await responsePromise;
      setBootstrap((current) => current ? {
        ...current,
        settings,
        runtime: { ...current.runtime, activeProfileCount: settings.profiles.filter((profile) => profile.enabled && !profile.archived).length },
        activity: [result, ...current.activity],
      } : current);
      setSavePhase('saved');
      push(successTitle ?? '설정을 저장하고 적용했습니다.', result.status, `revision ${settings.revision}`);
      window.setTimeout(() => setSavePhase('idle'), 2400);
      return true;
    } catch (error) {
      setSavePhase('error');
      const message = error instanceof Error ? error.message : String(error);
      if (error instanceof KeyForgeCommandError && error.result) {
        appendActivity(error.result);
      }
      setBootstrap((current) => current && current.settings.revision === previous.revision
        ? { ...current, settings: previous }
        : current);
      push('설정을 저장하지 못했습니다.', 'error', `${message} 기존 revision ${previous.revision}은 유지됩니다.`, { label: '다시 시도', run: () => void persistSettings(draft, successTitle) });
      return false;
    } finally {
      setSaving(false);
    }
  }, [appendActivity, bootstrap, push, saving]);

  const updateProfile = useCallback(async (profile: Profile) => {
    if (!bootstrap) return;
    const exists = bootstrap.settings.profiles.some((item) => item.id === profile.id);
    const profiles = exists ? bootstrap.settings.profiles.map((item) => item.id === profile.id ? profile : item) : [...bootstrap.settings.profiles, profile];
    const ok = await persistSettings({ ...bootstrap.settings, profiles }, `${profile.name} 프로필을 저장하고 적용했습니다.`);
    if (ok) setEditor(null);
  }, [bootstrap, persistSettings]);

  const mutateProfile = useCallback((profile: Profile, change: (profile: Profile) => Profile, message: string) => {
    if (!bootstrap) return;
    const profiles = bootstrap.settings.profiles.map((item) => item.id === profile.id ? change(item) : item);
    void persistSettings({ ...bootstrap.settings, profiles }, message);
  }, [bootstrap, persistSettings]);

  const profileActions = useMemo(() => ({
    onEdit: (profile: Profile) => setEditor({ profile, isNew: false }),
    onNew: openNewProfile,
    onToggle: (profile: Profile, enabled: boolean) => mutateProfile(profile, (item) => ({ ...item, enabled, updatedAt: new Date().toISOString() }), enabled ? `${profile.name} 프로필을 활성화했습니다.` : `${profile.name} 프로필을 비활성화했습니다.`),
    onDuplicate: (profile: Profile) => {
      if (!bootstrap) return;
      const timestamp = new Date().toISOString();
      const duplicate: Profile = { ...structuredClone(profile), id: makeId(), name: `${profile.name} 복사본`, enabled: false, enableOnStartup: false, createdAt: timestamp, updatedAt: timestamp, lastRunAt: null, rules: profile.rules.map((rule, index) => ({ ...structuredClone(rule), id: makeId(), order: index })) };
      void persistSettings({ ...bootstrap.settings, profiles: [...bootstrap.settings.profiles, duplicate] }, '프로필을 복제했습니다.');
    },
    onArchive: (profile: Profile) => mutateProfile(profile, (item) => ({ ...item, archived: !item.archived, enabled: item.archived ? item.enabled : false, updatedAt: new Date().toISOString() }), profile.archived ? '프로필을 보관에서 복원했습니다.' : '프로필을 보관했습니다.'),
    onDelete: (profile: Profile) => {
      if (!bootstrap || !window.confirm(`“${profile.name}” 프로필을 삭제할까요? 이 작업은 취소할 수 없습니다.`)) return;
      void persistSettings({ ...bootstrap.settings, profiles: bootstrap.settings.profiles.filter((item) => item.id !== profile.id) }, '프로필을 삭제했습니다.');
    },
  }), [bootstrap, mutateProfile, openNewProfile, persistSettings]);

  const toggleEngine = useCallback(async () => {
    if (!bootstrap) return;
    const paused = bootstrap.runtime.engineState !== 'paused';
    setBootstrap({ ...bootstrap, runtime: { ...bootstrap.runtime, engineState: paused ? 'paused' : 'running' } });
    try {
      const result = await keyforgeBridge.setEnginePaused(paused);
      appendActivity(result);
      push(result.message, result.status);
    } catch (error) {
      setBootstrap({ ...bootstrap, runtime: { ...bootstrap.runtime, engineState: paused ? 'running' : 'paused' } });
      push('입력 엔진 상태를 변경하지 못했습니다.', 'error', error instanceof Error ? error.message : String(error));
    }
  }, [appendActivity, bootstrap, push]);

  const updatePreferences = useCallback((preferences: AppPreferences) => {
    if (!bootstrap || saving || dataBusy) return;
    const launchAtLoginChanged = preferences.launchAtLogin !== bootstrap.settings.preferences.launchAtLogin;
    const successTitle = launchAtLoginChanged
      ? preferences.launchAtLogin
        ? 'Windows 로그인 시 KeyForge를 자동 시작하도록 등록했습니다.'
        : 'Windows 로그인 시 KeyForge 자동 시작 등록을 해제했습니다.'
      : '일반 설정을 저장했습니다.';
    setBootstrap({ ...bootstrap, settings: { ...bootstrap.settings, preferences } });
    void persistSettings({ ...bootstrap.settings, preferences }, successTitle);
  }, [bootstrap, dataBusy, persistSettings, saving]);

  const updateEngine = useCallback((engine: EngineSettings) => {
    if (!bootstrap || saving || dataBusy) return;
    setBootstrap({ ...bootstrap, settings: { ...bootstrap.settings, engine } });
    void persistSettings({ ...bootstrap.settings, engine }, '입력 엔진 설정을 저장했습니다.');
  }, [bootstrap, dataBusy, persistSettings, saving]);

  const backup = useCallback(async () => {
    setDataBusy(true);
    try { const result = await keyforgeBridge.createBackup(); appendActivity(result); push(result.message, result.status); }
    catch (error) {
      if (error instanceof KeyForgeCommandError && error.result) appendActivity(error.result);
      push('백업을 만들지 못했습니다.', 'error', error instanceof Error ? error.message : String(error));
    }
    finally { setDataBusy(false); }
  }, [appendActivity, push]);

  const restore = useCallback(async () => {
    if (!bootstrap || !window.confirm('최근 백업을 복원할까요? 현재 설정은 새 revision으로 교체됩니다.')) return;
    setDataBusy(true);
    try {
      const { settings, result } = await keyforgeBridge.restoreBackup(bootstrap.settings.revision);
      setBootstrap({ ...bootstrap, settings, activity: [result, ...bootstrap.activity] });
      push(result.message, result.status);
    } catch (error) {
      if (error instanceof KeyForgeCommandError && error.result) appendActivity(error.result);
      push('백업을 복원하지 못했습니다.', 'error', error instanceof Error ? error.message : String(error));
    }
    finally { setDataBusy(false); }
  }, [appendActivity, bootstrap, push]);

  if (loadError) return <main className="fatal-screen"><div className="brand-mark brand-mark--large">K</div><h1>KeyForge를 시작하지 못했습니다.</h1><p>{loadError}</p><Button variant="primary" icon={<RefreshCcw size={17} />} onClick={() => void load()}>다시 시도</Button></main>;
  if (!bootstrap) return <LoadingScreen />;

  const renderPage = () => {
    switch (page) {
      case 'dashboard': return <DashboardPage settings={bootstrap.settings} runtime={bootstrap.runtime} activity={bootstrap.activity} actions={profileActions} onNavigate={navigate} />;
      case 'profiles': return <ProfilesPage profiles={bootstrap.settings.profiles} actions={profileActions} />;
      case 'key-inspector': return <KeyInspectorPage />;
      case 'devices': return <DevicesPage />;
      case 'activity': return <ActivityPage activity={bootstrap.activity} />;
      case 'settings': return <SettingsPage settings={bootstrap.settings} settingsPath={bootstrap.settingsPath} runtime={bootstrap.runtime} onPreferences={updatePreferences} onEngine={updateEngine} onBackup={() => void backup()} onRestore={() => void restore()} busy={dataBusy || saving} native={keyforgeBridge.isNative()} />;
    }
  };

  const unread = bootstrap.activity.filter((item) => item.status !== 'success').length;
  const currentNav = navigation.find((item) => item.id === page);
  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="topbar__brand"><IconButton label="메뉴 열기" className="mobile-menu" onClick={() => setSidebarOpen(true)}><Menu size={20} /></IconButton><div className="brand-mark">K</div><strong>KeyForge</strong><span className="version-badge" aria-label={`KeyForge 버전 ${APP_VERSION}`}>v{APP_VERSION}</span></div>
        <div className={`engine-pill engine-pill--${bootstrap.runtime.engineState}`}><span className="engine-pulse" /><span>입력 엔진</span><strong>{bootstrap.runtime.engineState === 'running' ? '실행 중' : bootstrap.runtime.engineState === 'paused' ? '일시정지' : '오류'}</strong></div>
        <div className="topbar__actions">
          <button type="button" className="command-button" onClick={() => setCommandOpen(true)}><Search size={16} /><span>빠른 검색</span><kbd>Ctrl K</kbd></button>
          <Button variant={bootstrap.runtime.engineState === 'paused' ? 'primary' : 'secondary'} size="small" icon={bootstrap.runtime.engineState === 'paused' ? <Play size={15} /> : <Pause size={15} />} onClick={() => void toggleEngine()}>{bootstrap.runtime.engineState === 'paused' ? '다시 시작' : '모두 일시정지'}</Button>
          <div className="notification-button"><IconButton label={`알림 센터${unread ? `, 확인할 알림 ${unread}개` : ''}`} onClick={() => setNotificationsOpen(true)}><Bell size={19} /></IconButton>{unread > 0 && <span>{Math.min(unread, 9)}</span>}</div>
          <IconButton disabled={saving || dataBusy} label={bootstrap.settings.preferences.theme === 'dark' ? '밝은 테마' : '어두운 테마'} onClick={() => updatePreferences({ ...bootstrap.settings.preferences, theme: bootstrap.settings.preferences.theme === 'dark' ? 'light' : 'dark' })}>{bootstrap.settings.preferences.theme === 'dark' ? <Sun size={18} /> : <Moon size={18} />}</IconButton>
        </div>
      </header>

      <div className="shell-body">
        {sidebarOpen && <button type="button" className="sidebar-scrim" aria-label="메뉴 닫기" onClick={() => setSidebarOpen(false)} />}
        <aside className={`sidebar ${sidebarOpen ? 'is-open' : ''}`}>
          <div className="sidebar__mobile-head"><strong>탐색</strong><IconButton label="메뉴 닫기" onClick={() => setSidebarOpen(false)}><ChevronLeft size={19} /></IconButton></div>
          <nav aria-label="주 메뉴">{navigation.map((item) => { const Icon = item.icon; return <button type="button" key={item.id} className={page === item.id ? 'is-active' : ''} aria-current={page === item.id ? 'page' : undefined} onClick={() => navigate(item.id)}><Icon size={19} /><span>{item.label}</span>{item.id === 'activity' && unread > 0 && <small>{unread}</small>}</button>; })}</nav>
          <div className="sidebar__safety"><div><ShieldIcon /><span><strong>보호 모드</strong><small>주입 반복 차단 중</small></span></div><kbd>{bootstrap.settings.engine.emergencyStop.join(' + ')}</kbd><small>비상 정지</small></div>
          {!keyforgeBridge.isNative() && <div className="mock-chip"><Gauge size={14} /><span>브라우저 데모</span></div>}
        </aside>

        <main className="content" id="main-content">
          <div className="mobile-page-title"><span>{currentNav && <currentNav.icon size={17} />}</span><strong>{currentNav?.label}</strong></div>
          {renderPage()}
        </main>
      </div>

      <footer className="statusbar">
        <span><span className={`status-dot ${bootstrap.runtime.engineState === 'running' ? 'is-running' : ''}`} />활성 프로필 {bootstrap.runtime.activeProfileCount}개</span>
        <span className={`save-state save-state--${savePhase}`}>{savePhase === 'validating' ? '설정 검증 중…' : savePhase === 'writing' ? '디스크에 저장 중…' : savePhase === 'applying' ? '런타임에 적용 중…' : savePhase === 'saved' ? <><CheckCircle2 size={13} /> 저장됨</> : savePhase === 'error' ? '저장 실패' : `revision ${bootstrap.settings.revision}`}</span>
        <span>마지막 저장 {relativeTime(bootstrap.settings.updatedAt)}</span>
        <span className="statusbar__shortcut"><Command size={13} /> 비상 정지 <kbd>{bootstrap.settings.engine.emergencyStop.join(' + ')}</kbd></span>
      </footer>

      <ProfileEditor open={Boolean(editor)} profile={editor?.profile ?? null} isNew={editor?.isNew ?? false} saving={saving} onClose={() => setEditor(null)} onSave={updateProfile} />
      <CommandPalette open={commandOpen} onClose={() => setCommandOpen(false)} onNavigate={navigate} onNew={openNewProfile} />
      <NotificationCenter open={notificationsOpen} activity={bootstrap.activity} onClose={() => setNotificationsOpen(false)} onViewAll={() => navigate('activity')} />
      <ToastRegion toasts={toasts} dismiss={dismiss} />
    </div>
  );
}

function ShieldIcon() { return <Gauge size={18} />; }

function relativeTime(value?: string | null) {
  if (!value) return '기록 없음';
  const seconds = Math.max(0, Math.round((Date.now() - new Date(value).getTime()) / 1000));
  if (seconds < 60) return '방금 전';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}분 전`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}시간 전`;
  return new Date(value).toLocaleDateString('ko-KR');
}
