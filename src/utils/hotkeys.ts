const CODE_KEY_MAP: Record<string, string> = {
  Minus: 'Minus',
  Equal: 'Equal',
  BracketLeft: 'BracketLeft',
  BracketRight: 'BracketRight',
  Backslash: 'Backslash',
  Semicolon: 'Semicolon',
  Quote: 'Quote',
  Comma: 'Comma',
  Period: 'Period',
  Slash: 'Slash',
  Backquote: 'Backquote',
  Space: 'Space',
};

export function formatKeyboardCombo(e: KeyboardEvent): string | null {
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
    return null;
  }

  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Control');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');
  if (e.metaKey) parts.push('Super');

  let key = e.key;
  if (e.code.startsWith('Numpad')) {
    const sub = e.code.slice(6);
    if (
      /^[0-9]$/.test(sub) ||
      ['Add', 'Subtract', 'Multiply', 'Divide', 'Decimal', 'Enter'].includes(sub)
    ) {
      key = e.code;
    }
  } else if (e.code.startsWith('Digit')) {
    key = e.code.slice(5);
  } else if (e.code.startsWith('Key')) {
    key = e.code.slice(3).toUpperCase();
  } else if (CODE_KEY_MAP[e.code]) {
    key = CODE_KEY_MAP[e.code];
  } else if (key.startsWith('Arrow')) {
    key = key.replace('Arrow', '');
  } else if (key === ' ') {
    key = 'Space';
  } else if (key === '+') {
    key = 'Equal';
  } else if (key.length === 1) {
    key = key.toUpperCase();
  }

  parts.push(key);
  return parts.join('+');
}
