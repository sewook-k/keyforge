import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { makeProfile, makeRule } from '../data';
import { keyforgeBridge } from '../lib/bridge';
import type { Profile } from '../types';
import { ProfileEditor } from './ProfileEditor';

describe('ProfileEditor', () => {
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

  it('adds a connected keyboard selector and persists the activation block', async () => {
    const user = userEvent.setup();
    vi.spyOn(keyforgeBridge, 'listConnectedKeyboards').mockResolvedValue([{
      id: 'rawkbd-046dc31c',
      name: '회사 키보드',
      devicePath: String.raw`\\?\HID#VID_046D&PID_C31C&MI_00#7&1234&0&0000`,
      manufacturer: 'Example Devices',
      instanceId: String.raw`HID\VID_046D&PID_C31C&MI_00\7&1234&0&0000`,
      containerId: '{01234567-89ab-cdef-0123-456789abcdef}',
      hardwareIds: [String.raw`HID_DEVICE_SYSTEM_KEYBOARD`],
      locationPaths: [String.raw`PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(3)`],
      vendorId: '046D',
      productId: 'C31C',
      interfaceId: '00',
      keyboardType: 4,
      keyboardSubType: 0,
      keyboardMode: 1,
      functionKeyCount: 12,
      indicatorCount: 3,
      totalKeyCount: 104,
      isVirtual: false,
      source: 'raw_input',
    }]);
    const onSave = vi.fn(async (_profile: Profile): Promise<void> => undefined);

    render(
      <ProfileEditor
        open
        profile={makeProfile('회사 레이아웃')}
        isNew={false}
        saving={false}
        onClose={vi.fn()}
        onSave={onSave}
      />,
    );

    const dialog = await screen.findByRole('dialog', { name: '회사 레이아웃' });
    await user.click(within(dialog).getByRole('button', { name: '적용 조건' }));
    await user.click(await within(dialog).findByRole('button', { name: '이 장치 조건 추가' }));
    expect(within(dialog).getAllByText('VID_046D / PID_C31C · MI_00 · 제조사:Example Devices · 이름:회사 키보드 · 실장치').length).toBeGreaterThan(0);

    await user.click(within(dialog).getByRole('button', { name: '저장 및 적용' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].activation.connectedKeyboards).toEqual([
      {
        vendorId: '046D',
        productId: 'C31C',
        interfaceId: '00',
        manufacturerContains: 'Example Devices',
        nameContains: '회사 키보드',
        isVirtual: false,
      },
    ]);
  });
});
