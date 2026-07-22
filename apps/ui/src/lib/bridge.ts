import { invoke } from '@tauri-apps/api/core';
import { defaultBootstrap } from '../data';
import type { ActionResult, BootstrapPayload, KeyboardDeviceInfo, Settings } from '../types';

const SETTINGS_KEY = 'keyforge.mock.settings';
const ACTIVITY_KEY = 'keyforge.mock.activity';
const BACKUP_KEY = 'keyforge.mock.backup';
const ENGINE_KEY = 'keyforge.mock.enginePaused';
let mockCaptureSession = 0;

const clone = <T,>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const isTauri = () => typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);

type NativeCommandFailurePayload = {
  code: string;
  message: string;
  result?: ActionResult;
};

export type KeyCaptureEvent = {
  sessionId: number;
  key: string;
  phase: 'down' | 'up';
};

export type KeyCaptureSession = {
  sessionId: number;
};

export type KeyCaptureDrain = {
  sessionId: number;
  active: boolean;
  overflowed: boolean;
  events: KeyCaptureEvent[];
};

const isRecord = (value: unknown): value is Record<string, unknown> => (
  typeof value === 'object' && value !== null
);

const isActionResult = (value: unknown): value is ActionResult => (
  isRecord(value)
  && typeof value.actionId === 'string'
  && typeof value.actionType === 'string'
  && (value.status === 'success' || value.status === 'warning' || value.status === 'error')
  && typeof value.message === 'string'
  && typeof value.timestamp === 'string'
);

const isNativeCommandFailure = (value: unknown): value is NativeCommandFailurePayload => (
  isRecord(value)
  && typeof value.code === 'string'
  && typeof value.message === 'string'
  && (value.result === undefined || isActionResult(value.result))
);

export class KeyForgeCommandError extends Error {
  readonly code: string;
  readonly result: ActionResult | null;

  constructor({ code, message, result }: NativeCommandFailurePayload) {
    super(message);
    this.name = 'KeyForgeCommandError';
    this.code = code;
    this.result = result ?? null;
  }
}

export const normalizeBridgeError = (error: unknown): Error => {
  if (error instanceof KeyForgeCommandError) return error;
  if (isNativeCommandFailure(error)) return new KeyForgeCommandError(error);
  if (error instanceof Error) return error;
  return new Error(typeof error === 'string' ? error : 'KeyForge 명령을 완료하지 못했습니다.');
};

const nativeInvoke = async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeBridgeError(error);
  }
};

const makeResult = (
  actionType: string,
  status: ActionResult['status'],
  message: string,
  revision?: number,
): ActionResult => ({
  actionId: `${actionType}-${crypto.randomUUID?.() ?? Date.now()}`,
  actionType,
  status,
  message,
  revision,
  timestamp: new Date().toISOString(),
  recovery: null,
  details: { source: 'browser-mock' },
});

const readJson = <T,>(key: string, fallback: T): T => {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : clone(fallback);
  } catch {
    return clone(fallback);
  }
};

const writeActivity = (result: ActionResult) => {
  const items = readJson<ActionResult[]>(ACTIVITY_KEY, defaultBootstrap.activity);
  localStorage.setItem(ACTIVITY_KEY, JSON.stringify([result, ...items].slice(0, 100)));
};

const migrateMockSettings = (settings: Settings): Settings => ({
  ...settings,
  schemaVersion: 3,
  preferences: {
    ...defaultBootstrap.settings.preferences,
    ...settings.preferences,
    launchAtLogin: settings.preferences?.launchAtLogin ?? false,
  },
  profiles: settings.profiles.map((profile) => ({ ...profile, scope: { kind: 'global' } })),
});

export const keyforgeBridge = {
  isNative: isTauri,

  async bootstrap(): Promise<BootstrapPayload> {
    if (isTauri()) return nativeInvoke<BootstrapPayload>('bootstrap');

    await new Promise((resolve) => setTimeout(resolve, 180));
    const settings = migrateMockSettings(readJson(SETTINGS_KEY, defaultBootstrap.settings));
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    const activity = readJson(ACTIVITY_KEY, defaultBootstrap.activity);
    const paused = localStorage.getItem(ENGINE_KEY) === 'true';
    return {
      ...clone(defaultBootstrap),
      settings,
      activity,
      runtime: {
        ...defaultBootstrap.runtime,
        engineState: paused ? 'paused' : 'running',
        activeProfileCount: settings.profiles.filter((profile) => profile.enabled && !profile.archived)
          .length,
      },
    };
  },

  async listConnectedKeyboards(): Promise<KeyboardDeviceInfo[]> {
    if (isTauri()) return nativeInvoke<KeyboardDeviceInfo[]>('list_connected_keyboards');
    return [];
  },

  async beginKeyCapture(): Promise<KeyCaptureSession> {
    if (isTauri()) return nativeInvoke<KeyCaptureSession>('begin_key_capture');
    mockCaptureSession += 1;
    return { sessionId: mockCaptureSession };
  },

  async endKeyCapture(sessionId: number): Promise<void> {
    if (isTauri()) await nativeInvoke<void>('end_key_capture', { sessionId });
  },

  async drainKeyCaptureEvents(sessionId: number): Promise<KeyCaptureDrain> {
    if (isTauri()) {
      return nativeInvoke<KeyCaptureDrain>('drain_key_capture_events', { sessionId });
    }
    return { sessionId, active: true, overflowed: false, events: [] };
  },

  async saveSettings(
    draft: Settings,
    expectedRevision: number,
  ): Promise<{ settings: Settings; result: ActionResult }> {
    if (isTauri()) {
      return nativeInvoke<{ settings: Settings; result: ActionResult }>('save_settings', { draft, expectedRevision });
    }

    await new Promise((resolve) => setTimeout(resolve, 420));
    const current = readJson(SETTINGS_KEY, defaultBootstrap.settings);
    if (current.revision !== expectedRevision) {
      throw new Error(`설정이 다른 창에서 변경되었습니다. 현재 revision은 ${current.revision}입니다.`);
    }
    if (draft.profiles.some((profile) => profile.scope.kind !== 'global')) {
      throw new Error('이 빌드에서는 전역 프로필만 실행할 수 있습니다.');
    }

    const settings: Settings = {
      ...clone(draft),
      revision: expectedRevision + 1,
      updatedAt: new Date().toISOString(),
    };
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    const result = makeResult(
      'save_settings',
      'success',
      `설정을 저장하고 적용했습니다 · revision ${settings.revision}`,
      settings.revision,
    );
    writeActivity(result);
    return { settings, result };
  },

  async setEnginePaused(paused: boolean): Promise<ActionResult> {
    if (isTauri()) return nativeInvoke<ActionResult>('set_engine_paused', { paused });

    await new Promise((resolve) => setTimeout(resolve, 220));
    localStorage.setItem(ENGINE_KEY, String(paused));
    const result = makeResult(
      'set_engine_paused',
      'success',
      paused ? '모든 입력 규칙을 일시정지했습니다.' : '입력 엔진을 다시 시작했습니다.',
    );
    writeActivity(result);
    return result;
  },

  async createBackup(): Promise<ActionResult> {
    if (isTauri()) return nativeInvoke<ActionResult>('create_backup');

    await new Promise((resolve) => setTimeout(resolve, 300));
    const settings = readJson(SETTINGS_KEY, defaultBootstrap.settings);
    localStorage.setItem(BACKUP_KEY, JSON.stringify(settings));
    const result = makeResult(
      'create_backup',
      'success',
      `revision ${settings.revision} 백업을 만들었습니다.`,
      settings.revision,
    );
    writeActivity(result);
    return result;
  },

  async restoreBackup(
    expectedRevision: number,
  ): Promise<{ settings: Settings; result: ActionResult }> {
    if (isTauri()) return nativeInvoke<{ settings: Settings; result: ActionResult }>('restore_backup', { expectedRevision });

    await new Promise((resolve) => setTimeout(resolve, 360));
    const current = readJson(SETTINGS_KEY, defaultBootstrap.settings);
    if (current.revision !== expectedRevision) {
      throw new Error('복원 전에 설정이 변경되었습니다. 화면을 새로 고침한 뒤 다시 시도하세요.');
    }
    const backup = readJson<Settings | null>(BACKUP_KEY, null);
    if (!backup) throw new Error('복원할 백업이 없습니다. 먼저 백업을 만드세요.');

    const settings = {
      ...backup,
      revision: expectedRevision + 1,
      updatedAt: new Date().toISOString(),
    };
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    const result = makeResult(
      'restore_backup',
      'success',
      `백업을 복원했습니다 · revision ${settings.revision}`,
      settings.revision,
    );
    writeActivity(result);
    return { settings, result };
  },
};
