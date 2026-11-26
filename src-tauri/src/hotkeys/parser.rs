#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicalKey {
    Char(char), // 'A'..='Z'
    Digit(u8),  // 0..=9
    F(u8),      // 1..=24
    Numpad(u8), // 0..=9
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
    Space,
    Escape,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    CapsLock,
    Pause,
    PrintScreen,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Backquote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
    pub key: LogicalKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    Panic,
    ToggleOverlay,
    TriggerSound(String),
    ShowMainWindow,
}

#[derive(Debug, Clone)]
pub struct HotkeyBinding {
    pub combo: HotkeyCombo,
    pub action: HotkeyAction,
}

impl HotkeyCombo {
    /// Formats a combo in freedesktop XDG GlobalShortcuts preferred_trigger style (e.g. "CTRL+SHIFT+x").
    pub fn to_portal_trigger(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("CTRL");
        }
        if self.shift {
            parts.push("SHIFT");
        }
        if self.alt {
            parts.push("ALT");
        }
        if self.super_key {
            parts.push("LOGO");
        }
        let key_str = match self.key {
            LogicalKey::Char(c) => c.to_ascii_lowercase().to_string(),
            LogicalKey::Digit(d) => d.to_string(),
            LogicalKey::F(n) => format!("F{n}"),
            LogicalKey::Numpad(n) => format!("KP_{n}"),
            LogicalKey::NumpadAdd => "KP_Add".to_string(),
            LogicalKey::NumpadSubtract => "KP_Subtract".to_string(),
            LogicalKey::NumpadMultiply => "KP_Multiply".to_string(),
            LogicalKey::NumpadDivide => "KP_Divide".to_string(),
            LogicalKey::NumpadDecimal => "KP_Decimal".to_string(),
            LogicalKey::NumpadEnter => "KP_Enter".to_string(),
            LogicalKey::Space => "space".to_string(),
            LogicalKey::Escape => "Escape".to_string(),
            LogicalKey::Enter => "Return".to_string(),
            LogicalKey::Tab => "Tab".to_string(),
            LogicalKey::Backspace => "BackSpace".to_string(),
            LogicalKey::Delete => "Delete".to_string(),
            LogicalKey::Insert => "Insert".to_string(),
            LogicalKey::Home => "Home".to_string(),
            LogicalKey::End => "End".to_string(),
            LogicalKey::PageUp => "Page_Up".to_string(),
            LogicalKey::PageDown => "Page_Down".to_string(),
            LogicalKey::Up => "Up".to_string(),
            LogicalKey::Down => "Down".to_string(),
            LogicalKey::Left => "Left".to_string(),
            LogicalKey::Right => "Right".to_string(),
            LogicalKey::CapsLock => "Caps_Lock".to_string(),
            LogicalKey::Pause => "Pause".to_string(),
            LogicalKey::PrintScreen => "Print".to_string(),
            LogicalKey::Minus => "minus".to_string(),
            LogicalKey::Equal => "equal".to_string(),
            LogicalKey::BracketLeft => "bracketleft".to_string(),
            LogicalKey::BracketRight => "bracketright".to_string(),
            LogicalKey::Backslash => "backslash".to_string(),
            LogicalKey::Semicolon => "semicolon".to_string(),
            LogicalKey::Quote => "apostrophe".to_string(),
            LogicalKey::Comma => "comma".to_string(),
            LogicalKey::Period => "period".to_string(),
            LogicalKey::Slash => "slash".to_string(),
            LogicalKey::Backquote => "grave".to_string(),
        };
        if parts.is_empty() {
            key_str
        } else {
            format!("{}+{}", parts.join("+"), key_str)
        }
    }
}

pub fn parse_hotkey(spec: &str) -> Option<HotkeyCombo> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut super_key = false;
    let mut primary_key: Option<LogicalKey> = None;

    // Handle literal trailing '+' key (e.g. "+" or "Control+Shift++")
    let (body, trailing_plus) = if trimmed == "+" {
        ("", true)
    } else if let Some(prefix) = trimmed.strip_suffix("++") {
        (prefix, true)
    } else {
        (trimmed, false)
    };
    if trailing_plus {
        primary_key = Some(LogicalKey::Equal);
    }

    for raw_part in body.split('+') {
        let part = raw_part.trim();
        if part.is_empty() {
            continue;
        }
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "control" | "ctrl" | "commandorcontrol" | "cmdorctrl" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" => alt = true,
            "super" | "meta" | "win" | "command" | "cmd" => super_key = true,
            _ => {
                primary_key = parse_primary_key(part);
                if primary_key.is_none() {
                    return None;
                }
            }
        }
    }

    primary_key.map(|key| HotkeyCombo {
        ctrl,
        shift,
        alt,
        super_key,
        key,
    })
}

fn parse_primary_key(token: &str) -> Option<LogicalKey> {
    let t = token.trim();
    if t.len() == 1 {
        let ch = t.chars().next()?.to_ascii_uppercase();
        return match ch {
            'A'..='Z' => Some(LogicalKey::Char(ch)),
            '0'..='9' => Some(LogicalKey::Digit((ch as u8) - b'0')),
            '-' | '_' => Some(LogicalKey::Minus),
            '=' | '+' => Some(LogicalKey::Equal),
            '[' | '{' => Some(LogicalKey::BracketLeft),
            ']' | '}' => Some(LogicalKey::BracketRight),
            '\\' | '|' => Some(LogicalKey::Backslash),
            ';' | ':' => Some(LogicalKey::Semicolon),
            '\'' | '"' => Some(LogicalKey::Quote),
            ',' | '<' => Some(LogicalKey::Comma),
            '.' | '>' => Some(LogicalKey::Period),
            '/' | '?' => Some(LogicalKey::Slash),
            '`' | '~' => Some(LogicalKey::Backquote),
            '!' => Some(LogicalKey::Digit(1)),
            '@' => Some(LogicalKey::Digit(2)),
            '#' => Some(LogicalKey::Digit(3)),
            '$' => Some(LogicalKey::Digit(4)),
            '%' => Some(LogicalKey::Digit(5)),
            '^' => Some(LogicalKey::Digit(6)),
            '&' => Some(LogicalKey::Digit(7)),
            '*' => Some(LogicalKey::Digit(8)),
            '(' => Some(LogicalKey::Digit(9)),
            ')' => Some(LogicalKey::Digit(0)),
            _ => None,
        };
    }

    let lower = t.to_ascii_lowercase();
    if let Some(num) = lower.strip_prefix('f') {
        if let Ok(n) = num.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Some(LogicalKey::F(n));
            }
        }
    }
    if let Some(np) = lower.strip_prefix("numpad") {
        if let Ok(n) = np.parse::<u8>() {
            if n <= 9 {
                return Some(LogicalKey::Numpad(n));
            }
        }
        return match np {
            "add" | "plus" => Some(LogicalKey::NumpadAdd),
            "subtract" | "minus" => Some(LogicalKey::NumpadSubtract),
            "multiply" => Some(LogicalKey::NumpadMultiply),
            "divide" => Some(LogicalKey::NumpadDivide),
            "decimal" => Some(LogicalKey::NumpadDecimal),
            "enter" => Some(LogicalKey::NumpadEnter),
            _ => None,
        };
    }

    match lower.as_str() {
        "space" | "spacebar" => Some(LogicalKey::Space),
        "esc" | "escape" => Some(LogicalKey::Escape),
        "enter" | "return" => Some(LogicalKey::Enter),
        "tab" => Some(LogicalKey::Tab),
        "backspace" => Some(LogicalKey::Backspace),
        "delete" | "del" => Some(LogicalKey::Delete),
        "insert" | "ins" => Some(LogicalKey::Insert),
        "home" => Some(LogicalKey::Home),
        "end" => Some(LogicalKey::End),
        "pageup" | "pgup" => Some(LogicalKey::PageUp),
        "pagedown" | "pgdn" => Some(LogicalKey::PageDown),
        "up" | "arrowup" => Some(LogicalKey::Up),
        "down" | "arrowdown" => Some(LogicalKey::Down),
        "left" | "arrowleft" => Some(LogicalKey::Left),
        "right" | "arrowright" => Some(LogicalKey::Right),
        "capslock" => Some(LogicalKey::CapsLock),
        "pause" | "pausebreak" => Some(LogicalKey::Pause),
        "printscreen" | "prtsc" => Some(LogicalKey::PrintScreen),
        "minus" => Some(LogicalKey::Minus),
        "equal" => Some(LogicalKey::Equal),
        "bracketleft" => Some(LogicalKey::BracketLeft),
        "bracketright" => Some(LogicalKey::BracketRight),
        "backslash" => Some(LogicalKey::Backslash),
        "semicolon" => Some(LogicalKey::Semicolon),
        "quote" => Some(LogicalKey::Quote),
        "comma" => Some(LogicalKey::Comma),
        "period" => Some(LogicalKey::Period),
        "slash" => Some(LogicalKey::Slash),
        "backquote" => Some(LogicalKey::Backquote),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hotkey_handles_modifiers_functions_numpad_and_punctuation() {
        assert_eq!(
            parse_hotkey("Control+Shift+X"),
            Some(HotkeyCombo {
                ctrl: true,
                shift: true,
                alt: false,
                super_key: false,
                key: LogicalKey::Char('X'),
            })
        );
        assert_eq!(
            parse_hotkey("F12"),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: false,
                super_key: false,
                key: LogicalKey::F(12),
            })
        );
        assert_eq!(
            parse_hotkey("Alt+Numpad5"),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: true,
                super_key: false,
                key: LogicalKey::Numpad(5),
            })
        );
        assert_eq!(
            parse_hotkey("Space"),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: false,
                super_key: false,
                key: LogicalKey::Space,
            })
        );
        assert!(parse_hotkey("").is_none());
        assert!(parse_hotkey("InvalidKeyName123").is_none());
        assert_eq!(
            parse_hotkey("Control+Shift+Equal"),
            Some(HotkeyCombo {
                ctrl: true,
                shift: true,
                alt: false,
                super_key: false,
                key: LogicalKey::Equal,
            })
        );
        assert_eq!(
            parse_hotkey("Control++"),
            Some(HotkeyCombo {
                ctrl: true,
                shift: false,
                alt: false,
                super_key: false,
                key: LogicalKey::Equal,
            })
        );
    }
}
