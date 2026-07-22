import type {
  ActionResult,
  BootstrapPayload,
  Profile,
  Rule,
  Settings,
} from './types';

const now = '2026-07-10T13:34:00.000Z';

export const makeId = () => {
  if (typeof crypto !== 'undefined' && crypto.randomUUID) return crypto.randomUUID();
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (value) => {
    const random = Math.floor(Math.random() * 16);
    return (value === 'x' ? random : (random & 0x3) | 0x8).toString(16);
  });
};

export const makeRule = (input = 'Caps Lock', output = 'Escape'): Rule => ({
  id: makeId(),
  order: 0,
  enabled: true,
  trigger: {
    kind: 'keyboard',
    chord: input.split(' + '),
    phase: 'press',
    gesture: 'single',
  },
  action: {
    kind: 'send_keys',
    chord: output.split(' + '),
  },
  options: {
    passThroughOriginal: false,
    ignoreInjected: true,
    maxExecutions: 1_000,
    timeoutMs: 300_000,
  },
});

export const makeProfile = (name = '새 전역 프로필'): Profile => {
  const timestamp = new Date().toISOString();
  return {
    id: makeId(),
    name,
    enabled: true,
    scope: { kind: 'global' },
    rules: [],
    enableOnStartup: false,
    archived: false,
    createdAt: timestamp,
    updatedAt: timestamp,
    lastRunAt: null,
  };
};

const defaultProfiles: Profile[] = [
  {
    ...makeProfile('기본 키 매핑'),
    id: '11111111-1111-4111-8111-111111111111',
    rules: [makeRule('Caps Lock', 'Escape'), makeRule('Right Alt', '한/영')],
    enableOnStartup: true,
    createdAt: '2026-07-08T09:20:00.000Z',
    updatedAt: now,
    lastRunAt: '2026-07-10T13:32:00.000Z',
  },
  {
    ...makeProfile('집중 모드'),
    id: '22222222-2222-4222-8222-222222222222',
    rules: [makeRule('F9', 'Ctrl + Shift + F')],
    createdAt: '2026-07-09T05:10:00.000Z',
    updatedAt: '2026-07-10T10:16:00.000Z',
    lastRunAt: '2026-07-10T12:48:00.000Z',
  },
  {
    ...makeProfile('편집 단축키'),
    id: '33333333-3333-4333-8333-333333333333',
    enabled: false,
    scope: { kind: 'global' },
    rules: [makeRule('Alt + J', 'Ctrl + J'), makeRule('Alt + K', 'Ctrl + K')],
    createdAt: '2026-07-09T06:30:00.000Z',
    updatedAt: '2026-07-10T08:05:00.000Z',
    lastRunAt: '2026-07-10T11:02:00.000Z',
  },
];

export const defaultSettings: Settings = {
  schemaVersion: 3,
  revision: 18,
  updatedAt: now,
  profiles: defaultProfiles,
  preferences: {
    theme: 'system',
    language: 'ko-KR',
    startMinimized: false,
    launchAtLogin: false,
    closeToTray: true,
    notifications: 'all',
  },
  engine: {
    emergencyStop: ['Control', 'Alt', 'Pause'],
    maxRuleExecutions: 1_000,
    maxRuleDurationMs: 300_000,
    ignoreAllInjectedInput: true,
  },
};

export const defaultActivity: ActionResult[] = [
  {
    actionId: 'act-08df2',
    actionType: 'save_settings',
    status: 'success',
    message: '기본 키 매핑을 저장하고 적용했습니다.',
    revision: 18,
    timestamp: now,
    recovery: null,
    details: { profileName: '기본 키 매핑', stage: 'runtime_applied' },
  },
  {
    actionId: 'act-7be91',
    actionType: 'profile_activated',
    status: 'success',
    message: '집중 모드 프로필이 활성화되었습니다.',
    revision: 17,
    timestamp: '2026-07-10T12:48:00.000Z',
    recovery: null,
  },
  {
    actionId: 'act-91f42',
    actionType: 'input_warning',
    status: 'warning',
    message: '중복 단축키를 감지해 우선순위가 높은 규칙만 실행했습니다.',
    revision: 17,
    timestamp: '2026-07-10T11:28:00.000Z',
    recovery: {
      attempted: true,
      succeeded: true,
      message: '입력 반복은 차단되었습니다.',
      actions: [],
    },
  },
];

export const defaultBootstrap: BootstrapPayload = {
  settings: defaultSettings,
  runtime: {
    engineState: 'running',
    activeProfileCount: 2,
    hookInstalled: true,
    capabilities: ['keyboard', 'mouse'],
  },
  activity: defaultActivity,
  settingsPath: 'C:\\Users\\Demo\\AppData\\Local\\KeyForge\\settings.json',
};
