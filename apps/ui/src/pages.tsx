import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from 'react';
import {
  Activity,
  AlertTriangle,
  Archive,
  ArrowRight,
  Blocks,
  ChevronRight,
  Copy,
  Download,
  ExternalLink,
  FileInput,
  Filter,
  FolderOpen,
  Gauge,
  Globe2,
  Grid2X2,
  HardDriveDownload,
  Info,
  Keyboard,
  KeyboardIcon,
  LayoutGrid,
  List,
  Monitor,
  Moon,
  MoreHorizontal,
  Plus,
  Radio,
  RefreshCcw,
  RotateCcw,
  Search,
  Settings2,
  ShieldCheck,
  Sun,
  Trash2,
  Upload,
  Usb,
  Zap,
} from 'lucide-react';
import { actionLabel } from './components/ProfileEditor';
import { makeDeviceSelector, selectorLabel } from './deviceSelectors';
import { Badge, Button, Callout, EmptyState, IconButton, StatusIcon, Toggle } from './components/common';
import { keyboardEventToKey, orderChord } from './keyCatalog';
import { keyforgeBridge } from './lib/bridge';
import type {
  ActionResult,
  AppPreferences,
  EngineSettings,
  KeyboardDeviceInfo,
  PageId,
  Profile,
  RuntimeState,
  Settings,
  ThemePreference,
} from './types';

const relativeTime = (value?: string | null) => {
  if (!value) return '실행 기록 없음';
  const seconds = Math.round((Date.now() - new Date(value).getTime()) / 1000);
  if (seconds < 60) return '방금 전';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}분 전`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}시간 전`;
  return new Date(value).toLocaleDateString('ko-KR');
};

const hasConnectedKeyboardActivation = (profile: Profile) => profile.activation.connectedKeyboards.length > 0;

const scopeMeta = (profile: Profile) => {
  if (hasConnectedKeyboardActivation(profile)) return { label: '연결 조건', tone: 'purple' as const, icon: <Keyboard size={13} /> };
  if (profile.scope.kind === 'global') return { label: '전역', tone: 'accent' as const, icon: <Globe2 size={13} /> };
  if (profile.scope.kind === 'device') return { label: '장치 조건', tone: 'purple' as const, icon: <Keyboard size={13} /> };
  if (profile.scope.kind === 'combined') return { label: '앱 + 장치', tone: 'purple' as const, icon: <Blocks size={13} /> };
  return { label: '앱 조건', tone: 'purple' as const, icon: <Monitor size={13} /> };
};

const meaningfulManufacturer = (value?: string | null) => {
  if (!value) return null;
  const normalized = value.trim().toLowerCase();
  return normalized === '(표준 키보드)' || normalized === '(standard keyboards)' ? null : value;
};

const humanConnectionLabel = (device: KeyboardDeviceInfo) => {
  if (device.isVirtual) return '가상·원격 입력 경로';
  const upperInstance = device.instanceId?.toUpperCase() ?? '';
  const upperPath = device.devicePath.toUpperCase();
  const location = device.locationPaths[0]?.toUpperCase() ?? '';
  if (upperInstance.startsWith('ACPI\\') || location.includes('ACPI(KBD_)')) return '노트북/내장 키보드 계열';
  if (upperInstance.includes('BTH') || upperPath.includes('BTH')) return 'Bluetooth 키보드 계열';
  if (location.includes('USBROOT') || upperInstance.startsWith('HID\\VID_') || upperPath.includes('HID#VID_')) return 'USB 또는 2.4GHz 리시버 HID';
  if (upperInstance.startsWith('HID\\INTC')) return '내장 핫키/펌웨어 컬렉션';
  return '일반 HID 키보드 인터페이스';
};

const humanLocationLabel = (device: KeyboardDeviceInfo) => {
  const location = device.locationPaths[0];
  if (!location) return null;
  const usbRoot = location.match(/USBROOT\((\d+)\)/i)?.[1];
  const usbPorts = [...location.matchAll(/USB\((\d+)\)/gi)].map((match) => match[1]);
  if (usbRoot || usbPorts.length) {
    return [usbRoot ? `USB 루트 ${usbRoot}` : null, usbPorts.length ? `포트 ${usbPorts.join(' → ')}` : null].filter(Boolean).join(' · ');
  }
  if (location.toUpperCase().includes('ACPI(KBD_)')) return 'ACPI 내장 키보드 경로';
  return location;
};

const containerGroupLabel = (device: KeyboardDeviceInfo, devices: KeyboardDeviceInfo[]) => {
  if (!device.containerId) return '독립 인터페이스';
  const siblings = devices.filter((candidate) => candidate.containerId === device.containerId).length;
  return siblings > 1 ? `같은 물리 장치 그룹 ${siblings}개 중 하나` : '단일 물리 장치 그룹';
};

function useConnectedKeyboards(autoLoad = true) {
  const request = useRef(0);
  const [devices, setDevices] = useState<KeyboardDeviceInfo[]>([]);
  const [loading, setLoading] = useState(autoLoad);
  const [error, setError] = useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null);

  const refresh = useCallback(async () => {
    const current = ++request.current;
    setLoading(true);
    setError(null);
    try {
      const next = await keyforgeBridge.listConnectedKeyboards();
      if (request.current !== current) return;
      setDevices(next);
      setUpdatedAt(new Date());
    } catch (reason) {
      if (request.current !== current) return;
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (request.current === current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (autoLoad) void refresh();
    return () => { request.current += 1; };
  }, [autoLoad, refresh]);

  return { devices, loading, error, updatedAt, refresh };
}

function ConnectedKeyboardSpotlight({
  devices,
  loading,
  error,
  updatedAt,
  onRefresh,
  title,
  description,
  emptyDescription,
  compact = false,
}: {
  devices: KeyboardDeviceInfo[];
  loading: boolean;
  error: string | null;
  updatedAt: Date | null;
  onRefresh?: () => void;
  title: string;
  description: string;
  emptyDescription: string;
  compact?: boolean;
}) {
  return <section className={`connected-keyboard-spotlight ${compact ? 'connected-keyboard-spotlight--compact' : ''}`} aria-label={title}>
    <div className="connected-keyboard-spotlight__head">
      <div>
        <span className="page-eyebrow">CONNECTED KEYBOARDS</span>
        <h2>{title}</h2>
        <p>{description}</p>
      </div>
      <div className="connected-keyboard-spotlight__actions">
        <Badge tone="success">{devices.length}개 연결됨</Badge>
        {onRefresh && <Button size="small" icon={<RefreshCcw className={loading ? 'spin' : ''} size={15} />} disabled={loading} onClick={onRefresh}>{loading ? '새로 읽는 중…' : '다시 읽기'}</Button>}
      </div>
    </div>
    {updatedAt && <p className="connected-keyboard-spotlight__timestamp">마지막 확인 {updatedAt.toLocaleTimeString('ko-KR')}</p>}
    {error && <Callout tone="danger" title="연결된 키보드 목록을 읽지 못했습니다.">{error}</Callout>}
    {loading && !devices.length ? <div className="connected-keyboard-spotlight__empty"><RefreshCcw className="spin" size={18} />현재 연결된 키보드를 읽는 중입니다.</div> : null}
    {!loading && !devices.length ? <div className="connected-keyboard-spotlight__empty"><Keyboard size={18} />{emptyDescription}</div> : null}
    {devices.length ? <div className="connected-keyboard-spotlight__grid">{devices.map((device) => {
      const manufacturer = meaningfulManufacturer(device.manufacturer);
      const connectionLabel = humanConnectionLabel(device);
      const location = humanLocationLabel(device);
      const group = containerGroupLabel(device, devices);
      return <article className="connected-keyboard-pill" key={device.id}>
        <span className="connected-keyboard-pill__icon"><Keyboard size={18} /></span>
        <div className="connected-keyboard-pill__body">
          <strong>{device.name}</strong>
          <p>{manufacturer ?? connectionLabel}</p>
          <small>{location ?? group}</small>
        </div>
        <div className="connected-keyboard-pill__meta">
          <Badge tone={device.isVirtual ? 'purple' : 'success'}>{device.vendorId && device.productId ? `VID_${device.vendorId} / PID_${device.productId}` : connectionLabel}</Badge>
          <span>{group}</span>
        </div>
      </article>;
    })}</div> : null}
  </section>;
}

export function PageHeader({
  eyebrow,
  title,
  description,
  actions,
}: {
  eyebrow?: string;
  title: string;
  description: string;
  actions?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div>
        {eyebrow && <span className="page-eyebrow">{eyebrow}</span>}
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {actions && <div className="page-header__actions">{actions}</div>}
    </header>
  );
}

export function MetricCard({ icon, label, value, detail, tone = 'blue' }: { icon: ReactNode; label: string; value: string | number; detail: string; tone?: 'blue' | 'green' | 'purple' | 'amber' }) {
  return (
    <article className="metric-card">
      <div className={`metric-card__icon metric-card__icon--${tone}`}>{icon}</div>
      <div><span>{label}</span><strong>{value}</strong><small>{detail}</small></div>
    </article>
  );
}

function ProfileMenu({ profile, onDuplicate, onArchive, onDelete }: { profile: Profile; onDuplicate: () => void; onArchive: () => void; onDelete: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="menu-anchor">
      <IconButton label={`${profile.name} 더보기`} onClick={() => setOpen(!open)}><MoreHorizontal size={18} /></IconButton>
      {open && (
        <div className="context-menu">
          <button type="button" onClick={() => { onDuplicate(); setOpen(false); }}><Copy size={15} /> 복제</button>
          <button type="button" onClick={() => { onArchive(); setOpen(false); }}><Archive size={15} /> {profile.archived ? '보관 해제' : '보관'}</button>
          <button type="button" className="is-danger" onClick={() => { onDelete(); setOpen(false); }}><Trash2 size={15} /> 삭제</button>
        </div>
      )}
    </div>
  );
}

export function ProfileCard({
  profile,
  compact = false,
  onEdit,
  onToggle,
  onDuplicate,
  onArchive,
  onDelete,
}: {
  profile: Profile;
  compact?: boolean;
  onEdit: () => void;
  onToggle: (enabled: boolean) => void;
  onDuplicate: () => void;
  onArchive: () => void;
  onDelete: () => void;
}) {
  const scope = scopeMeta(profile);
  const rulePreview = profile.rules.slice(0, 2);
  return (
    <article className={`profile-card ${compact ? 'profile-card--compact' : ''} ${!profile.enabled ? 'is-disabled' : ''}`}>
      <div className="profile-card__top">
        <div className={`profile-avatar ${profile.enabled ? 'is-active' : ''}`}><KeyboardIcon size={20} /></div>
        <div className="profile-card__title"><div><h3>{profile.name}</h3><Badge tone={scope.tone}>{scope.icon}{scope.label}</Badge></div><span>{profile.rules.length}개 규칙 · {relativeTime(profile.lastRunAt)}</span></div>
        <ProfileMenu profile={profile} onDuplicate={onDuplicate} onArchive={onArchive} onDelete={onDelete} />
      </div>
      {!compact && (
        <div className="profile-card__rules">
          {rulePreview.length ? rulePreview.map((rule) => (
            <div key={rule.id}><span className="mini-keycap">{rule.trigger.kind === 'keyboard' ? rule.trigger.chord.join(' + ') : `${rule.trigger.button} click`}</span><ArrowRight size={14} /><span>{actionLabel(rule.action)}</span></div>
          )) : <div className="no-rules"><Info size={14} /> 아직 규칙이 없습니다.</div>}
          {profile.rules.length > 2 && <small>+{profile.rules.length - 2}개 더 있음</small>}
        </div>
      )}
      <div className="profile-card__footer">
        <div className="profile-state"><span className={`status-dot ${profile.enabled ? 'is-running' : ''}`} />{profile.enabled ? '활성' : '비활성'}</div>
        <div><Toggle checked={profile.enabled} onChange={onToggle} label={`${profile.name} 활성화`} /><Button size="small" onClick={onEdit}>편집</Button></div>
      </div>
    </article>
  );
}

type ProfileActions = {
  onEdit: (profile: Profile) => void;
  onNew: () => void;
  onToggle: (profile: Profile, enabled: boolean) => void;
  onDuplicate: (profile: Profile) => void;
  onArchive: (profile: Profile) => void;
  onDelete: (profile: Profile) => void;
};

export function DashboardPage({ settings, runtime, activity, actions, onNavigate }: { settings: Settings; runtime: RuntimeState; activity: ActionResult[]; actions: ProfileActions; onNavigate: (page: PageId) => void }) {
  const active = settings.profiles.filter((profile) => profile.enabled && !profile.archived);
  const errors = activity.filter((item) => item.status === 'error');
  return (
    <div className="page">
      <PageHeader eyebrow="오늘의 KeyForge" title="키 입력을 원하는 방식으로" description="전역 프로필은 앱을 따로 지정하지 않아도 어디서나 바로 동작합니다." actions={<Button variant="primary" icon={<Plus size={17} />} onClick={actions.onNew}>새 프로필</Button>} />
      <section className="metrics-grid" aria-label="상태 요약">
        <MetricCard icon={<Radio size={21} />} label="입력 엔진" value={runtime.engineState === 'running' ? '실행 중' : runtime.engineState === 'paused' ? '일시정지' : '오류'} detail={runtime.hookInstalled ? '키보드 후크 정상' : '후크 확인 필요'} tone={runtime.engineState === 'running' ? 'green' : 'amber'} />
        <MetricCard icon={<LayersIcon />} label="활성 프로필" value={runtime.activeProfileCount} detail={`전체 ${settings.profiles.filter((item) => !item.archived).length}개 중`} tone="blue" />
        <MetricCard icon={<Zap size={21} />} label="등록된 규칙" value={settings.profiles.reduce((sum, profile) => sum + profile.rules.length, 0)} detail="주입 입력 반복 방지 중" tone="purple" />
        <MetricCard icon={<AlertTriangle size={21} />} label="최근 오류" value={errors.length} detail={errors[0] ? relativeTime(errors[0].timestamp) : '문제 없음'} tone={errors.length ? 'amber' : 'green'} />
      </section>

      <Callout tone="warning" title="비상 정지는 언제나 우선합니다.">
        <span className="inline-key">{settings.engine.emergencyStop.join(' + ')}</span>를 누르면 모든 입력 규칙이 즉시 멈춥니다.
      </Callout>

      <section className="dashboard-section">
        <div className="section-heading"><div><h2>활성 프로필</h2><p>현재 입력을 처리하고 있는 프로필입니다.</p></div><Button variant="ghost" size="small" onClick={() => onNavigate('profiles')}>전체 보기 <ChevronRight size={16} /></Button></div>
        {active.length ? (
          <div className="profile-grid">{active.slice(0, 3).map((profile) => <ProfileCard key={profile.id} profile={profile} onEdit={() => actions.onEdit(profile)} onToggle={(enabled) => actions.onToggle(profile, enabled)} onDuplicate={() => actions.onDuplicate(profile)} onArchive={() => actions.onArchive(profile)} onDelete={() => actions.onDelete(profile)} />)}</div>
        ) : <EmptyState icon={<Keyboard size={26} />} title="활성 프로필이 없습니다" description="새 전역 프로필을 만들어 키 매핑을 시작하세요." action={<Button variant="primary" onClick={actions.onNew}>새 프로필</Button>} />}
      </section>

      <div className="dashboard-bottom-grid">
        <section className="panel-card">
          <div className="panel-card__head"><div><h2>최근 활동</h2><p>저장과 적용 결과를 확인하세요.</p></div><IconButton label="활동 기록 열기" onClick={() => onNavigate('activity')}><ExternalLink size={17} /></IconButton></div>
          <div className="mini-activity-list">
            {activity.slice(0, 4).map((item) => <div key={item.actionId} className={`mini-activity mini-activity--${item.status}`}><span className="mini-activity__icon"><StatusIcon status={item.status} size={15} /></span><div><strong>{item.message}</strong><small>{relativeTime(item.timestamp)}{item.revision ? ` · revision ${item.revision}` : ''}</small></div></div>)}
          </div>
        </section>
        <section className="panel-card quick-actions">
          <div className="panel-card__head"><div><h2>빠른 작업</h2><p>키 매핑과 설정으로 이동합니다.</p></div></div>
          <div className="quick-action-grid">
            <button type="button" onClick={() => onNavigate('profiles')}><Keyboard size={19} /><span><strong>키 매핑</strong><small>프로필 규칙 편집</small></span><ChevronRight size={16} /></button>
            <button type="button" onClick={actions.onNew}><Plus size={19} /><span><strong>새 프로필</strong><small>전역 규칙 만들기</small></span><ChevronRight size={16} /></button>
            <button type="button" onClick={() => onNavigate('settings')}><HardDriveDownload size={19} /><span><strong>설정 백업</strong><small>안전하게 복원</small></span><ChevronRight size={16} /></button>
          </div>
        </section>
      </div>
    </div>
  );
}

function LayersIcon() { return <Grid2X2 size={21} />; }

export function ProfilesPage({ profiles, actions }: { profiles: Profile[]; actions: ProfileActions }) {
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<'all' | 'active' | 'inactive' | 'global' | 'conditional' | 'archived'>('all');
  const [view, setView] = useState<'grid' | 'list'>('grid');
  const filtered = profiles.filter((profile) => {
    const matchesSearch = profile.name.toLowerCase().includes(query.toLowerCase());
    const matchesFilter = filter === 'all' ? !profile.archived : filter === 'active' ? profile.enabled && !profile.archived : filter === 'inactive' ? !profile.enabled && !profile.archived : filter === 'global' ? profile.scope.kind === 'global' && !hasConnectedKeyboardActivation(profile) && !profile.archived : filter === 'conditional' ? (profile.scope.kind !== 'global' || hasConnectedKeyboardActivation(profile)) && !profile.archived : profile.archived;
    return matchesSearch && matchesFilter;
  });

  return (
    <div className="page">
      <PageHeader title="프로필" description="입력 규칙을 목적별로 묶고 한 번에 켜거나 끕니다." actions={<><Button icon={<Upload size={16} />}>가져오기</Button><Button variant="primary" icon={<Plus size={17} />} onClick={actions.onNew}>새 프로필</Button></>} />
      <div className="toolbar-card">
        <label className="search-field"><Search size={17} /><input aria-label="프로필 검색" placeholder="프로필 검색" value={query} onChange={(event) => setQuery(event.target.value)} /></label>
        <div className="filter-tabs" role="group" aria-label="프로필 필터">
          {([['all', '전체'], ['active', '활성'], ['inactive', '비활성'], ['global', '전역'], ['conditional', '조건부'], ['archived', '보관']] as const).map(([id, label]) => <button type="button" key={id} className={filter === id ? 'is-active' : ''} onClick={() => setFilter(id)}>{label}</button>)}
        </div>
        <div className="view-switch"><IconButton label="카드 보기" aria-pressed={view === 'grid'} onClick={() => setView('grid')}><LayoutGrid size={17} /></IconButton><IconButton label="목록 보기" aria-pressed={view === 'list'} onClick={() => setView('list')}><List size={17} /></IconButton></div>
      </div>
      <div className="result-count"><strong>{filtered.length}</strong>개 프로필 <span>· {filter === 'all' ? '보관 제외' : '선택한 조건'}</span></div>
      {filtered.length ? <div className={view === 'grid' ? 'profile-grid' : 'profile-list'}>{filtered.map((profile) => <ProfileCard compact={view === 'list'} key={profile.id} profile={profile} onEdit={() => actions.onEdit(profile)} onToggle={(enabled) => actions.onToggle(profile, enabled)} onDuplicate={() => actions.onDuplicate(profile)} onArchive={() => actions.onArchive(profile)} onDelete={() => actions.onDelete(profile)} />)}</div> : <EmptyState icon={<Filter size={25} />} title="조건에 맞는 프로필이 없습니다" description="검색어나 필터를 바꾸거나 새 프로필을 만드세요." action={<Button onClick={() => { setFilter('all'); setQuery(''); }}>필터 초기화</Button>} />}
    </div>
  );
}

interface InspectedKeyEvent {
  id: number;
  phase: 'key-down' | 'key-up';
  mappedKey: string;
  key: string;
  code: string;
  keyCode: number;
  location: number;
  modifiers: string[];
  repeat: boolean;
  composing: boolean;
  trusted: boolean;
  observedAt: string;
}

const locationLabel = (location: number) => {
  switch (location) {
    case 1: return '왼쪽';
    case 2: return '오른쪽';
    case 3: return '숫자패드';
    default: return '표준';
  }
};

const hex = (value: number) => `0x${value.toString(16).toUpperCase().padStart(2, '0')}`;

export function KeyInspectorPage() {
  const captureRef = useRef<HTMLDivElement>(null);
  const sequence = useRef(0);
  const pressed = useRef(new Set<string>());
  const [pressedKeys, setPressedKeys] = useState<string[]>([]);
  const [events, setEvents] = useState<InspectedKeyEvent[]>([]);
  const {
    devices,
    loading: devicesLoading,
    error: deviceError,
    updatedAt: devicesUpdatedAt,
    refresh: refreshDevices,
  } = useConnectedKeyboards();

  useEffect(() => {
    captureRef.current?.focus();
    return () => pressed.current.clear();
  }, []);

  const inspect = (event: ReactKeyboardEvent<HTMLDivElement>, phase: InspectedKeyEvent['phase']) => {
    event.preventDefault();
    event.stopPropagation();
    const native = event.nativeEvent;
    const mappedKey = keyboardEventToKey(native) ?? 'Unidentified';
    if (phase === 'key-down') pressed.current.add(mappedKey);
    else pressed.current.delete(mappedKey);
    setPressedKeys(orderChord(pressed.current));
    const modifiers = [
      native.ctrlKey && 'Ctrl',
      native.shiftKey && 'Shift',
      native.altKey && 'Alt',
      native.metaKey && 'Meta',
    ].filter((value): value is string => Boolean(value));
    const inspected: InspectedKeyEvent = {
      id: ++sequence.current,
      phase,
      mappedKey,
      key: native.key || '(없음)',
      code: native.code || 'Unidentified',
      keyCode: native.keyCode || native.which || 0,
      location: native.location,
      modifiers,
      repeat: native.repeat,
      composing: native.isComposing,
      trusted: native.isTrusted,
      observedAt: new Date().toLocaleTimeString('ko-KR', { hour12: false }),
    };
    setEvents((current) => [inspected, ...current].slice(0, 20));
  };

  const clear = () => {
    pressed.current.clear();
    setPressedKeys([]);
    setEvents([]);
    captureRef.current?.focus();
  };

  const latest = events[0];
  return <div className="page key-inspector-page">
    <PageHeader title="키 확인" description="키보드 이벤트의 이름과 코드를 확인합니다. 검사 결과는 저장되지 않습니다." actions={<Button icon={<Trash2 size={16} />} onClick={clear}>기록 지우기</Button>} />
    <Callout title="개인정보 보호를 위해 이 화면 안에서만 감지합니다.">
      아래 검사 영역에 포커스가 있을 때만 동작합니다. 탭을 벗어나면 메모리 기록이 사라지며 설정과 활동 기록에 저장하지 않습니다.
    </Callout>
    <ConnectedKeyboardSpotlight
      devices={devices}
      loading={devicesLoading}
      error={deviceError}
      updatedAt={devicesUpdatedAt}
      onRefresh={() => void refreshDevices()}
      title="현재 연결된 키보드"
      description="키 확인 페이지에서도 현재 연결된 장치를 바로 보이게 유지합니다. 다만 브라우저 KeyboardEvent만으로는 방금 눌린 키가 이 목록 중 어느 키보드에서 왔는지 확정할 수 없습니다."
      emptyDescription={keyforgeBridge.isNative() ? '연결된 키보드를 찾지 못했습니다.' : '브라우저 데모에서는 실제 Windows 장치를 열거하지 않습니다.'}
      compact
    />
    <div
      ref={captureRef}
      className={`key-inspector-target ${pressedKeys.length ? 'is-pressed' : ''}`}
      role="application"
      aria-label="키 입력 검사 영역"
      tabIndex={0}
      onKeyDown={(event) => inspect(event, 'key-down')}
      onKeyUp={(event) => inspect(event, 'key-up')}
      onBlur={() => {
        pressed.current.clear();
        setPressedKeys([]);
      }}
    >
      <KeyboardIcon size={34} />
      <span>{pressedKeys.length ? '현재 눌린 키' : '여기를 클릭하고 확인할 키를 누르세요'}</span>
      <strong>{pressedKeys.length ? pressedKeys.join(' + ') : '대기 중'}</strong>
      <small>좌·우 보조키, 숫자패드, 기능키와 미디어 키를 가능한 범위에서 구분합니다.</small>
    </div>

    {latest ? <section className="key-detail-card" aria-label="최근 키 상세 정보">
      <div className="key-detail-card__head"><div><span className="page-eyebrow">LATEST EVENT</span><h2>{latest.mappedKey}</h2></div><Badge tone={latest.phase === 'key-down' ? 'accent' : 'neutral'}>{latest.phase}</Badge></div>
      <dl className="key-detail-grid">
        <div><dt>KeyForge 키</dt><dd>{latest.mappedKey}</dd></div>
        <div><dt>KeyboardEvent.code</dt><dd>{latest.code}</dd></div>
        <div><dt>KeyboardEvent.key</dt><dd>{latest.key}</dd></div>
        <div><dt>keyCode / VK 참고값</dt><dd>{latest.keyCode} / {hex(latest.keyCode)}</dd></div>
        <div><dt>위치</dt><dd>{locationLabel(latest.location)} ({latest.location})</dd></div>
        <div><dt>Modifier</dt><dd>{latest.modifiers.join(' + ') || '없음'}</dd></div>
        <div><dt>반복 / 조합 중</dt><dd>{latest.repeat ? 'repeat' : '첫 입력'} / {latest.composing ? 'IME 조합 중' : '아님'}</dd></div>
        <div><dt>이벤트 신뢰 / 시각</dt><dd>{latest.trusted ? 'OS 입력' : '합성·테스트'} / {latest.observedAt}</dd></div>
        <div className="key-detail-grid__wide"><dt>Windows Scan Code</dt><dd>브라우저 API에서 직접 제공하지 않음 · 정확한 값은 향후 네이티브 검사 모드에서 제공</dd></div>
      </dl>
    </section> : <EmptyState icon={<KeyboardIcon size={26} />} title="아직 감지된 키가 없습니다" description="위 검사 영역에 포커스를 두고 키를 누르면 상세 코드가 나타납니다." />}

    <section className="key-history-card">
      <div className="panel-card__head"><div><h2>최근 이벤트</h2><p>현재 탭의 최근 20건만 메모리에 유지합니다.</p></div><Badge>{events.length}/20</Badge></div>
      {events.length ? <div className="key-history-list">{events.map((item) => <div key={item.id} className="key-history-row"><Badge tone={item.phase === 'key-down' ? 'accent' : 'neutral'}>{item.phase === 'key-down' ? 'DOWN' : 'UP'}</Badge><strong>{item.mappedKey}</strong><code>{item.code}</code><span>{item.keyCode} / {hex(item.keyCode)}</span><time>{item.observedAt}</time></div>)}</div> : <div className="key-history-empty"><Info size={17} />키를 누르면 down/up 이벤트가 여기에 표시됩니다.</div>}
    </section>
    <Callout tone="warning" title="Windows 예약 키와 코드 표시에 한계가 있습니다.">
      `Ctrl+Alt+Delete`처럼 운영체제가 가로채는 조합은 앱에 전달되지 않습니다. `keyCode`는 호환용 참고값이며 정확한 물리 스캔 코드는 아닙니다.
    </Callout>
  </div>;
}

export function DevicesPage() {
  const {
    devices,
    loading,
    error,
    updatedAt,
    refresh,
  } = useConnectedKeyboards();

  const pnpCount = devices.filter((device) => device.instanceId || device.containerId).length;
  const containerCount = new Set(devices.flatMap((device) => device.containerId ? [device.containerId] : [])).size;
  return <div className="page devices-page">
    <PageHeader
      title="장치"
      description="Windows Raw Input이 현재 연결된 것으로 보고하는 실제 키보드를 확인합니다."
      actions={<Button icon={<RefreshCcw className={loading ? 'spin' : ''} size={16} />} disabled={loading} onClick={() => void refresh()}>{loading ? '확인 중…' : '새로 고침'}</Button>}
    />
    <Callout title="프로필 활성화용 selector 후보를 함께 보여줍니다.">
      정적 예제 데이터는 사용하지 않습니다. 한 물리 키보드가 여러 HID collection으로 노출되면 여러 항목으로 표시될 수 있습니다. 장치별 입력 출처 리맵은 여전히 비활성이지만, 아래 selector 후보는 연결 상태 기반 프로필 자동 전환에 사용할 수 있습니다.
    </Callout>
    <ConnectedKeyboardSpotlight
      devices={devices}
      loading={loading}
      error={error}
      updatedAt={updatedAt}
      title="지금 연결된 키보드"
      description="먼저 현재 살아있는 키보드를 눈에 띄게 보여주고, 아래 상세 카드에서 세부 식별 정보를 이어서 확인합니다."
      emptyDescription={keyforgeBridge.isNative() ? '장치를 다시 연결한 뒤 새로 고침하세요.' : '브라우저 데모에서는 실제 Windows 장치를 열거하지 않습니다.'}
    />
    <div className="device-summary-grid">
      <MetricCard icon={<Keyboard size={20} />} label="Raw Input 항목" value={devices.length} detail={updatedAt ? `${updatedAt.toLocaleTimeString('ko-KR')} 확인` : '확인 대기'} />
      <MetricCard icon={<Usb size={20} />} label="PnP 정보 결합" value={pnpCount} detail="표시 이름과 Windows 식별 정보" tone="green" />
      <MetricCard icon={<Radio size={20} />} label="Container 그룹" value={containerCount} detail="물리 장치 묶음 참고값" tone="purple" />
    </div>

    {devices.length ? <div className="real-device-list">{devices.map((device) => {
      const manufacturer = meaningfulManufacturer(device.manufacturer);
      const connectionLabel = humanConnectionLabel(device);
      const locationLabel = humanLocationLabel(device);
      const groupLabel = containerGroupLabel(device, devices);
      return <article className="real-device-card" key={device.id}>
        <header><span className="real-device-card__icon"><Keyboard size={21} /></span><div><h2>{device.name}</h2><p className="real-device-card__manufacturer">{manufacturer ?? connectionLabel}</p><code>{device.id}</code></div><Badge tone={device.isVirtual ? 'purple' : 'success'}>{device.isVirtual ? '가상·원격 경로' : '일반 장치 경로'}</Badge></header>
        <div className="device-id-chips">
          <span>연결 방식 <strong>{connectionLabel}</strong></span>
          <span>컨테이너 <strong>{groupLabel}</strong></span>
          <span>VID/PID <strong>{device.vendorId && device.productId ? `${device.vendorId}/${device.productId}` : '없음'}</strong></span>
          <span>PnP <strong>{device.instanceId || device.containerId ? '결합됨' : '부분 정보'}</strong></span>
        </div>
        <div className="setting-row" style={{ marginTop: 12 }}><div><strong>연결 조건 selector</strong><p>{selectorLabel(makeDeviceSelector(device))}</p></div></div>
        <dl className="device-metadata-grid">
          <div><dt>사람이 읽는 구분 힌트</dt><dd>{connectionLabel}</dd></div>
          <div><dt>위치 힌트</dt><dd>{locationLabel ?? '보고 안 됨'}</dd></div>
          <div><dt>총 키 수</dt><dd>{device.totalKeyCount || '보고 안 됨'}</dd></div>
          <div><dt>기능키 수</dt><dd>{device.functionKeyCount || '보고 안 됨'}</dd></div>
          <div><dt>Indicator 수</dt><dd>{device.indicatorCount || '보고 안 됨'}</dd></div>
          <div><dt>Type / Subtype</dt><dd>{device.keyboardType} / {device.keyboardSubType}</dd></div>
        </dl>
        <details><summary>고급 식별 정보 보기</summary><dl className="device-detail-list">
          <div><dt>Raw Input 인터페이스 경로</dt><dd><code>{device.devicePath}</code></dd></div>
          <div><dt>Device Instance ID</dt><dd><code>{device.instanceId ?? '보고 안 됨'}</code></dd></div>
          <div><dt>Container ID</dt><dd><code>{device.containerId ?? '보고 안 됨'}</code></dd></div>
          <div><dt>Hardware IDs</dt><dd><code>{device.hardwareIds.length ? device.hardwareIds.join('\n') : '보고 안 됨'}</code></dd></div>
          <div><dt>Location Paths</dt><dd><code>{device.locationPaths.length ? device.locationPaths.join('\n') : '보고 안 됨'}</code></dd></div>
          <div><dt>Keyboard mode</dt><dd>{device.keyboardMode}</dd></div>
          <div><dt>MI(경로)</dt><dd>{device.interfaceId ?? '없음'}</dd></div>
        </dl></details>
      </article>;
    })}</div> : null}
    <Callout tone="warning" title="동일 모델 여러 대는 아직 구분하지 못합니다.">
      devicePath나 현재 inventory ID는 저장하지 않습니다. 같은 VID/PID와 이름을 가진 키보드가 여러 대 연결되면 모두 같은 selector에 매칭됩니다.
    </Callout>
  </div>;
}

type ActivityFilter = 'all' | 'success' | 'warning' | 'error';

export function ActivityPage({ activity }: { activity: ActionResult[] }) {
  const [filter, setFilter] = useState<ActivityFilter>('all');
  const [query, setQuery] = useState('');
  const [expanded, setExpanded] = useState<string | null>(null);
  const filtered = activity.filter((item) => (filter === 'all' || item.status === filter) && item.message.toLowerCase().includes(query.toLowerCase()));
  return <div className="page">
    <PageHeader title="활동 기록" description="저장, 적용, 복구와 엔진 동작 결과를 한곳에서 확인합니다." actions={<Button icon={<Download size={16} />}>진단 로그 내보내기</Button>} />
    <Callout title="개인정보 보호">실제 키 입력 텍스트와 클립보드 내용은 활동 기록에 저장하지 않습니다.</Callout>
    <div className="toolbar-card"><label className="search-field"><Search size={17} /><input aria-label="활동 검색" placeholder="메시지 또는 action ID 검색" value={query} onChange={(event) => setQuery(event.target.value)} /></label><div className="filter-tabs" role="group" aria-label="활동 상태 필터">{([['all', '전체'], ['success', '성공'], ['warning', '경고'], ['error', '오류']] as const).map(([id, label]) => <button type="button" key={id} className={filter === id ? 'is-active' : ''} onClick={() => setFilter(id)}>{label}{id !== 'all' && <span className="filter-count">{activity.filter((item) => item.status === id).length}</span>}</button>)}</div></div>
    {filtered.length ? <div className="activity-table"><div className="activity-table__header"><span>상태</span><span>메시지</span><span>작업</span><span>Revision</span><span>시간</span><span /></div>{filtered.map((item) => <div key={item.actionId} className={`activity-entry ${expanded === item.actionId ? 'is-expanded' : ''}`}><button type="button" className="activity-entry__row" onClick={() => setExpanded(expanded === item.actionId ? null : item.actionId)}><span className={`activity-status activity-status--${item.status}`}><StatusIcon status={item.status} size={16} /><span>{item.status === 'success' ? '성공' : item.status === 'warning' ? '경고' : '오류'}</span></span><strong>{item.message}</strong><code>{item.actionType}</code><span>{item.revision ?? '—'}</span><time>{new Date(item.timestamp).toLocaleString('ko-KR')}</time><ChevronRight size={16} /></button>{expanded === item.actionId && <div className="activity-entry__details"><div><span>Action ID</span><code>{item.actionId}</code></div><div><span>복구 결과</span><strong>{item.recovery?.message ?? (item.recovery?.attempted ? '복구 작업을 수행했습니다.' : '복구 작업 없음')}</strong></div><div><span>세부 단계</span><code>{String(item.details?.stage ?? item.details?.source ?? '완료')}</code></div></div>}</div>)}</div> : <EmptyState icon={<Activity size={25} />} title="표시할 활동이 없습니다" description="검색어나 필터 조건을 변경해 보세요." />}
  </div>;
}

function SettingsSection({ icon, title, description, children }: { icon: ReactNode; title: string; description: string; children: ReactNode }) {
  return <section className="settings-section"><header><span>{icon}</span><div><h2>{title}</h2><p>{description}</p></div></header><div className="settings-list">{children}</div></section>;
}

export function SettingsPage({
  settings,
  settingsPath,
  runtime,
  onPreferences,
  onEngine,
  onBackup,
  onRestore,
  busy,
  native,
}: {
  settings: Settings;
  settingsPath: string;
  runtime: RuntimeState;
  onPreferences: (preferences: AppPreferences) => void;
  onEngine: (engine: EngineSettings) => void;
  onBackup: () => void;
  onRestore: () => void;
  busy: boolean;
  native: boolean;
}) {
  const themeOptions: Array<{ id: ThemePreference; label: string; icon: ReactNode }> = [{ id: 'system', label: '시스템', icon: <Monitor size={16} /> }, { id: 'light', label: '밝게', icon: <Sun size={16} /> }, { id: 'dark', label: '어둡게', icon: <Moon size={16} /> }];
  return <div className="page">
    <PageHeader title="설정" description="앱 동작, 입력 엔진의 안전 제한, 백업과 진단을 관리합니다." />
    <div className="settings-layout">
      <SettingsSection icon={<Settings2 size={20} />} title="일반" description="앱의 모양과 시작 동작을 선택합니다.">
        <div className="setting-row"><div><strong>테마</strong><p>Windows 시스템 테마를 따르거나 직접 지정합니다.</p></div><div className="theme-picker">{themeOptions.map((option) => <button key={option.id} type="button" disabled={busy} className={settings.preferences.theme === option.id ? 'is-selected' : ''} onClick={() => onPreferences({ ...settings.preferences, theme: option.id })}>{option.icon}{option.label}</button>)}</div></div>
        <div className="setting-row"><div><strong>Windows 로그인 시 자동 시작</strong><p>현재 사용자 계정의 시작 프로그램에 KeyForge를 등록합니다. 관리자 권한은 필요하지 않으며, Windows 시작 앱 설정에서 별도로 끌 수 있습니다. 포터블 EXE 위치를 옮겼다면 이 설정을 껐다가 다시 켜세요.</p></div><Toggle checked={settings.preferences.launchAtLogin} onChange={(launchAtLogin) => onPreferences({ ...settings.preferences, launchAtLogin })} label="Windows 로그인 시 자동 시작" disabled={busy} /></div>
        <div className="setting-row"><div><strong>시작할 때 최소화</strong><p>알림 영역에서 조용히 입력 엔진을 시작합니다.</p></div><Toggle checked={settings.preferences.startMinimized} onChange={(startMinimized) => onPreferences({ ...settings.preferences, startMinimized })} label="시작할 때 최소화" disabled={busy} /></div>
        <div className="setting-row"><div><strong>창을 닫으면 알림 영역으로</strong><p>켜면 X는 창만 숨기고 키 매핑을 계속 실행합니다. 끄면 X가 KeyForge와 키 매핑을 완전히 종료합니다.</p></div><Toggle checked={settings.preferences.closeToTray} onChange={(closeToTray) => onPreferences({ ...settings.preferences, closeToTray })} label="알림 영역에서 계속 실행" disabled={busy} /></div>
        <div className="setting-row"><div><strong>알림 수준</strong><p>저장과 실행 결과를 어느 수준까지 표시할지 선택합니다.</p></div><select disabled={busy} value={settings.preferences.notifications} onChange={(event) => onPreferences({ ...settings.preferences, notifications: event.target.value as AppPreferences['notifications'] })}><option value="all">모든 결과</option><option value="warnings">경고와 오류</option><option value="errors">오류만</option></select></div>
      </SettingsSection>

      <SettingsSection icon={<Gauge size={20} />} title="입력 엔진" description="입력 반복과 주입 재처리를 안전하게 제한합니다.">
        <div className="setting-row"><div><strong>비상 정지 단축키</strong><p>어떤 규칙보다 높은 우선순위로 모든 입력을 중단합니다.</p></div><span className="shortcut-chip shortcut-chip--large">{settings.engine.emergencyStop.join(' + ')}</span></div>
        <div className="setting-row"><div><strong>주입된 입력 모두 무시</strong><p>KeyForge가 만든 입력을 다시 규칙으로 처리하지 않습니다.</p></div><Toggle checked={settings.engine.ignoreAllInjectedInput} onChange={(ignoreAllInjectedInput) => onEngine({ ...settings.engine, ignoreAllInjectedInput })} label="주입된 입력 모두 무시" disabled={busy} /></div>
        <div className="setting-row"><div><strong>규칙 최대 실행 횟수</strong><p>규칙 하나가 실행할 수 있는 최대 반복 횟수입니다.</p></div><input className="short-number" aria-label="규칙 최대 실행 횟수" type="number" min={1} disabled={busy} value={settings.engine.maxRuleExecutions} onChange={(event) => onEngine({ ...settings.engine, maxRuleExecutions: Number(event.target.value) })} /></div>
      </SettingsSection>

      <SettingsSection icon={<HardDriveDownload size={20} />} title="데이터 및 백업" description={`설정 revision ${settings.revision} · ${new Date(settings.updatedAt).toLocaleString('ko-KR')}`}>
        <div className="setting-row setting-row--stack"><div><strong>설정 파일</strong><p className="path-text">{settingsPath}</p></div><Button size="small" icon={<FolderOpen size={15} />} disabled={!native}>폴더 열기 {!native && <Badge>준비 중</Badge>}</Button></div>
        <div className="setting-row"><div><strong>백업 만들기</strong><p>현재 검증된 설정을 별도 백업으로 보관합니다.</p></div><Button icon={<HardDriveDownload size={16} />} disabled={busy} onClick={onBackup}>{busy ? '처리 중…' : '지금 백업'}</Button></div>
        <div className="setting-row"><div><strong>최근 백업 복원</strong><p>복원 전 revision을 확인하고 새로운 revision으로 안전하게 적용합니다.</p></div><Button icon={<RotateCcw size={16} />} disabled={busy} onClick={onRestore}>백업 복원</Button></div>
        <div className="setting-row"><div><strong>가져오기 · 내보내기</strong><p>민감 정보가 없는 설정 파일을 다른 PC로 옮깁니다.</p></div><div className="header-button-row"><Button size="small" icon={<FileInput size={15} />}>가져오기</Button><Button size="small" icon={<Download size={15} />}>내보내기</Button></div></div>
      </SettingsSection>

      <SettingsSection icon={<ShieldCheck size={20} />} title="진단" description="입력 엔진과 설정 저장 상태를 확인합니다.">
        <div className="diagnostic-grid"><div><span>엔진 상태</span><strong className={runtime.engineState === 'running' ? 'success-text' : ''}><span className={`status-dot ${runtime.engineState === 'running' ? 'is-running' : ''}`} /> {runtime.engineState === 'running' ? '정상 실행 중' : runtime.engineState === 'paused' ? '일시정지' : '오류'}</strong></div><div><span>입력 후크</span><strong>{runtime.hookInstalled ? '설치됨' : '확인 필요'}</strong></div><div><span>설정 스키마</span><strong>v{settings.schemaVersion}</strong></div><div><span>실행 환경</span><strong>{native ? 'Tauri / Windows' : 'Browser mock'}</strong></div></div>
        <div className="setting-row"><div><strong>진단 번들</strong><p>개인 키 입력 없이 시스템 상태와 오류 로그만 수집합니다.</p></div><Button icon={<Download size={16} />}>진단 번들 생성</Button></div>
      </SettingsSection>
    </div>
  </div>;
}
