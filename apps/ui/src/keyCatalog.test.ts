import { describe, expect, it } from 'vitest';
import { keyboardEventToKey, orderChord, parseChordText } from './keyCatalog';

function keyboardEvent(key: string, code: string) {
  return { key, code } as KeyboardEvent;
}

describe('keyboard catalog', () => {
  it('uses physical codes for left/right and numpad keys', () => {
    expect(keyboardEventToKey(keyboardEvent('Control', 'ControlRight'))).toBe('ControlRight');
    expect(keyboardEventToKey(keyboardEvent('Enter', 'NumpadEnter'))).toBe('NumpadEnter');
    expect(keyboardEventToKey(keyboardEvent('MediaPlayPause', 'MediaPlayPause'))).toBe('MediaPlayPause');
  });

  it('normalizes letters and keeps modifiers first in a chord', () => {
    expect(keyboardEventToKey(keyboardEvent('k', 'KeyK'))).toBe('K');
    expect(orderChord(['K', 'ControlRight', 'ShiftLeft'])).toEqual([
      'ControlRight',
      'ShiftLeft',
      'K',
    ]);
  });

  it('parses typed chord aliases into canonical ordered keys', () => {
    expect(parseChordText('leftalt + space')).toEqual(['AltLeft', 'Space']);
    expect(parseChordText('rightctrl + k')).toEqual(['ControlRight', 'K']);
    expect(parseChordText('Shift + rightalt + f')).toEqual(['ShiftLeft', 'AltRight', 'F']);
  });
});
