export type PageId =
  | 'dashboard'
  | 'profiles'
  | 'key-inspector'
  | 'devices'
  | 'activity'
  | 'settings';

export type EngineState = 'running' | 'paused' | 'error';
export type ResultStatus = 'success' | 'warning' | 'error';
export type ScopeKind = 'global' | 'application' | 'device' | 'combined';

export interface ScopeCondition {
  kind: 'process_name' | 'executable_path' | 'window_class' | 'device_id';
  operator: 'equals' | 'contains';
  value: string;
}

export type ProfileScope =
  | { kind: 'global' }
  | {
      kind: Exclude<ScopeKind, 'global'>;
      conditions: {
        operator: 'and' | 'or';
        conditions: ScopeCondition[];
      };
    };

export type RuleTrigger =
  | {
      kind: 'keyboard';
      chord: string[];
      phase: 'press' | 'release';
      gesture: 'single' | 'hold' | 'double';
    }
  | {
      kind: 'mouse';
      button: 'left' | 'right' | 'middle' | 'x1' | 'x2';
      phase: 'press' | 'release';
    };

export type RuleAction =
  | { kind: 'send_keys'; chord: string[] }
  | { kind: 'send_mouse'; button: 'left' | 'right' | 'middle' | 'x1' | 'x2' };

export interface Rule {
  id: string;
  order: number;
  enabled: boolean;
  trigger: RuleTrigger;
  action: RuleAction;
  options: {
    passThroughOriginal: boolean;
    ignoreInjected: boolean;
    maxExecutions: number;
    timeoutMs: number;
  };
}

export interface Profile {
  id: string;
  name: string;
  enabled: boolean;
  scope: ProfileScope;
  rules: Rule[];
  enableOnStartup: boolean;
  archived: boolean;
  createdAt: string;
  updatedAt: string;
  lastRunAt?: string | null;
}

export type ThemePreference = 'system' | 'light' | 'dark';

export interface AppPreferences {
  theme: ThemePreference;
  language: string;
  startMinimized: boolean;
  launchAtLogin: boolean;
  closeToTray: boolean;
  notifications: 'all' | 'warnings' | 'errors';
}

export interface EngineSettings {
  emergencyStop: string[];
  maxRuleExecutions: number;
  maxRuleDurationMs: number;
  ignoreAllInjectedInput: boolean;
}

export interface Settings {
  schemaVersion: number;
  revision: number;
  updatedAt: string;
  profiles: Profile[];
  preferences: AppPreferences;
  engine: EngineSettings;
}

export interface RuntimeState {
  engineState: EngineState;
  activeProfileCount: number;
  hookInstalled: boolean;
  capabilities: string[];
}

export interface ActionResult {
  actionId: string;
  actionType: string;
  status: ResultStatus;
  message: string;
  revision?: number | null;
  timestamp: string;
  recovery?: Recovery | null;
  details?: Record<string, unknown> | null;
}

export interface Recovery {
  attempted: boolean;
  succeeded?: boolean | null;
  message?: string | null;
  actions: Array<'retry' | 'restore_backup' | 'open_logs'>;
}

export interface BootstrapPayload {
  settings: Settings;
  runtime: RuntimeState;
  activity: ActionResult[];
  settingsPath: string;
}

export interface KeyboardDeviceInfo {
  id: string;
  name: string;
  devicePath: string;
  manufacturer: string | null;
  instanceId: string | null;
  containerId: string | null;
  hardwareIds: string[];
  locationPaths: string[];
  vendorId: string | null;
  productId: string | null;
  interfaceId: string | null;
  keyboardType: number;
  keyboardSubType: number;
  keyboardMode: number;
  functionKeyCount: number;
  indicatorCount: number;
  totalKeyCount: number;
  isVirtual: boolean;
  source: 'raw_input';
}

export interface ToastMessage {
  id: string;
  status: ResultStatus;
  title: string;
  description?: string;
  actionLabel?: string;
  onAction?: () => void;
}
