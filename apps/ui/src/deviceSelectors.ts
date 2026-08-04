import type { DeviceSelector, KeyboardDeviceInfo, Profile, Settings } from './types';

const normalizeHex = (value?: string | null) => value?.trim().toUpperCase() || null;
const normalizeText = (value?: string | null) => value?.trim() || null;

export const makeDeviceSelector = (device: KeyboardDeviceInfo): DeviceSelector => ({
  vendorId: normalizeHex(device.vendorId),
  productId: normalizeHex(device.productId),
  interfaceId: normalizeHex(device.interfaceId),
  manufacturerContains: normalizeText(device.manufacturer),
  nameContains: normalizeText(device.name),
  isVirtual: device.isVirtual,
});

export const deviceSelectorMatches = (device: KeyboardDeviceInfo, selector: DeviceSelector): boolean => {
  const contains = (actual: string | null | undefined, expected: string | null | undefined) => (
    expected == null
      || (actual ?? '').toLowerCase().includes(expected.toLowerCase())
  );
  return (selector.vendorId == null || normalizeHex(device.vendorId) === normalizeHex(selector.vendorId))
    && (selector.productId == null || normalizeHex(device.productId) === normalizeHex(selector.productId))
    && (selector.interfaceId == null || normalizeHex(device.interfaceId) === normalizeHex(selector.interfaceId))
    && contains(device.manufacturer, selector.manufacturerContains ?? null)
    && contains(device.name, selector.nameContains ?? null)
    && (selector.isVirtual == null || device.isVirtual === selector.isVirtual);
};

export const profileActivationMatches = (profile: Profile, devices: KeyboardDeviceInfo[]): boolean => (
  !profile.archived
  && profile.enabled
  && (profile.activation.connectedKeyboards.length === 0
    || profile.activation.connectedKeyboards.some((selector) => devices.some((device) => deviceSelectorMatches(device, selector))))
);

export const countActiveProfiles = (settings: Settings, devices: KeyboardDeviceInfo[]): number => (
  settings.profiles.filter((profile) => profileActivationMatches(profile, devices)).length
);

export const selectorLabel = (selector: DeviceSelector): string => {
  const parts = [
    selector.vendorId && selector.productId ? `VID_${selector.vendorId} / PID_${selector.productId}` : null,
    selector.interfaceId ? `MI_${selector.interfaceId}` : null,
    selector.manufacturerContains ? `제조사:${selector.manufacturerContains}` : null,
    selector.nameContains ? `이름:${selector.nameContains}` : null,
    selector.isVirtual == null ? null : selector.isVirtual ? '가상 장치' : '실장치',
  ].filter((value): value is string => Boolean(value));
  return parts.join(' · ') || '빈 selector';
};
