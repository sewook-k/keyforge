import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { makeProfile, makeRule } from '../data';
import { keyforgeBridge, type KeyCaptureDrain } from '../lib/bridge';
import type { Profile } from '../types';
import { ProfileEditor } from './ProfileEditor';

describe('ProfileEditor', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('captures AltLeft + Space and prevents the browser default while the dialog is open', async () => {
    const user = userEvent.setup();
    const beginCapture = vi.spyOn(keyforgeBridge, 'beginKeyCapture');
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture');
    const profile: Profile = {
      ...makeProfile('Alt Space capture'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Alt Space capture' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });
    expect(beginCapture).toHaveBeenCalledOnce();

    const altDown = new KeyboardEvent('keydown', {
      altKey: true,
      bubbles: true,
      cancelable: true,
      code: 'AltLeft',
      key: 'Alt',
    });
    const spaceDown = new KeyboardEvent('keydown', {
      altKey: true,
      bubbles: true,
      cancelable: true,
      code: 'Space',
      key: ' ',
    });
    window.dispatchEvent(altDown);
    window.dispatchEvent(spaceDown);

    expect(altDown.defaultPrevented).toBe(true);
    expect(spaceDown.defaultPrevented).toBe(true);
    expect(await within(captureDialog).findByText('AltLeft + Space')).toBeInTheDocument();
    await user.click(within(captureDialog).getByRole('button', { name: '이 입력 사용' }));
    await waitFor(() => expect(endCapture).toHaveBeenCalledTimes(1));
  });

  it('uses the native capture stream for AltLeft + Space, applies it, and saves the rule', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockResolvedValue({ sessionId: 42 });
    vi.spyOn(keyforgeBridge, 'endKeyCapture').mockResolvedValue();
    const drain = vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents').mockResolvedValue({
      sessionId: 42,
      active: true,
      overflowed: false,
      events: [],
    });
    drain.mockResolvedValueOnce({
      sessionId: 42,
      active: true,
      overflowed: false,
      events: [
        { sessionId: 42, key: 'AltLeft', phase: 'down' },
        { sessionId: 42, key: 'Space', phase: 'down' },
        { sessionId: 42, key: 'Space', phase: 'up' },
        { sessionId: 42, key: 'AltLeft', phase: 'up' },
      ],
    });
    const profile: Profile = {
      ...makeProfile('Native Alt Space capture'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };
    const onSave = vi.fn(async (_profile: Profile): Promise<void> => undefined);

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={onSave}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Native Alt Space capture' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });

    expect(await within(captureDialog).findByText('AltLeft + Space')).toBeInTheDocument();
    expect(drain).toHaveBeenCalledWith(42);
    await user.click(within(captureDialog).getByRole('button', { name: '이 입력 사용' }));
    expect(await within(ruleDialog).findByText('AltLeft + Space')).toBeInTheDocument();
    await user.click(within(ruleDialog).getByRole('button', { name: '규칙 적용' }));
    await user.click(within(profileDialog).getByRole('button', { name: '저장 및 적용' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].rules[0].trigger).toMatchObject({
      kind: 'keyboard',
      chord: ['AltLeft', 'Space'],
    });
  });
  it('waits for an input teardown before beginning native output capture and saves the output chord', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    const beginCapture = vi.spyOn(keyforgeBridge, 'beginKeyCapture')
      .mockResolvedValueOnce({ sessionId: 86 })
      .mockResolvedValueOnce({ sessionId: 87 });
    let resolveInputEnd: () => void = () => {
      throw new Error('input teardown resolver was not initialized');
    };
    const delayedInputEnd = new Promise<void>((resolve) => {
      resolveInputEnd = resolve;
    });
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture')
      .mockImplementation((sessionId) => (sessionId === 86 ? delayedInputEnd : Promise.resolve()));
    const drain = vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents')
      .mockImplementation(async (sessionId): Promise<KeyCaptureDrain> => ({
        sessionId,
        active: true,
        overflowed: false,
        events: sessionId === 86
          ? [{ sessionId, key: 'ControlLeft', phase: 'down' }]
          : [
            { sessionId, key: 'AltLeft', phase: 'down' },
            { sessionId, key: 'Space', phase: 'down' },
          ],
      }));
    const profile: Profile = {
      ...makeProfile('Native output session handoff'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };
    const onSave = vi.fn(async (_profile: Profile): Promise<void> => undefined);

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={onSave}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Native output session handoff' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const inputCapture = await screen.findByRole('dialog', { name: '입력 키 선택' });
    expect(await within(inputCapture).findByText('ControlLeft')).toBeInTheDocument();

    await user.click(within(inputCapture).getByRole('button', { name: '이 입력 사용' }));
    await waitFor(() => expect(endCapture).toHaveBeenCalledWith(86));
    await user.click(within(ruleDialog).getByRole('button', { name: '전송할 키 직접 누르기' }));
    expect(beginCapture).toHaveBeenCalledOnce();

    resolveInputEnd();

    await waitFor(() => expect(beginCapture).toHaveBeenCalledTimes(2));
    const outputCapture = await screen.findByRole('dialog', { name: '전송 키 선택' });
    expect(await within(outputCapture).findByText('AltLeft + Space')).toBeInTheDocument();
    expect(drain).toHaveBeenCalledWith(87);

    await user.click(within(outputCapture).getByRole('button', { name: '이 출력 사용' }));
    await waitFor(() => expect(endCapture).toHaveBeenNthCalledWith(2, 87));
    expect(endCapture).toHaveBeenCalledTimes(2);
    expect(within(ruleDialog).getByLabelText('전송할 키 조합 직접 입력')).toHaveValue('AltLeft + Space');

    await user.click(within(ruleDialog).getByRole('button', { name: '규칙 적용' }));
    await user.click(within(profileDialog).getByRole('button', { name: '저장 및 적용' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].rules[0].action).toEqual({
      kind: 'send_keys',
      chord: ['AltLeft', 'Space'],
    });
  });
  it('does not begin native output capture when input teardown rejects', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    const beginCapture = vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockResolvedValue({ sessionId: 88 });
    let rejectInputEnd: (reason?: unknown) => void = () => {
      throw new Error('input teardown rejecter was not initialized');
    };
    const inputEnd = new Promise<void>((_resolve, reject) => {
      rejectInputEnd = reject;
    });
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture')
      .mockImplementation((sessionId) => (sessionId === 88 ? inputEnd : Promise.resolve()));
    vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents').mockResolvedValue({
      sessionId: 88,
      active: true,
      overflowed: false,
      events: [{ sessionId: 88, key: 'ControlLeft', phase: 'down' }],
    });
    const profile: Profile = {
      ...makeProfile('Rejected input-to-output handoff'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Rejected input-to-output handoff' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const inputCapture = await screen.findByRole('dialog', { name: '입력 키 선택' });
    expect(await within(inputCapture).findByText('ControlLeft')).toBeInTheDocument();

    await user.click(within(inputCapture).getByRole('button', { name: '이 입력 사용' }));
    await waitFor(() => expect(endCapture).toHaveBeenCalledWith(88));
    await user.click(within(ruleDialog).getByRole('button', { name: '전송할 키 직접 누르기' }));
    expect(beginCapture).toHaveBeenCalledOnce();

    rejectInputEnd(new Error('teardown failed'));

    expect(await within(ruleDialog).findByText(/^이전 키 캡처를 종료하지 못했습니다\./)).toBeInTheDocument();
    expect(beginCapture).toHaveBeenCalledOnce();
    expect(screen.queryByRole('dialog', { name: '전송 키 선택' })).not.toBeInTheDocument();
  });
  it('ends a delayed capture session once after the composer closes and unmounts without opening a dialog', async () => {
    const user = userEvent.setup();
    let resolveBegin: (session: { sessionId: number }) => void = () => {
      throw new Error('begin resolver was not initialized');
    };
    const delayedBegin = new Promise<{ sessionId: number }>((resolve) => {
      resolveBegin = resolve;
    });
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockReturnValue(delayedBegin);
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture').mockResolvedValue();
    const profile: Profile = {
      ...makeProfile('Delayed capture cleanup'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    const { unmount } = render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Delayed capture cleanup' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    await waitFor(() => expect(keyforgeBridge.beginKeyCapture).toHaveBeenCalledOnce());

    await user.click(within(ruleDialog).getByRole('button', { name: '취소' }));
    resolveBegin({ sessionId: 81 });

    await waitFor(() => expect(endCapture).toHaveBeenCalledWith(81));
    expect(screen.queryByRole('dialog', { name: '입력 키 선택' })).not.toBeInTheDocument();
    unmount();
    expect(endCapture).toHaveBeenCalledOnce();
  });

  it('keeps DOM keys native-gated until a pending native teardown succeeds', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockResolvedValue({ sessionId: 82 });
    let resolveEnd: () => void = () => {
      throw new Error('end resolver was not initialized');
    };
    const pendingEnd = new Promise<void>((resolve) => {
      resolveEnd = resolve;
    });
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture').mockReturnValue(pendingEnd);
    vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents').mockResolvedValue({
      sessionId: 82,
      active: false,
      overflowed: false,
      events: [],
    });
    const profile: Profile = {
      ...makeProfile('Pending teardown capture'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Pending teardown capture' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });
    await waitFor(() => expect(endCapture).toHaveBeenCalledWith(82));

    const pendingKey = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      code: 'KeyQ',
      key: 'q',
    });
    window.dispatchEvent(pendingKey);

    expect(pendingKey.defaultPrevented).toBe(true);
    expect(within(captureDialog).getByRole('button', { name: '이 입력 사용' })).toBeDisabled();

    resolveEnd();
    await screen.findByText('네이티브 캡처가 중단되어 일반 키 입력 모드로 전환했습니다.');

    const capturedKey = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      code: 'KeyQ',
      key: 'q',
    });
    window.dispatchEvent(capturedKey);

    expect(capturedKey.defaultPrevented).toBe(true);
    expect(await within(captureDialog).findByText('Q')).toBeInTheDocument();
  });

  it('keeps DOM input native-gated and warns when native teardown rejects', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockResolvedValue({ sessionId: 83 });
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture').mockRejectedValue(new Error('teardown failed'));
    vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents').mockResolvedValue({
      sessionId: 83,
      active: false,
      overflowed: false,
      events: [],
    });
    const profile: Profile = {
      ...makeProfile('Rejected teardown capture'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Rejected teardown capture' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });

    expect(await within(captureDialog).findByText(/네이티브 캡처를 종료하지 못해 일반 키 입력으로 전환하지 않았습니다\./)).toBeInTheDocument();
    expect(endCapture).toHaveBeenCalledWith(83);

    const key = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      code: 'KeyR',
      key: 'r',
    });
    window.dispatchEvent(key);

    expect(key.defaultPrevented).toBe(true);
    expect(within(captureDialog).getByRole('button', { name: '이 입력 사용' })).toBeDisabled();
    expect(within(captureDialog).queryByText('R')).not.toBeInTheDocument();
  });

  it('ends the owned session and clears a partial chord when a drain reports another session', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockResolvedValue({ sessionId: 84 });
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture').mockResolvedValue();
    let resolveMismatch: (drain: KeyCaptureDrain) => void = () => {
      throw new Error('mismatched drain resolver was not initialized');
    };
    const mismatchedDrain = new Promise<KeyCaptureDrain>((resolve) => {
      resolveMismatch = resolve;
    });
    const drain = vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents')
      .mockResolvedValueOnce({
        sessionId: 84,
        active: true,
        overflowed: false,
        events: [{ sessionId: 84, key: 'AltLeft', phase: 'down' }],
      })
      .mockReturnValueOnce(mismatchedDrain);
    const profile: Profile = {
      ...makeProfile('Mismatched drain capture'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Mismatched drain capture' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });
    expect(await within(captureDialog).findByText('AltLeft')).toBeInTheDocument();
    await waitFor(() => expect(drain).toHaveBeenCalledTimes(2));

    resolveMismatch({
      sessionId: 904,
      active: true,
      overflowed: false,
      events: [],
    });

    await waitFor(() => expect(endCapture).toHaveBeenCalledWith(84));
    expect(endCapture).not.toHaveBeenCalledWith(904);
    expect(await within(captureDialog).findByText('대기 중…')).toBeInTheDocument();
    expect(within(captureDialog).getByRole('button', { name: '이 입력 사용' })).toBeDisabled();
  });

  it('shows the first ordinary key immediately while a native drain is still pending', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockResolvedValue({ sessionId: 85 });
    let resolveNativeDrain: (drain: KeyCaptureDrain) => void = () => {
      throw new Error('native drain resolver was not initialized');
    };
    const nativeDrain = new Promise<KeyCaptureDrain>((resolve) => {
      resolveNativeDrain = resolve;
    });
    const drain = vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents')
      .mockReturnValueOnce(nativeDrain)
      .mockResolvedValue({
        sessionId: 85,
        active: true,
        overflowed: false,
        events: [],
      });
    const profile: Profile = {
      ...makeProfile('Native first key responsiveness'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Native first key responsiveness' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });
    await waitFor(() => expect(drain).toHaveBeenCalledWith(85));

    const domKey = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      code: 'KeyQ',
      key: 'q',
    });
    window.dispatchEvent(domKey);

    expect(domKey.defaultPrevented).toBe(true);
    expect(await within(captureDialog).findByText('Q')).toBeInTheDocument();
    expect(within(captureDialog).getByRole('button', { name: '이 입력 사용' })).toBeEnabled();

    resolveNativeDrain({
      sessionId: 85,
      active: true,
      overflowed: false,
      events: [{ sessionId: 85, key: 'Q', phase: 'down' }],
    });

    expect(await within(captureDialog).findByText('Q')).toBeInTheDocument();
    expect(within(captureDialog).queryByText('Q + Q')).not.toBeInTheDocument();
  });

  it('ends an overflowed native capture before accepting a clean DOM chord', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'isNative').mockReturnValue(true);
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockResolvedValue({ sessionId: 77 });
    const endCapture = vi.spyOn(keyforgeBridge, 'endKeyCapture').mockResolvedValue();
    let releaseOverflow: (drain: KeyCaptureDrain) => void = () => {
      throw new Error('overflow drain resolver was not initialized');
    };
    const overflowDrain = new Promise<KeyCaptureDrain>((resolve) => {
      releaseOverflow = resolve;
    });
    const drain = vi.spyOn(keyforgeBridge, 'drainKeyCaptureEvents')
      .mockResolvedValueOnce({
        sessionId: 77,
        active: true,
        overflowed: false,
        events: [{ sessionId: 77, key: 'AltLeft', phase: 'down' }],
      })
      .mockReturnValueOnce(overflowDrain);
    const profile: Profile = {
      ...makeProfile('Native fallback capture'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    const { unmount } = render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Native fallback capture' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });
    expect(await within(captureDialog).findByText('AltLeft')).toBeInTheDocument();
    await waitFor(() => expect(drain).toHaveBeenCalledTimes(2));
    releaseOverflow({
      sessionId: 77,
      active: true,
      overflowed: true,
      events: [],
    });
    await waitFor(() => expect(endCapture).toHaveBeenCalledOnce());
    expect(endCapture).toHaveBeenCalledWith(77);
    expect(await within(captureDialog).findByText('네이티브 캡처를 다시 연결할 수 없습니다.')).toBeInTheDocument();

    const controlDown = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      code: 'ControlLeft',
      key: 'Control',
    });
    const keyDown = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      code: 'KeyQ',
      key: 'q',
    });
    window.dispatchEvent(controlDown);
    window.dispatchEvent(keyDown);

    expect(controlDown.defaultPrevented).toBe(true);
    expect(keyDown.defaultPrevented).toBe(true);
    expect(await within(captureDialog).findByText('ControlLeft + Q')).toBeInTheDocument();
    expect(within(captureDialog).queryByText('AltLeft + ControlLeft + Q')).not.toBeInTheDocument();

    await user.click(within(captureDialog).getByRole('button', { name: '취소' }));
    expect(endCapture).toHaveBeenCalledOnce();
    unmount();
    expect(endCapture).toHaveBeenCalledOnce();
  });

  it('does not open a capture dialog when the native guard cannot be enabled', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'beginKeyCapture').mockRejectedValue(new Error('native guard unavailable'));
    const profile: Profile = {
      ...makeProfile('Capture guard failure'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={vi.fn(async (_profile: Profile): Promise<void> => undefined)}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Capture guard failure' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));

    expect(await within(ruleDialog).findByText('키 캡처 보호를 시작하지 못했습니다. native guard unavailable')).toBeInTheDocument();
    expect(screen.queryByRole('dialog', { name: '입력 키 선택' })).not.toBeInTheDocument();
  });

  it('captures and applies a ControlLeft to MetaLeft remap while keeping key selectors', async () => {
    const user = userEvent.setup();
    const profile: Profile = {
      ...makeProfile('테스트 리맵'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };
    const onSave = vi.fn(async (_profile: Profile): Promise<void> => undefined);

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={onSave}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: '테스트 리맵' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });

    expect(within(ruleDialog).getByLabelText('입력 키 목록')).toBeInTheDocument();
    expect(within(ruleDialog).getByLabelText('전송할 키 목록')).toBeInTheDocument();
    const actionSelect = within(ruleDialog).getByLabelText('동작 종류') as HTMLSelectElement;
    expect(Array.from(actionSelect.options, (option) => option.value)).toEqual(['send_keys', 'send_mouse']);

    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const inputCapture = await screen.findByRole('dialog', { name: '입력 키 선택' });
    fireEvent.keyDown(window, { key: 'Control', code: 'ControlLeft', ctrlKey: true });
    expect(await within(inputCapture).findByText('ControlLeft')).toBeInTheDocument();
    await user.click(within(inputCapture).getByRole('button', { name: '이 입력 사용' }));

    await user.click(within(ruleDialog).getByRole('button', { name: '전송할 키 직접 누르기' }));
    const outputCapture = await screen.findByRole('dialog', { name: '전송 키 선택' });
    fireEvent.keyDown(window, { key: 'Meta', code: 'MetaLeft', metaKey: true });
    expect(await within(outputCapture).findByText('MetaLeft')).toBeInTheDocument();
    await user.click(within(outputCapture).getByRole('button', { name: '이 출력 사용' }));

    expect(within(ruleDialog).getByLabelText('전송할 키 조합 직접 입력')).toHaveValue('MetaLeft');
    expect(within(ruleDialog).getByLabelText('전송할 키 목록')).toHaveValue('MetaLeft');
    await user.click(within(ruleDialog).getByRole('button', { name: '규칙 적용' }));

    expect(within(profileDialog).getByText('ControlLeft')).toBeInTheDocument();
    expect(within(profileDialog).getByText('MetaLeft')).toBeInTheDocument();
    await user.click(within(profileDialog).getByRole('button', { name: '저장 및 적용' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    const saved = onSave.mock.calls[0][0];
    expect(saved.rules[0].trigger).toMatchObject({ kind: 'keyboard', chord: ['ControlLeft'] });
    expect(saved.rules[0].action).toEqual({ kind: 'send_keys', chord: ['MetaLeft'] });
  });

  it('allows typing an input chord directly when capture is unreliable', async () => {
    const user = userEvent.setup();
    const profile: Profile = {
      ...makeProfile('Typed trigger input'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };
    const onSave = vi.fn(async (_profile: Profile): Promise<void> => undefined);

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={onSave}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Typed trigger input' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });

    fireEvent.change(within(ruleDialog).getByLabelText('입력 키 조합 직접 입력'), {
      target: { value: 'leftalt + space' },
    });
    expect(within(ruleDialog).getByText('AltLeft + Space')).toBeInTheDocument();

    await user.click(within(ruleDialog).getByRole('button', { name: '규칙 적용' }));
    await user.click(within(profileDialog).getByRole('button', { name: '저장 및 적용' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].rules[0].trigger).toMatchObject({
      kind: 'keyboard',
      chord: ['AltLeft', 'Space'],
    });
  });

  it('combines up to three input keys from the selector list', async () => {
    const user = userEvent.setup();
    const profile: Profile = {
      ...makeProfile('Listed trigger chord'),
      rules: [{ ...makeRule('ControlLeft', 'Escape'), order: 0 }],
    };
    const onSave = vi.fn(async (_profile: Profile): Promise<void> => undefined);

    render(
      <ProfileEditor
        open
        profile={profile}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={onSave}
      />,
    );

    const profileDialog = await screen.findByRole('dialog', { name: 'Listed trigger chord' });
    await user.click(within(profileDialog).getByRole('button', { name: '편집' }));
    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });

    await user.selectOptions(within(ruleDialog).getByLabelText('입력 키 목록 2'), 'AltLeft');
    await user.selectOptions(within(ruleDialog).getByLabelText('입력 키 목록 3'), 'Space');
    expect(within(ruleDialog).getByText('ControlLeft + AltLeft + Space')).toBeInTheDocument();

    await user.click(within(ruleDialog).getByRole('button', { name: '규칙 적용' }));
    await user.click(within(profileDialog).getByRole('button', { name: '저장 및 적용' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].rules[0].trigger).toMatchObject({
      kind: 'keyboard',
      chord: ['ControlLeft', 'AltLeft', 'Space'],
    });
  });
});
