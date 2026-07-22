export interface KeyOptionGroup {
  label: string;
  options: Array<{ value: string; label: string }>;
}

const options = (values: Array<string | [string, string]>) =>
  values.map((value) =>
    typeof value === 'string' ? { value, label: value } : { value: value[0], label: value[1] },
  );

export const KEY_OPTION_GROUPS: KeyOptionGroup[] = [
  {
    label: '문자',
    options: options('ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('')),
  },
  {
    label: '숫자',
    options: options('0123456789'.split('')),
  },
  {
    label: '보조키',
    options: options([
      ['ControlLeft', '왼쪽 Ctrl'],
      ['ControlRight', '오른쪽 Ctrl'],
      ['ShiftLeft', '왼쪽 Shift'],
      ['ShiftRight', '오른쪽 Shift'],
      ['AltLeft', '왼쪽 Alt'],
      ['AltRight', '오른쪽 Alt'],
      ['MetaLeft', '왼쪽 Windows'],
      ['MetaRight', '오른쪽 Windows'],
    ]),
  },
  {
    label: '일반·이동키',
    options: options([
      'Escape', 'Tab', 'CapsLock', 'Space', 'Enter', 'Backspace', 'Insert', 'Delete',
      'Home', 'End', 'PageUp', 'PageDown', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown',
      'PrintScreen', 'ScrollLock', 'Pause', 'NumLock', 'ContextMenu',
    ]),
  },
  {
    label: '기능키',
    options: options(Array.from({ length: 24 }, (_, index) => `F${index + 1}`)),
  },
  {
    label: '숫자패드',
    options: options([
      'Numpad0', 'Numpad1', 'Numpad2', 'Numpad3', 'Numpad4',
      'Numpad5', 'Numpad6', 'Numpad7', 'Numpad8', 'Numpad9',
      'NumpadDecimal', 'NumpadAdd', 'NumpadSubtract', 'NumpadMultiply',
      'NumpadDivide', 'NumpadEnter', 'NumpadEqual', 'NumpadComma',
    ]),
  },
  {
    label: '문장부호·OEM',
    options: options([
      ['Backquote', '` / ~'],
      ['Minus', '- / _'],
      ['Equal', '= / +'],
      ['BracketLeft', '[ / {'],
      ['BracketRight', '] / }'],
      ['Backslash', '\\ / |'],
      ['Semicolon', '; / :'],
      ['Quote', "' / \""],
      ['Comma', ', / <'],
      ['Period', '. / >'],
      ['Slash', '/ / ?'],
      'IntlBackslash', 'IntlRo', 'IntlYen',
    ]),
  },
  {
    label: '한국어·국제 입력',
    options: options([
      ['Lang1', '한/영'],
      ['Lang2', '한자'],
      'Lang3', 'Lang4', 'Lang5', 'Convert', 'NonConvert', 'KanaMode',
    ]),
  },
  {
    label: '미디어·볼륨',
    options: options([
      'AudioVolumeMute', 'AudioVolumeDown', 'AudioVolumeUp',
      'MediaTrackNext', 'MediaTrackPrevious', 'MediaStop', 'MediaPlayPause',
      'LaunchMail', 'LaunchMediaPlayer', 'LaunchApp1', 'LaunchApp2',
    ]),
  },
  {
    label: '브라우저',
    options: options([
      'BrowserBack', 'BrowserForward', 'BrowserRefresh', 'BrowserStop',
      'BrowserSearch', 'BrowserFavorites', 'BrowserHome',
    ]),
  },
];

const keyAliases: Record<string, string> = {
  ' ': 'Space',
  Spacebar: 'Space',
  Esc: 'Escape',
  OS: 'MetaLeft',
  VolumeMute: 'AudioVolumeMute',
  VolumeDown: 'AudioVolumeDown',
  VolumeUp: 'AudioVolumeUp',
  MediaNextTrack: 'MediaTrackNext',
  MediaPreviousTrack: 'MediaTrackPrevious',
  MediaSelect: 'LaunchMediaPlayer',
  HangulMode: 'Lang1',
  HanjaMode: 'Lang2',
};

const typedKeyAliases: Record<string, string> = {
  alt: 'AltLeft',
  altleft: 'AltLeft',
  leftalt: 'AltLeft',
  altright: 'AltRight',
  rightalt: 'AltRight',
  ctrl: 'ControlLeft',
  control: 'ControlLeft',
  controlleft: 'ControlLeft',
  leftctrl: 'ControlLeft',
  leftcontrol: 'ControlLeft',
  controlright: 'ControlRight',
  rightctrl: 'ControlRight',
  rightcontrol: 'ControlRight',
  shift: 'ShiftLeft',
  shiftleft: 'ShiftLeft',
  leftshift: 'ShiftLeft',
  shiftright: 'ShiftRight',
  rightshift: 'ShiftRight',
  win: 'MetaLeft',
  windows: 'MetaLeft',
  metaleft: 'MetaLeft',
  leftwin: 'MetaLeft',
  leftwindows: 'MetaLeft',
  metaright: 'MetaRight',
  rightwin: 'MetaRight',
  rightwindows: 'MetaRight',
  context: 'ContextMenu',
  apps: 'ContextMenu',
  menu: 'ContextMenu',
};

const listedKeys = new Map(
  KEY_OPTION_GROUPS.flatMap((group) => group.options.map((option) => [option.value.toLowerCase(), option.value] as const)),
);

export function keyboardEventToKey(event: KeyboardEvent): string | null {
  const code = event.code;
  if (code && code !== 'Unidentified') {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3);
    if (/^Digit[0-9]$/.test(code)) return code.slice(5);
    return code;
  }
  const key = keyAliases[event.key] ?? event.key;
  if (!key || key === 'Unidentified' || key === 'Dead' || key === 'Process') return null;
  return key.length === 1 ? key.toUpperCase() : key;
}

export function parseChordText(value: string): string[] {
  return orderChord(
    value
      .split('+')
      .map((item) => item.trim())
      .filter(Boolean)
      .map((item) => {
        if (/^[a-z]$/i.test(item)) return item.toUpperCase();
        const lookup = item.toLowerCase().replace(/\s+/g, '');
        return typedKeyAliases[lookup] ?? listedKeys.get(item.toLowerCase()) ?? item;
      }),
  );
}

const modifierOrder = [
  'ControlLeft', 'ControlRight', 'ShiftLeft', 'ShiftRight',
  'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight',
];

export function orderChord(keys: Iterable<string>): string[] {
  const unique = [...new Set(keys)];
  return unique.sort((left, right) => {
    const leftIndex = modifierOrder.indexOf(left);
    const rightIndex = modifierOrder.indexOf(right);
    if (leftIndex >= 0 || rightIndex >= 0) {
      return (leftIndex >= 0 ? leftIndex : modifierOrder.length) -
        (rightIndex >= 0 ? rightIndex : modifierOrder.length);
    }
    return left.localeCompare(right);
  });
}

export function isModifierKey(key: string): boolean {
  return modifierOrder.includes(key);
}

