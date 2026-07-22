import { describe, expect, it } from 'vitest';
import { makeProfile } from '../data';
import { KeyForgeCommandError, keyforgeBridge, normalizeBridgeError } from './bridge';

describe('browser mock bridge', () => {
  it('preserves structured native command failures for the toast and activity feed', () => {
    const result = {
      actionId: 'startup-failure',
      actionType: 'save_settings',
      status: 'error' as const,
      message: 'Windows 시작 프로그램에 등록하지 못했습니다.',
      revision: null,
      timestamp: '2026-07-12T00:00:00.000Z',
      recovery: null,
      details: null,
    };

    const error = normalizeBridgeError({
      code: 'startup_registration_failed',
      message: 'startup command is too long',
      result,
    });

    expect(error).toBeInstanceOf(KeyForgeCommandError);
    expect(error.message).toBe('startup command is too long');
    expect((error as KeyForgeCommandError).result).toEqual(result);
  });

  it('bootstraps a fully usable global-first settings model', async () => {
    const payload = await keyforgeBridge.bootstrap();

    expect(payload.runtime.engineState).toBe('running');
    expect(payload.settings.profiles[0].scope).toEqual({ kind: 'global' });
    expect(payload.settingsPath).toContain('KeyForge');
  });

  it('increments revisions and records save action results', async () => {
    const initial = await keyforgeBridge.bootstrap();
    const draft = {
      ...initial.settings,
      profiles: [...initial.settings.profiles, makeProfile('테스트 프로필')],
    };

    const saved = await keyforgeBridge.saveSettings(draft, initial.settings.revision);
    const reloaded = await keyforgeBridge.bootstrap();

    expect(saved.settings.revision).toBe(initial.settings.revision + 1);
    expect(saved.result.status).toBe('success');
    expect(reloaded.settings.profiles.some((profile) => profile.name === '테스트 프로필')).toBe(true);
    expect(reloaded.activity[0].actionType).toBe('save_settings');
  });

  it('creates and restores backups without overwriting the revision contract', async () => {
    const initial = await keyforgeBridge.bootstrap();
    const first = await keyforgeBridge.saveSettings(
      { ...initial.settings, profiles: [...initial.settings.profiles, makeProfile('백업 대상')] },
      initial.settings.revision,
    );
    await keyforgeBridge.createBackup();
    const second = await keyforgeBridge.saveSettings(
      { ...first.settings, profiles: first.settings.profiles.filter((profile) => profile.name !== '백업 대상') },
      first.settings.revision,
    );

    const restored = await keyforgeBridge.restoreBackup(second.settings.revision);

    expect(restored.settings.revision).toBe(second.settings.revision + 1);
    expect(restored.settings.profiles.some((profile) => profile.name === '백업 대상')).toBe(true);
    expect(restored.result.actionType).toBe('restore_backup');
  });

  it('persists the paused state in browser mode', async () => {
    await keyforgeBridge.setEnginePaused(true);
    expect((await keyforgeBridge.bootstrap()).runtime.engineState).toBe('paused');
    await keyforgeBridge.setEnginePaused(false);
    expect((await keyforgeBridge.bootstrap()).runtime.engineState).toBe('running');
  });
});
