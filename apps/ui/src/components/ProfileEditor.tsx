import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowDown,
  ArrowRight,
  ArrowUp,
  Check,
  CircleDot,
  Copy,
  Globe2,
  GripVertical,
  Keyboard,
  MonitorCog,
  Plus,
  Save,
  ShieldCheck,
  Trash2,
  WandSparkles,
} from 'lucide-react';
import { makeId, makeRule } from '../data';
import { KEY_OPTION_GROUPS, keyboardEventToKey, orderChord, parseChordText } from '../keyCatalog';
import { keyforgeBridge, normalizeBridgeError, type KeyCaptureEvent } from '../lib/bridge';
import type { Profile, ProfileScope, Rule, RuleAction, RuleTrigger, ScopeCondition } from '../types';
import { Badge, Button, Callout, IconButton, Modal, Toggle } from './common';

type EditorTab = 'rules' | 'scope' | 'execution' | 'history';
type CaptureDialog = { purpose: 'input' | 'output'; sessionId: number };

const triggerLabel = (trigger: RuleTrigger) =>
  trigger.kind === 'keyboard' ? trigger.chord.join(' + ') : `${trigger.button} 마우스 버튼`;

export const actionLabel = (action: RuleAction) => {
  switch (action.kind) {
    case 'send_keys':
      return action.chord.join(' + ');
    case 'send_mouse':
      return `${action.button} 클릭`;
  }
};

const scopeLabel: Record<ProfileScope['kind'], string> = {
  global: '모든 앱과 장치에서 동작',
  application: '특정 앱에서만 동작',
  device: '특정 장치에서만 동작',
  combined: '앱과 장치 조건 결합',
};

function scopeForKind(kind: ProfileScope['kind']): ProfileScope {
  if (kind === 'global') return { kind: 'global' };
  const condition: ScopeCondition =
    kind === 'device'
      ? { kind: 'device_id', operator: 'equals', value: '' }
      : { kind: 'process_name', operator: 'equals', value: '' };
  return { kind, conditions: { operator: 'and', conditions: [condition] } };
}

const CHORD_SELECT_SLOTS = 3;

function chordWithSelectedKey(chord: string[], slot: number, key: string): string[] {
  const next = chord.slice(0, CHORD_SELECT_SLOTS);
  next[slot] = key;
  return orderChord(next.filter(Boolean));
}

function KeyCapture({
  open,
  onClose,
  onUse,
  sessionId,
  onEndNativeCapture,
  purpose = 'input',
}: {
  open: boolean;
  onClose: () => void;
  onUse: (chord: string[]) => void;
  sessionId: number | null;
  onEndNativeCapture: (sessionId: number) => Promise<void>;
  purpose?: 'input' | 'output';
}) {
  const [captured, setCaptured] = useState<string[]>([]);
  const [nativeWarning, setNativeWarning] = useState<string | null>(null);
  const isOutput = purpose === 'output';
  const captureTargetRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    if (!window.navigator.userAgent.includes('jsdom')) {
      window.focus();
    }
    captureTargetRef.current?.focus();
  }, [open]);
  useEffect(() => {
    if (!open) return;
    setCaptured([]);
    setNativeWarning(null);
    const pressed = new Set<string>();
    let nativeAvailable = keyforgeBridge.isNative();
    const record = (key: string, phase: KeyCaptureEvent['phase']) => {
      if (phase === 'up') {
        pressed.delete(key);
        return;
      }
      if (pressed.has(key)) return;
      pressed.add(key);
      setCaptured(orderChord(pressed));
    };

    // Keep the modal itself focusable so the first physical key lands in the
    // WebView immediately. The native queue still covers reserved chords such as
    // Alt+Space when Windows routes them outside the DOM.
    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      if (event.repeat) return;
      const key = keyboardEventToKey(event);
      if (key) record(key, 'down');
    };
    const handleKeyUp = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      const key = keyboardEventToKey(event);
      if (key) record(key, 'up');
    };
    window.addEventListener('keydown', handleKeyDown, { capture: true });
    window.addEventListener('keyup', handleKeyUp, { capture: true });
    const removeDomCapture = () => {
      window.removeEventListener('keydown', handleKeyDown, { capture: true });
      window.removeEventListener('keyup', handleKeyUp, { capture: true });
    };
    let disposed = false;
    let fallbackPending = false;
    const fallBackToDom = (message: string) => {
      if (disposed || !nativeAvailable || fallbackPending) return;
      pressed.clear();
      setCaptured([]);
      setNativeWarning(message);
      if (sessionId === null) {
        nativeAvailable = false;
        return;
      }
      fallbackPending = true;
      void onEndNativeCapture(sessionId)
        .then(() => {
          if (!disposed) nativeAvailable = false;
        })
        .catch(() => {
          if (!disposed) setNativeWarning(`${message} 네이티브 캡처를 종료하지 못해 일반 키 입력으로 전환하지 않았습니다.`);
        });
    };

    if (nativeAvailable) {
      if (sessionId === null) {
        fallBackToDom('네이티브 캡처 세션을 시작하지 못했습니다. 일반 키 입력으로 계속 시도할 수 있습니다.');
        return removeDomCapture;
      }

      let polling = false;
      const poll = () => {
        if (disposed || !nativeAvailable || fallbackPending || polling) return;
        polling = true;
        void keyforgeBridge.drainKeyCaptureEvents(sessionId)
          .then((drain) => {
            if (disposed || !nativeAvailable || fallbackPending) return;
            if (!drain.active || drain.sessionId !== sessionId) {
              fallBackToDom('네이티브 캡처가 중단되어 일반 키 입력 모드로 전환했습니다.');
              return;
            }
            if (drain.overflowed) {
              fallBackToDom('네이티브 입력이 너무 빠르게 들어와 일반 키 입력 모드로 전환했습니다.');
              return;
            }
            drain.events.forEach((event) => record(event.key, event.phase));
          })
          .catch(() => {
            fallBackToDom('네이티브 키 캡처 연결이 끊어져 일반 키 입력 모드로 전환했습니다.');
          })
          .finally(() => {
            polling = false;
          });
      };

      poll();
      const timer = window.setInterval(poll, 24);
      return () => {
        disposed = true;
        window.clearInterval(timer);
        removeDomCapture();
      };
    }
    return removeDomCapture;
  }, [open, sessionId, onEndNativeCapture]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={isOutput ? '전송 키 선택' : '입력 키 선택'}
      description={`${isOutput
        ? '실행 동작으로 전송할 키 또는 조합을 눌러주세요.'
        : '키보드의 키 또는 조합을 눌러주세요.'} 캡처 중에는 KeyForge 창 메뉴와 웹 단축키가 실행되지 않습니다.`}
      size="small"
    >
      <div className="capture-panel">
        <div
          ref={captureTargetRef}
          className={`capture-target ${captured.length ? 'has-value' : ''}`}
          tabIndex={-1}
        >
          <Keyboard size={28} />
          <span>{captured.length ? (isOutput ? '감지된 출력' : '감지된 입력') : '지금 키를 눌러보세요'}</span>
          <strong>{captured.length ? captured.join(' + ') : '대기 중…'}</strong>
        </div>
        <div className="key-metadata">
          <span>좌·우 보조키 · 숫자패드 · 미디어 키 구분</span>
          <span>주입 입력 자동 무시</span>
        </div>
        {nativeWarning && (
          <Callout tone="warning" title="네이티브 캡처를 다시 연결할 수 없습니다.">
            {nativeWarning}
          </Callout>
        )}
        {captured.includes('Escape') && (
          <Callout title="Escape가 입력으로 선택되었습니다.">
            이 창을 닫으려면 아래 취소 버튼을 사용하세요.
          </Callout>
        )}
        <div className="modal-actions">
          <Button onClick={onClose}>취소</Button>
          <Button variant="primary" disabled={!captured.length} onClick={() => onUse(captured)} icon={<Check size={17} />}>
            {isOutput ? '이 출력 사용' : '이 입력 사용'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

function KeySelect({
  label,
  value,
  onChange,
}: {
  label: string;
  value?: string;
  onChange: (value: string) => void;
}) {
  const listed = KEY_OPTION_GROUPS.some((group) => group.options.some((option) => option.value === value));
  return (
    <select aria-label={label} className="key-select" value={listed ? value : ''} onChange={(event) => onChange(event.target.value)}>
      <option value="">목록에서 키 선택…</option>
      {KEY_OPTION_GROUPS.map((group) => (
        <optgroup key={group.label} label={group.label}>
          {group.options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        </optgroup>
      ))}
    </select>
  );
}

function ChordSelectStrip({
  baseLabel,
  chord,
  onChange,
}: {
  baseLabel: string;
  chord: string[];
  onChange: (chord: string[]) => void;
}) {
  return (
    <div className="field-grid field-grid--3">
      {Array.from({ length: CHORD_SELECT_SLOTS }, (_, slot) => (
        <KeySelect
          key={`${baseLabel}-${slot}`}
          label={slot === 0 ? baseLabel : `${baseLabel} ${slot + 1}`}
          value={chord[slot] ?? ''}
          onChange={(value) => onChange(chordWithSelectedKey(chord, slot, value))}
        />
      ))}
    </div>
  );
}

function RuleComposer({
  rule,
  open,
  onClose,
  onCommit,
}: {
  rule: Rule | null;
  open: boolean;
  onClose: () => void;
  onCommit: (rule: Rule) => void;
}) {
  const [draft, setDraft] = useState<Rule | null>(rule);
  const [capture, setCapture] = useState<CaptureDialog | null>(null);
  const [captureStarting, setCaptureStarting] = useState(false);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const captureSessionRef = useRef<number | null>(null);
  const captureRequestRef = useRef(0);
  const captureTeardownRef = useRef<Promise<void> | null>(null);
  const endOwnedCapture = useCallback((sessionId: number | null) => {
    if (sessionId === null || captureSessionRef.current !== sessionId) return Promise.resolve();
    captureSessionRef.current = null;
    let teardown: Promise<void>;
    teardown = keyforgeBridge.endKeyCapture(sessionId).finally(() => {
      if (captureTeardownRef.current === teardown) captureTeardownRef.current = null;
    });
    captureTeardownRef.current = teardown;
    return teardown;
  }, []);

  useEffect(() => setDraft(rule ? structuredClone(rule) : null), [rule]);
  useEffect(() => () => {
    captureRequestRef.current += 1;
    void endOwnedCapture(captureSessionRef.current);
  }, [endOwnedCapture]);

  useEffect(() => {
    if (open) return;
    captureRequestRef.current += 1;
    void endOwnedCapture(captureSessionRef.current);
    setCapture(null);
    setCaptureError(null);
  }, [endOwnedCapture, open]);
  if (!draft) return null;

  const openKeyCapture = (purpose: 'input' | 'output') => {
    if (captureStarting || capture) return;
    setCaptureStarting(true);
    setCaptureError(null);
    const requestId = captureRequestRef.current + 1;
    captureRequestRef.current = requestId;
    let awaitingTeardown = false;
    void (async () => {
      while (captureTeardownRef.current) {
        awaitingTeardown = true;
        await captureTeardownRef.current;
      }
      awaitingTeardown = false;
      if (captureRequestRef.current !== requestId || !open) return null;
      return keyforgeBridge.beginKeyCapture();
    })()
      .then((session) => {
        if (!session) return;
        if (captureRequestRef.current !== requestId || !open) {
          void keyforgeBridge.endKeyCapture(session.sessionId);
          return;
        }
        captureSessionRef.current = session.sessionId;
        setCapture({ purpose, sessionId: session.sessionId });
      })
      .catch((error) => {
        const detail = normalizeBridgeError(error).message;
        setCaptureError(awaitingTeardown
          ? `이전 키 캡처를 종료하지 못했습니다. 다시 시도하세요. ${detail}`
          : `키 캡처 보호를 시작하지 못했습니다. ${detail}`);
      })
      .finally(() => setCaptureStarting(false));
  };

  const closeKeyCapture = () => {
    captureRequestRef.current += 1;
    void endOwnedCapture(captureSessionRef.current);
    setCapture(null);
  };

  const updateTriggerChord = (chord: string[]) => {
    setDraft((current) => current?.trigger.kind === 'keyboard'
      ? { ...current, trigger: { ...current.trigger, chord } }
      : current);
  };

  const updateActionChord = (chord: string[]) => {
    setDraft((current) => current?.action.kind === 'send_keys'
      ? { ...current, action: { ...current.action, chord } }
      : current);
  };

  const setActionKind = (kind: RuleAction['kind']) => {
    const actions: Record<RuleAction['kind'], RuleAction> = {
      send_keys: { kind: 'send_keys', chord: ['Escape'] },
      send_mouse: { kind: 'send_mouse', button: 'left' },
    };
    setDraft({ ...draft, action: actions[kind] });
  };

  return (
    <>
      <Modal open={open} onClose={onClose} title="규칙 편집" description="입력과 실행할 동작을 한 쌍으로 구성합니다." size="medium">
        <div className="rule-composer">
          {captureError && (
            <Callout tone="danger" title="키 캡처를 열 수 없습니다.">
              {captureError}
            </Callout>
          )}
          <section className="composer-card">
            <div className="composer-card__eyebrow">입력</div>
            <div className="composer-card__main composer-card__main--keys">
              <div className="keycap-large">{triggerLabel(draft.trigger)}</div>
              <div className="key-chord-controls">
                <input
                  aria-label="입력 키 조합 직접 입력"
                  value={draft.trigger.kind === 'keyboard' ? draft.trigger.chord.join(' + ') : ''}
                  onChange={(event) => updateTriggerChord(parseChordText(event.target.value))}
                />
                <Button aria-label="입력 키 직접 누르기" disabled={captureStarting} onClick={() => openKeyCapture('input')} icon={<Keyboard size={16} />}>키 직접 누르기</Button>
              </div>
            </div>
            {draft.trigger.kind === 'keyboard' && (
              <>
                <label className="field">
                  <span>입력 키 조합</span>
                  <ChordSelectStrip
                    baseLabel="입력 키 목록"
                    chord={draft.trigger.chord}
                    onChange={updateTriggerChord}
                  />
                </label>
                <div className="field-grid field-grid--3">
                  <label className="field">
                    <span>입력 시점</span>
                    <select
                      value={draft.trigger.phase}
                      onChange={(event) => {
                        const phase = event.target.value as 'press' | 'release';
                        setDraft((current) => current?.trigger.kind === 'keyboard'
                          ? { ...current, trigger: { ...current.trigger, phase } }
                          : current);
                      }}
                    >
                      <option value="press">누를 때</option>
                      <option value="release">뗄 때</option>
                    </select>
                  </label>
                  <label className="field">
                    <span>제스처</span>
                    <select
                      value={draft.trigger.gesture}
                      onChange={(event) => {
                        const gesture = event.target.value as 'single' | 'hold' | 'double';
                        setDraft((current) => current?.trigger.kind === 'keyboard'
                          ? { ...current, trigger: { ...current.trigger, gesture } }
                          : current);
                      }}
                    >
                      <option value="single">한 번 누르기</option>
                      <option value="hold">길게 누르기</option>
                      <option value="double">빠르게 두 번</option>
                    </select>
                  </label>
                </div>
              </>
            )}
          </section>

          <div className="composer-flow"><ArrowDown size={20} /></div>

          <section className="composer-card">
            <div className="composer-card__eyebrow">실행 동작</div>
            <label className="field">
              <span>동작 종류</span>
              <select value={draft.action.kind} onChange={(event) => setActionKind(event.target.value as RuleAction['kind'])}>
                <option value="send_keys">키 전송</option>
                <option value="send_mouse">마우스 클릭</option>
              </select>
            </label>

            {draft.action.kind === 'send_keys' && (
              <div className="field">
                <span>전송할 키 조합</span>
                <div className="key-output-controls">
                  <input aria-label="전송할 키 조합 직접 입력" value={draft.action.chord.join(' + ')} onChange={(event) => updateActionChord(parseChordText(event.target.value))} />
                  <Button aria-label="전송할 키 직접 누르기" disabled={captureStarting} onClick={() => openKeyCapture('output')} icon={<Keyboard size={16} />}>키 직접 누르기</Button>
                </div>
                <ChordSelectStrip baseLabel="전송할 키 목록" chord={draft.action.chord} onChange={updateActionChord} />
              </div>
            )}
            {draft.action.kind === 'send_mouse' && (
              <label className="field">
                <span>마우스 버튼</span>
                <select value={draft.action.button} onChange={(event) => setDraft({ ...draft, action: { kind: 'send_mouse', button: event.target.value as Extract<RuleAction, { kind: 'send_mouse' }>['button'] } })}>
                  <option value="left">왼쪽 버튼</option>
                  <option value="right">오른쪽 버튼</option>
                  <option value="middle">가운데 버튼</option>
                  <option value="x1">뒤로 버튼(X1)</option>
                  <option value="x2">앞으로 버튼(X2)</option>
                </select>
              </label>
            )}
          </section>

          <div className="settings-list settings-list--compact">
            <div className="setting-row">
              <div><strong>원래 입력도 함께 전달</strong><p>변환 동작과 함께 원래 키 입력을 앱에 전달합니다.</p></div>
              <Toggle checked={draft.options.passThroughOriginal} onChange={(value) => setDraft({ ...draft, options: { ...draft.options, passThroughOriginal: value } })} label="원래 입력 전달" />
            </div>
            <div className="setting-row">
              <div><strong>주입된 입력 무시</strong><p>규칙이 만든 입력을 다시 처리하지 않아 반복을 방지합니다.</p></div>
              <Toggle checked={draft.options.ignoreInjected} onChange={(value) => setDraft({ ...draft, options: { ...draft.options, ignoreInjected: value } })} label="주입 입력 무시" />
            </div>
          </div>

          <div className="modal-actions">
            <Button onClick={onClose}>취소</Button>
            <Button variant="primary" icon={<Check size={17} />} onClick={() => onCommit(draft)}>
              규칙 적용
            </Button>
          </div>
        </div>
      </Modal>
      <KeyCapture
        open={capture?.purpose === 'input'}
        onClose={closeKeyCapture}
        sessionId={capture?.sessionId ?? null}
        onEndNativeCapture={endOwnedCapture}
        onUse={(chord) => {
          setDraft({ ...draft, trigger: { kind: 'keyboard', chord, phase: 'press', gesture: 'single' } });
          closeKeyCapture();
        }}
      />
      <KeyCapture
        purpose="output"
        open={capture?.purpose === 'output'}
        onClose={closeKeyCapture}
        sessionId={capture?.sessionId ?? null}
        onEndNativeCapture={endOwnedCapture}
        onUse={(chord) => {
          setDraft((current) => current?.action.kind === 'send_keys'
            ? { ...current, action: { ...current.action, chord } }
            : current);
          closeKeyCapture();
        }}
      />
    </>
  );
}

export function ProfileEditor({
  open,
  profile,
  isNew,
  saving,
  onClose,
  onSave,
}: {
  open: boolean;
  profile: Profile | null;
  isNew: boolean;
  saving: boolean;
  onClose: () => void;
  onSave: (profile: Profile) => Promise<void>;
}) {
  const [draft, setDraft] = useState<Profile | null>(profile);
  const [tab, setTab] = useState<EditorTab>('rules');
  const [editingRule, setEditingRule] = useState<Rule | null>(null);

  useEffect(() => {
    setDraft(profile ? structuredClone(profile) : null);
    setTab(isNew ? 'scope' : 'rules');
  }, [isNew, profile]);

  const dirty = useMemo(() => Boolean(profile && draft && JSON.stringify(profile) !== JSON.stringify(draft)), [draft, profile]);

  useEffect(() => {
    if (!open || !draft) return;
    const handleSave = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        void onSave({ ...draft, updatedAt: new Date().toISOString() });
      }
    };
    window.addEventListener('keydown', handleSave);
    return () => window.removeEventListener('keydown', handleSave);
  }, [draft, onSave, open]);

  if (!draft) return null;

  const addRule = () => {
    const rule = { ...makeRule('Caps Lock', 'Escape'), order: draft.rules.length };
    setEditingRule(rule);
  };

  const commitRule = (rule: Rule) => {
    const exists = draft.rules.some((item) => item.id === rule.id);
    setDraft({
      ...draft,
      rules: exists ? draft.rules.map((item) => (item.id === rule.id ? rule : item)) : [...draft.rules, rule],
    });
    setEditingRule(null);
  };

  const moveRule = (index: number, direction: -1 | 1) => {
    const destination = index + direction;
    if (destination < 0 || destination >= draft.rules.length) return;
    const rules = [...draft.rules];
    [rules[index], rules[destination]] = [rules[destination], rules[index]];
    setDraft({ ...draft, rules: rules.map((rule, order) => ({ ...rule, order })) });
  };

  const setScope = (kind: ProfileScope['kind']) => setDraft({ ...draft, scope: scopeForKind(kind) });

  return (
    <>
      <Modal open={open} onClose={onClose} title={isNew ? '새 프로필' : draft.name} description={isNew ? '프로필은 기본적으로 모든 앱과 장치에서 동작합니다.' : '규칙과 적용 범위를 편집합니다.'} size="large">
        <div className="profile-editor">
          <div className="profile-editor__topbar">
            <label className="field profile-name-field">
              <span>프로필 이름</span>
              <input autoFocus={isNew} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} />
            </label>
            <div className="editor-state">
              <Toggle checked={draft.enabled} onChange={(enabled) => setDraft({ ...draft, enabled })} label="프로필 활성화" />
              <Badge tone={draft.scope.kind === 'global' ? 'accent' : 'purple'}>{draft.scope.kind === 'global' ? '전역' : '조건부'}</Badge>
              {dirty && <span className="unsaved-dot"><CircleDot size={14} /> 저장되지 않음</span>}
            </div>
          </div>

          <nav className="editor-tabs" aria-label="프로필 편집 섹션">
            {([
              ['rules', '규칙'],
              ['scope', '적용 조건'],
              ['execution', '실행 설정'],
              ['history', '기록'],
            ] as Array<[EditorTab, string]>).map(([id, label]) => (
              <button key={id} type="button" className={tab === id ? 'is-active' : ''} onClick={() => setTab(id)}>
                {label}{id === 'rules' && <span>{draft.rules.length}</span>}
              </button>
            ))}
          </nav>

          <div className="editor-content">
            {tab === 'rules' && (
              <section>
                <div className="section-heading">
                  <div><h3>입력 규칙</h3><p>위에 있는 규칙부터 먼저 평가합니다.</p></div>
                  <Button variant="primary" size="small" icon={<Plus size={16} />} onClick={addRule}>규칙 추가</Button>
                </div>
                {draft.rules.length ? (
                  <div className="rule-table" role="table" aria-label="입력 규칙">
                    <div className="rule-table__header" role="row"><span>입력</span><span>실행 동작</span><span>조건</span><span>상태</span><span /></div>
                    {draft.rules.map((rule, index) => (
                      <div className="rule-row" role="row" key={rule.id}>
                        <GripVertical size={16} className="drag-handle" aria-hidden />
                        <div className="rule-cell"><span className="keycap">{triggerLabel(rule.trigger)}</span></div>
                        <div className="rule-cell rule-cell--action"><ArrowRight size={15} /><strong>{actionLabel(rule.action)}</strong></div>
                        <div className="rule-cell"><Badge>항상</Badge></div>
                        <div className="rule-cell"><Toggle checked={rule.enabled} onChange={(enabled) => setDraft({ ...draft, rules: draft.rules.map((item) => item.id === rule.id ? { ...item, enabled } : item) })} label={`${triggerLabel(rule.trigger)} 규칙 활성화`} /></div>
                        <div className="rule-actions">
                          <IconButton label="위로 이동" disabled={index === 0} onClick={() => moveRule(index, -1)}><ArrowUp size={15} /></IconButton>
                          <IconButton label="아래로 이동" disabled={index === draft.rules.length - 1} onClick={() => moveRule(index, 1)}><ArrowDown size={15} /></IconButton>
                          <IconButton label="규칙 복제" onClick={() => setDraft({ ...draft, rules: [...draft.rules, { ...structuredClone(rule), id: makeId(), order: draft.rules.length }] })}><Copy size={15} /></IconButton>
                          <Button size="small" onClick={() => setEditingRule(rule)}>편집</Button>
                          <IconButton label="규칙 삭제" onClick={() => setDraft({ ...draft, rules: draft.rules.filter((item) => item.id !== rule.id) })}><Trash2 size={15} /></IconButton>
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <button className="add-rule-empty" type="button" onClick={addRule}>
                    <WandSparkles size={25} />
                    <strong>첫 번째 규칙을 추가하세요</strong>
                    <span>키 전송 또는 마우스 클릭 동작을 연결할 수 있습니다.</span>
                  </button>
                )}
              </section>
            )}

            {tab === 'scope' && (
              <section>
                {isNew && <Callout title="전역 범위가 기본값입니다.">앱이나 장치를 따로 선택하지 않아도 저장 즉시 모든 일반 입력에서 동작합니다.</Callout>}
                <Callout tone="warning" title="현재 버전은 전역 범위만 실행합니다.">
                  장치별 입력 출처를 정확히 구분하는 네이티브 계층이 준비될 때까지 앱·장치 조건은 선택할 수 없습니다. 이전 조건부 프로필은 설정을 보존한 채 전역 범위로 전환됩니다.
                </Callout>
                <div className="section-heading"><div><h3>적용 범위</h3><p>이 프로필을 어느 곳에서 활성화할지 선택합니다.</p></div></div>
                <div className="scope-options" role="radiogroup" aria-label="적용 범위">
                  {(['global', 'application', 'device', 'combined'] as ProfileScope['kind'][]).map((kind) => (
                    <button type="button" role="radio" aria-checked={draft.scope.kind === kind} disabled={kind !== 'global'} className={`scope-option ${draft.scope.kind === kind ? 'is-selected' : ''}`} key={kind} onClick={() => setScope(kind)}>
                      <span className="scope-option__radio">{draft.scope.kind === kind && <Check size={13} />}</span>
                      <span className="scope-option__icon">{kind === 'global' ? <Globe2 size={20} /> : kind === 'device' ? <Keyboard size={20} /> : <MonitorCog size={20} />}</span>
                      <span><strong>{scopeLabel[kind]}</strong><small>{kind === 'global' ? '권장 · 별도 지정 없이 즉시 적용' : '준비 중 · 현재 버전에서는 선택할 수 없음'}</small></span>
                    </button>
                  ))}
                </div>
                {draft.scope.kind !== 'global' && (
                  <div className="condition-builder">
                    <div className="condition-builder__head">
                      <strong>다음 조건을</strong>
                      <select value={draft.scope.conditions.operator} onChange={(event) => { const operator = event.target.value as 'and' | 'or'; setDraft((current) => current && current.scope.kind !== 'global' ? { ...current, scope: { ...current.scope, conditions: { ...current.scope.conditions, operator } } } : current); }}>
                        <option value="and">모두 만족</option><option value="or">하나 이상 만족</option>
                      </select>
                    </div>
                    {draft.scope.conditions.conditions.map((condition, index) => (
                      <div className="condition-row" key={`${condition.kind}-${index}`}>
                        <select value={condition.kind} onChange={(event) => {
                          const kind = event.target.value as ScopeCondition['kind'];
                          setDraft((current) => {
                            if (!current || current.scope.kind === 'global') return current;
                            const conditions = [...current.scope.conditions.conditions];
                            conditions[index] = { ...condition, kind };
                            return { ...current, scope: { ...current.scope, conditions: { ...current.scope.conditions, conditions } } };
                          });
                        }}><option value="process_name">프로세스 이름</option><option value="executable_path">실행 파일 경로</option><option value="window_class">창 클래스</option><option value="device_id">장치 ID</option></select>
                        <select value={condition.operator} onChange={(event) => {
                          const operator = event.target.value as 'equals' | 'contains';
                          setDraft((current) => {
                            if (!current || current.scope.kind === 'global') return current;
                            const conditions = [...current.scope.conditions.conditions];
                            conditions[index] = { ...condition, operator };
                            return { ...current, scope: { ...current.scope, conditions: { ...current.scope.conditions, conditions } } };
                          });
                        }}><option value="equals">같음</option><option value="contains">포함</option></select>
                        <input aria-label="조건 값" value={condition.value} placeholder={condition.kind === 'device_id' ? 'VID_…&PID_…' : '예: Code.exe'} onChange={(event) => {
                          const value = event.target.value;
                          setDraft((current) => {
                            if (!current || current.scope.kind === 'global') return current;
                            const conditions = [...current.scope.conditions.conditions];
                            conditions[index] = { ...condition, value };
                            return { ...current, scope: { ...current.scope, conditions: { ...current.scope.conditions, conditions } } };
                          });
                        }} />
                        <IconButton label="조건 삭제" onClick={() => {
                          if (draft.scope.kind === 'global') return;
                          const conditions = draft.scope.conditions.conditions.filter((_, itemIndex) => itemIndex !== index);
                          setDraft({ ...draft, scope: { ...draft.scope, conditions: { ...draft.scope.conditions, conditions } } });
                        }}><Trash2 size={16} /></IconButton>
                      </div>
                    ))}
                    <Button size="small" icon={<Plus size={15} />} onClick={() => {
                      if (draft.scope.kind === 'global') return;
                      const kind: ScopeCondition['kind'] = draft.scope.kind === 'device' ? 'device_id' : 'process_name';
                      setDraft({ ...draft, scope: { ...draft.scope, conditions: { ...draft.scope.conditions, conditions: [...draft.scope.conditions.conditions, { kind, operator: 'equals', value: '' }] } } });
                    }}>조건 추가</Button>
                  </div>
                )}
              </section>
            )}

            {tab === 'execution' && (
              <section>
                <div className="section-heading"><div><h3>실행 설정</h3><p>시작 동작과 안전 제한을 지정합니다.</p></div></div>
                <div className="settings-list">
                  <div className="setting-row"><div><strong>Windows 시작 시 이 프로필 활성화</strong><p>KeyForge가 시작되면 이 프로필을 함께 켭니다.</p></div><Toggle checked={draft.enableOnStartup} onChange={(enableOnStartup) => setDraft({ ...draft, enableOnStartup })} label="시작 시 활성화" /></div>
                  <div className="setting-row"><div><strong>입력 반복 방지</strong><p>주입된 입력을 표시하고 재처리를 차단합니다.</p></div><Badge tone="success"><ShieldCheck size={13} /> 항상 켜짐</Badge></div>
                </div>
                <Callout tone="warning" title="비상 정지 단축키">Ctrl + Alt + Pause를 누르면 이 프로필을 포함한 모든 입력 규칙이 즉시 중단됩니다.</Callout>
              </section>
            )}

            {tab === 'history' && (
              <section>
                <div className="section-heading"><div><h3>프로필 기록</h3><p>민감한 키 입력 내용은 기록하지 않습니다.</p></div></div>
                <div className="history-timeline">
                  <div><span className="timeline-dot is-success" /><strong>프로필 설정을 불러왔습니다.</strong><small>{draft.updatedAt ? new Date(draft.updatedAt).toLocaleString('ko-KR') : '기록 없음'}</small></div>
                  {draft.lastRunAt && <div><span className="timeline-dot" /><strong>마지막으로 규칙을 실행했습니다.</strong><small>{new Date(draft.lastRunAt).toLocaleString('ko-KR')}</small></div>}
                </div>
              </section>
            )}
          </div>

          <footer className="profile-editor__footer">
            <span>{dirty ? '저장하지 않은 변경사항이 있습니다.' : `마지막 저장 · ${new Date(draft.updatedAt).toLocaleString('ko-KR')}`}</span>
            <div><Button onClick={onClose}>취소</Button><Button variant="primary" disabled={saving || !draft.name.trim() || (draft.scope.kind !== 'global' && draft.scope.conditions.conditions.some((item) => !item.value.trim()))} icon={saving ? <CircleDot className="spin" size={17} /> : <Save size={17} />} onClick={() => void onSave({ ...draft, name: draft.name.trim(), updatedAt: new Date().toISOString() })}>{saving ? '저장 중…' : '저장 및 적용'}</Button></div>
          </footer>
        </div>
      </Modal>
      <RuleComposer rule={editingRule} open={Boolean(editingRule)} onClose={() => setEditingRule(null)} onCommit={commitRule} />
    </>
  );
}
