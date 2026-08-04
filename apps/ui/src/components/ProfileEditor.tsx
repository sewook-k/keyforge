import { useEffect, useMemo, useState } from 'react';
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
import { makeDeviceSelector, selectorLabel } from '../deviceSelectors';
import { makeId, makeRule } from '../data';
import { KEY_OPTION_GROUPS, keyboardEventToKey, orderChord } from '../keyCatalog';
import { keyforgeBridge } from '../lib/bridge';
import type {
  DeviceSelector,
  KeyboardDeviceInfo,
  Profile,
  ProfileScope,
  Rule,
  RuleAction,
  RuleTrigger,
  ScopeCondition,
} from '../types';
import { Badge, Button, Callout, IconButton, Modal, Toggle } from './common';

type EditorTab = 'rules' | 'scope' | 'execution' | 'history';

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

const hasConnectedKeyboardActivation = (profile: Profile) => profile.activation.connectedKeyboards.length > 0;

const sameSelector = (left: DeviceSelector, right: DeviceSelector) => JSON.stringify(left) === JSON.stringify(right);

function KeyCapture({
  open,
  onClose,
  onUse,
  purpose = 'input',
}: {
  open: boolean;
  onClose: () => void;
  onUse: (chord: string[]) => void;
  purpose?: 'input' | 'output';
}) {
  const [captured, setCaptured] = useState<string[]>([]);
  const isOutput = purpose === 'output';

  useEffect(() => {
    if (!open) return;
    setCaptured([]);
    const pressed = new Set<string>();
    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.repeat) return;
      const key = keyboardEventToKey(event);
      if (!key) return;
      pressed.add(key);
      setCaptured(orderChord(pressed));
    };
    const handleKeyUp = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      const key = keyboardEventToKey(event);
      if (key) pressed.delete(key);
    };
    window.addEventListener('keydown', handleKeyDown, { capture: true });
    window.addEventListener('keyup', handleKeyUp, { capture: true });
    return () => {
      window.removeEventListener('keydown', handleKeyDown, { capture: true });
      window.removeEventListener('keyup', handleKeyUp, { capture: true });
    };
  }, [open]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={isOutput ? '전송 키 선택' : '입력 키 선택'}
      description={isOutput
        ? '실행 동작으로 전송할 키 또는 조합을 눌러주세요.'
        : 'Windows 예약 조합을 제외한 키보드의 키 또는 조합을 눌러주세요.'}
      size="small"
    >
      <div className="capture-panel">
        <div className={`capture-target ${captured.length ? 'has-value' : ''}`}>
          <Keyboard size={28} />
          <span>{captured.length ? (isOutput ? '감지된 출력' : '감지된 입력') : '지금 키를 눌러보세요'}</span>
          <strong>{captured.length ? captured.join(' + ') : '대기 중…'}</strong>
        </div>
        <div className="key-metadata">
          <span>좌·우 보조키 · 숫자패드 · 미디어 키 구분</span>
          <span>주입 입력 자동 무시</span>
        </div>
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
  const [captureOpen, setCaptureOpen] = useState(false);
  const [outputCaptureOpen, setOutputCaptureOpen] = useState(false);

  useEffect(() => setDraft(rule ? structuredClone(rule) : null), [rule]);
  if (!draft) return null;

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
          <section className="composer-card">
            <div className="composer-card__eyebrow">입력</div>
            <div className="composer-card__main composer-card__main--keys">
              <div className="keycap-large">{triggerLabel(draft.trigger)}</div>
              <div className="key-input-controls">
                <Button aria-label="입력 키 직접 누르기" onClick={() => setCaptureOpen(true)} icon={<Keyboard size={16} />}>키 직접 누르기</Button>
                <KeySelect
                  label="입력 키 목록"
                  value={draft.trigger.kind === 'keyboard' && draft.trigger.chord.length === 1 ? draft.trigger.chord[0] : ''}
                  onChange={(key) => key && setDraft({ ...draft, trigger: { kind: 'keyboard', chord: [key], phase: 'press', gesture: 'single' } })}
                />
              </div>
            </div>
            {draft.trigger.kind === 'keyboard' && (
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
                  <input aria-label="전송할 키 조합 직접 입력" value={draft.action.chord.join(' + ')} onChange={(event) => setDraft({ ...draft, action: { kind: 'send_keys', chord: event.target.value.split('+').map((item) => item.trim()).filter(Boolean) } })} />
                  <Button aria-label="전송할 키 직접 누르기" onClick={() => setOutputCaptureOpen(true)} icon={<Keyboard size={16} />}>키 직접 누르기</Button>
                  <KeySelect label="전송할 키 목록" value={draft.action.chord.length === 1 ? draft.action.chord[0] : ''} onChange={(key) => key && setDraft({ ...draft, action: { kind: 'send_keys', chord: [key] } })} />
                </div>
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
        open={captureOpen}
        onClose={() => setCaptureOpen(false)}
        onUse={(chord) => {
          setDraft({ ...draft, trigger: { kind: 'keyboard', chord, phase: 'press', gesture: 'single' } });
          setCaptureOpen(false);
        }}
      />
      <KeyCapture
        purpose="output"
        open={outputCaptureOpen}
        onClose={() => setOutputCaptureOpen(false)}
        onUse={(chord) => {
          setDraft((current) => current?.action.kind === 'send_keys'
            ? { ...current, action: { ...current.action, chord } }
            : current);
          setOutputCaptureOpen(false);
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
  const [connectedKeyboards, setConnectedKeyboards] = useState<KeyboardDeviceInfo[]>([]);
  const [devicesLoading, setDevicesLoading] = useState(false);
  const [deviceError, setDeviceError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(profile ? structuredClone(profile) : null);
    setTab(isNew ? 'scope' : 'rules');
  }, [isNew, profile]);

  const dirty = useMemo(() => Boolean(profile && draft && JSON.stringify(profile) !== JSON.stringify(draft)), [draft, profile]);
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setDevicesLoading(true);
    setDeviceError(null);
    void keyforgeBridge.listConnectedKeyboards()
      .then((devices) => {
        if (cancelled) return;
        setConnectedKeyboards(devices);
      })
      .catch((error) => {
        if (cancelled) return;
        setConnectedKeyboards([]);
        setDeviceError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!cancelled) setDevicesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

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
  const addConnectedKeyboardSelector = (device: KeyboardDeviceInfo) => {
    const selector = makeDeviceSelector(device);
    if (draft.activation.connectedKeyboards.some((item) => sameSelector(item, selector))) return;
    setDraft({
      ...draft,
      activation: {
        ...draft.activation,
        connectedKeyboards: [...draft.activation.connectedKeyboards, selector],
      },
    });
  };

  const removeConnectedKeyboardSelector = (selector: DeviceSelector) => {
    setDraft({
      ...draft,
      activation: {
        ...draft.activation,
        connectedKeyboards: draft.activation.connectedKeyboards.filter((item) => !sameSelector(item, selector)),
      },
    });
  };

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
              <Badge tone={hasConnectedKeyboardActivation(draft) ? 'purple' : 'accent'}>{hasConnectedKeyboardActivation(draft) ? '연결 조건' : '전역'}</Badge>
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
                {isNew && <Callout title="전역 범위가 기본값입니다.">앱을 따로 지정하지 않아도 저장 즉시 모든 일반 입력에서 동작합니다.</Callout>}
                <Callout tone="warning" title="입력 발생 키보드별 리맵은 아직 지원하지 않습니다.">
                  장치별 입력 출처를 정확히 구분하는 네이티브 계층이 준비될 때까지 앱·장치 scope는 계속 비활성입니다. 이번 기능은 현재 연결된 키보드 집합을 보고 프로필을 자동 켜고 끄는 활성화 조건입니다.
                </Callout>
                <div className="section-heading"><div><h3>적용 범위</h3><p>프로필 규칙 자체는 계속 전역으로 실행하고, 연결 상태만 별도 조건으로 계산합니다.</p></div></div>
                <div className="scope-options" role="radiogroup" aria-label="적용 범위">
                  {(['global', 'application', 'device', 'combined'] as ProfileScope['kind'][]).map((kind) => (
                    <button type="button" role="radio" aria-checked={draft.scope.kind === kind} disabled={kind !== 'global'} className={`scope-option ${draft.scope.kind === kind ? 'is-selected' : ''}`} key={kind} onClick={() => setScope(kind)}>
                      <span className="scope-option__radio">{draft.scope.kind === kind && <Check size={13} />}</span>
                      <span className="scope-option__icon">{kind === 'global' ? <Globe2 size={20} /> : kind === 'device' ? <Keyboard size={20} /> : <MonitorCog size={20} />}</span>
                      <span><strong>{scopeLabel[kind]}</strong><small>{kind === 'global' ? '계속 지원 · 실제 입력 규칙은 전역으로 평가' : '준비 중 · 입력 출처별 리맵은 비활성'}</small></span>
                    </button>
                  ))}
                </div>
                <div className="section-heading"><div><h3>연결된 키보드 활성화</h3><p>선택한 selector 중 하나라도 연결되어 있으면 이 프로필을 자동으로 활성화합니다.</p></div></div>
                {draft.activation.connectedKeyboards.length ? (
                  <div className="condition-builder">
                    {draft.activation.connectedKeyboards.map((selector, index) => (
                      <div className="condition-row" key={`${selectorLabel(selector)}-${index}`}>
                        <span className="keycap" style={{ flex: 1 }}>{selectorLabel(selector)}</span>
                        <IconButton label="연결 조건 삭제" onClick={() => removeConnectedKeyboardSelector(selector)}><Trash2 size={16} /></IconButton>
                      </div>
                    ))}
                  </div>
                ) : (
                  <Callout title="항상 활성화됩니다.">지금은 어떤 키보드도 선택하지 않아, 연결 상태와 관계없이 기존 전역 프로필처럼 동작합니다.</Callout>
                )}
                {deviceError && <Callout tone="danger" title="현재 키보드 목록을 읽지 못했습니다.">{deviceError}</Callout>}
                {devicesLoading ? (
                  <Callout title="현재 연결된 키보드를 확인하는 중입니다.">Windows Raw Input 인벤토리에서 selector 후보를 읽고 있습니다.</Callout>
                ) : connectedKeyboards.length ? (
                  <div className="settings-list">
                    {connectedKeyboards.map((device) => {
                      const selector = makeDeviceSelector(device);
                      const alreadyAdded = draft.activation.connectedKeyboards.some((item) => sameSelector(item, selector));
                      return (
                        <div className="setting-row" key={device.id}>
                          <div>
                            <strong>{device.name}</strong>
                            <p>{selectorLabel(selector)}</p>
                          </div>
                          <Button size="small" disabled={alreadyAdded} onClick={() => addConnectedKeyboardSelector(device)}>
                            {alreadyAdded ? '추가됨' : '이 장치 조건 추가'}
                          </Button>
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <Callout title="연결된 키보드가 없습니다.">지금은 후보를 만들 수 없지만, 저장된 selector는 그대로 유지됩니다.</Callout>
                )}
                <Callout tone="warning" title="동일 모델 여러 대는 아직 구분하지 못합니다.">
                  이 selector는 devicePath나 현재 세션 ID를 저장하지 않습니다. 같은 VID/PID와 이름을 가진 키보드가 여러 대 연결되면 모두 같은 조건으로 취급됩니다.
                </Callout>
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
            <div><Button onClick={onClose}>취소</Button><Button variant="primary" disabled={saving || !draft.name.trim()} icon={saving ? <CircleDot className="spin" size={17} /> : <Save size={17} />} onClick={() => void onSave({ ...draft, name: draft.name.trim(), updatedAt: new Date().toISOString() })}>{saving ? '저장 중…' : '저장 및 적용'}</Button></div>
          </footer>
        </div>
      </Modal>
      <RuleComposer rule={editingRule} open={Boolean(editingRule)} onClose={() => setEditingRule(null)} onCommit={commitRule} />
    </>
  );
}
