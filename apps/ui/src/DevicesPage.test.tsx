import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { keyforgeBridge } from './lib/bridge';
import { DevicesPage } from './pages';
import type { KeyboardDeviceInfo } from './types';

const keyboard: KeyboardDeviceInfo = {
  id: 'rawkbd-0123456789abcdef',
  name: '테스트 기계식 키보드',
  devicePath: String.raw`\\?\HID#VID_046D&PID_C31C&MI_00#7&1234&0&0000`,
  manufacturer: 'Example Devices',
  instanceId: String.raw`HID\VID_046D&PID_C31C&MI_00\7&1234&0&0000`,
  containerId: '{01234567-89ab-cdef-0123-456789abcdef}',
  hardwareIds: [String.raw`HID_DEVICE_SYSTEM_KEYBOARD`, String.raw`HID_DEVICE_UP:0001_U:0006`],
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
};

describe('DevicesPage', () => {
  afterEach(() => vi.restoreAllMocks());

  it('renders only keyboard data returned by the native inventory bridge', async () => {
    const inventory = vi.spyOn(keyforgeBridge, 'listConnectedKeyboards').mockResolvedValue([keyboard]);
    const user = userEvent.setup();
    render(<DevicesPage />);

    const heading = await screen.findByRole('heading', { name: keyboard.name });
    const card = heading.closest('article');
    expect(card).not.toBeNull();
    if (!card) throw new Error('device card not found');
    expect(within(card).getByText(keyboard.name)).toBeInTheDocument();
    expect(within(card).getByText('046D')).toBeInTheDocument();
    expect(within(card).getByText('C31C')).toBeInTheDocument();
    expect(within(card).getByText('104')).toBeInTheDocument();
    expect(within(card).getByText('Example Devices')).toBeInTheDocument();
    expect(within(card).getByText(keyboard.containerId!)).toBeInTheDocument();
    expect(within(card).getByText(keyboard.instanceId!)).toBeInTheDocument();
    expect(screen.queryByText('USB Mechanical Keyboard')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '새로 고침' }));
    await waitFor(() => expect(inventory).toHaveBeenCalledTimes(2));
  });

  it('shows an actionable error instead of fake fallback devices', async () => {
    vi.spyOn(keyforgeBridge, 'listConnectedKeyboards').mockRejectedValue(new Error('Raw Input 실패'));
    render(<DevicesPage />);

    expect(await screen.findByText('키보드 목록을 읽지 못했습니다.')).toBeInTheDocument();
    expect(screen.getByText('Raw Input 실패')).toBeInTheDocument();
    expect(screen.queryByText('USB Mechanical Keyboard')).not.toBeInTheDocument();
  });

  it('renders a disconnected-during-query endpoint as partial information', async () => {
    vi.spyOn(keyforgeBridge, 'listConnectedKeyboards').mockResolvedValue([{
      ...keyboard,
      id: 'rawkbd-partial',
      name: 'Windows 키보드',
      manufacturer: null,
      instanceId: null,
      containerId: null,
      hardwareIds: [],
      locationPaths: [],
    }]);
    render(<DevicesPage />);

    expect(await screen.findByRole('heading', { name: 'Windows 키보드' })).toBeInTheDocument();
    expect(screen.getByText('제조사 정보 없음')).toBeInTheDocument();
    expect(screen.getByText('부분 정보')).toBeInTheDocument();
  });

  it('keeps the last successful inventory when refresh fails', async () => {
    vi.spyOn(keyforgeBridge, 'listConnectedKeyboards')
      .mockResolvedValueOnce([keyboard])
      .mockRejectedValueOnce(new Error('새로 고침 실패'));
    const user = userEvent.setup();
    render(<DevicesPage />);

    expect(await screen.findByRole('heading', { name: keyboard.name })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '새로 고침' }));

    expect(await screen.findByText('새로 고침 실패')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: keyboard.name })).toBeInTheDocument();
  });
});
