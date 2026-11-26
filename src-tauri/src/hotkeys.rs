use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RAWKEYBOARD, RID_INPUT,
    RIDEV_INPUTSINK, RIM_TYPEKEYBOARD,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, FindWindowExW, GetMessageW, HWND_MESSAGE, MSG, PostMessageW, RegisterClassW,
    TranslateMessage, WM_APP, WM_INPUT, WM_QUIT, WNDCLASSW,
};

pub const HOTKEY_WINDOW_CLASS: &str = "MemeboardRawInputHotkeyWindow";
pub const WM_SHOW_MAIN_WINDOW: u32 = WM_APP + 42;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
    pub vkey: u16,
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

#[derive(Debug, Default, Clone)]
struct KeyboardState {
    ctrl_left: bool,
    ctrl_right: bool,
    ctrl_generic: bool,
    shift_left: bool,
    shift_right: bool,
    shift_generic: bool,
    alt_left: bool,
    alt_right: bool,
    alt_generic: bool,
    win_left: bool,
    win_right: bool,
    pressed_vkeys: Vec<bool>,
}

impl KeyboardState {
    fn new() -> Self {
        Self {
            pressed_vkeys: vec![false; 256],
            ..Default::default()
        }
    }

    fn ctrl(&self) -> bool {
        self.ctrl_left || self.ctrl_right || self.ctrl_generic
    }

    fn shift(&self) -> bool {
        self.shift_left || self.shift_right || self.shift_generic
    }

    fn alt(&self) -> bool {
        self.alt_left || self.alt_right || self.alt_generic
    }

    fn super_key(&self) -> bool {
        self.win_left || self.win_right
    }

    fn sync_modifiers(&mut self, ctrl: bool, shift: bool, alt: bool, lwin: bool, rwin: bool) {
        if !ctrl {
            self.ctrl_left = false;
            self.ctrl_right = false;
            self.ctrl_generic = false;
        }
        if !shift {
            self.shift_left = false;
            self.shift_right = false;
            self.shift_generic = false;
        }
        if !alt {
            self.alt_left = false;
            self.alt_right = false;
            self.alt_generic = false;
        }
        if !lwin {
            self.win_left = false;
        }
        if !rwin {
            self.win_right = false;
        }
    }

    /// Processes one Raw Input keyboard event and returns Some(HotkeyCombo) only on initial
    /// non-repeat primary key press (never on modifier-only presses, repeats, or releases).
    fn process_event(&mut self, vkey: u16, make_code: u16, is_break: bool, is_e0: bool) -> Option<HotkeyCombo> {
        if vkey == 0 || vkey == 0xFF || vkey as usize >= 256 {
            return None;
        }

        let down = !is_break;
        let resolved_vk = resolve_vkey(vkey, make_code, is_e0);

        match resolved_vk {
            0xA2 => self.ctrl_left = down,    // VK_LCONTROL
            0xA3 => self.ctrl_right = down,   // VK_RCONTROL
            0x11 => self.ctrl_generic = down, // VK_CONTROL
            0xA0 => self.shift_left = down,   // VK_LSHIFT
            0xA1 => self.shift_right = down,  // VK_RSHIFT
            0x10 => self.shift_generic = down,// VK_SHIFT
            0xA4 => self.alt_left = down,     // VK_LMENU
            0xA5 => self.alt_right = down,    // VK_RMENU
            0x12 => self.alt_generic = down,  // VK_MENU
            0x5B => self.win_left = down,     // VK_LWIN
            0x5C => self.win_right = down,    // VK_RWIN
            _ => {}
        }

        let idx = resolved_vk as usize;
        if is_break {
            self.pressed_vkeys[idx] = false;
            return None;
        }

        // Ignore key auto-repeat while key is held down
        if self.pressed_vkeys[idx] {
            return None;
        }
        self.pressed_vkeys[idx] = true;

        if is_modifier_vkey(resolved_vk) {
            return None;
        }

        Some(HotkeyCombo {
            ctrl: self.ctrl(),
            shift: self.shift(),
            alt: self.alt(),
            super_key: self.super_key(),
            vkey: resolved_vk,
        })
    }
}

#[inline]
fn is_modifier_vkey(vk: u16) -> bool {
    matches!(
        vk,
        0x10 | 0x11 | 0x12 | 0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5 | 0x5B | 0x5C
    )
}

#[inline]
fn resolve_vkey(vkey: u16, make_code: u16, is_e0: bool) -> u16 {
    match vkey {
        0x11 => {
            // VK_CONTROL
            if is_e0 { 0xA3 } else { 0xA2 }
        }
        0x12 => {
            // VK_MENU
            if is_e0 { 0xA5 } else { 0xA4 }
        }
        0x10 => {
            // VK_SHIFT (distinguish via scan code 0x36 = RSHIFT)
            if make_code == 0x36 { 0xA1 } else { 0xA0 }
        }
        other => other,
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
    let mut primary_vk: Option<u16> = None;

    // Handle literal trailing '+' key (e.g. "+" or "Control+Shift++")
    let (body, trailing_plus) = if trimmed == "+" {
        ("", true)
    } else if let Some(prefix) = trimmed.strip_suffix("++") {
        (prefix, true)
    } else {
        (trimmed, false)
    };
    if trailing_plus {
        primary_vk = Some(VK_OEM_PLUS);
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
                primary_vk = parse_primary_vkey(part);
                if primary_vk.is_none() {
                    return None;
                }
            }
        }
    }

    primary_vk.map(|vkey| HotkeyCombo {
        ctrl,
        shift,
        alt,
        super_key,
        vkey,
    })
}

fn parse_primary_vkey(token: &str) -> Option<u16> {
    let t = token.trim();
    if t.len() == 1 {
        let ch = t.chars().next()?.to_ascii_uppercase();
        return match ch {
            'A'..='Z' => Some(ch as u16),
            '0'..='9' => Some(ch as u16),
            '-' | '_' => Some(VK_OEM_MINUS),
            '=' | '+' => Some(VK_OEM_PLUS),
            '[' | '{' => Some(VK_OEM_4),
            ']' | '}' => Some(VK_OEM_6),
            '\\' | '|' => Some(VK_OEM_5),
            ';' | ':' => Some(VK_OEM_1),
            '\'' | '"' => Some(VK_OEM_7),
            ',' | '<' => Some(VK_OEM_COMMA),
            '.' | '>' => Some(VK_OEM_PERIOD),
            '/' | '?' => Some(VK_OEM_2),
            '`' | '~' => Some(VK_OEM_3),
            '!' => Some(b'1' as u16),
            '@' => Some(b'2' as u16),
            '#' => Some(b'3' as u16),
            '$' => Some(b'4' as u16),
            '%' => Some(b'5' as u16),
            '^' => Some(b'6' as u16),
            '&' => Some(b'7' as u16),
            '*' => Some(b'8' as u16),
            '(' => Some(b'9' as u16),
            ')' => Some(b'0' as u16),
            _ => None,
        };
    }

    let lower = t.to_ascii_lowercase();
    if let Some(num) = lower.strip_prefix('f') {
        if let Ok(n) = num.parse::<u16>() {
            if (1..=24).contains(&n) {
                return Some(VK_F1 + (n - 1));
            }
        }
    }
    if let Some(np) = lower.strip_prefix("numpad") {
        if let Ok(n) = np.parse::<u16>() {
            if n <= 9 {
                return Some(VK_NUMPAD0 + n);
            }
        }
        return match np {
            "add" | "plus" => Some(VK_ADD),
            "subtract" | "minus" => Some(VK_SUBTRACT),
            "multiply" => Some(VK_MULTIPLY),
            "divide" => Some(VK_DIVIDE),
            "decimal" => Some(VK_DECIMAL),
            "enter" => Some(VK_RETURN),
            _ => None,
        };
    }

    match lower.as_str() {
        "space" | "spacebar" => Some(VK_SPACE),
        "esc" | "escape" => Some(VK_ESCAPE),
        "enter" | "return" => Some(VK_RETURN),
        "tab" => Some(VK_TAB),
        "backspace" => Some(VK_BACK),
        "delete" | "del" => Some(VK_DELETE),
        "insert" | "ins" => Some(VK_INSERT),
        "home" => Some(VK_HOME),
        "end" => Some(VK_END),
        "pageup" | "pgup" => Some(VK_PRIOR),
        "pagedown" | "pgdn" => Some(VK_NEXT),
        "up" | "arrowup" => Some(VK_UP),
        "down" | "arrowdown" => Some(VK_DOWN),
        "left" | "arrowleft" => Some(VK_LEFT),
        "right" | "arrowright" => Some(VK_RIGHT),
        "capslock" => Some(VK_CAPITAL),
        "pause" | "pausebreak" => Some(VK_PAUSE),
        "printscreen" | "prtsc" => Some(VK_SNAPSHOT),
        "minus" => Some(VK_OEM_MINUS),
        "equal" => Some(VK_OEM_PLUS),
        "bracketleft" => Some(VK_OEM_4),
        "bracketright" => Some(VK_OEM_6),
        "backslash" => Some(VK_OEM_5),
        "semicolon" => Some(VK_OEM_1),
        "quote" => Some(VK_OEM_7),
        "comma" => Some(VK_OEM_COMMA),
        "period" => Some(VK_OEM_PERIOD),
        "slash" => Some(VK_OEM_2),
        "backquote" => Some(VK_OEM_3),
        _ => None,
    }
}

pub struct HotkeyManager {
    bindings: Arc<Mutex<Vec<HotkeyBinding>>>,
    recording_paused: Arc<AtomicBool>,
    hwnd_raw: Arc<AtomicIsize>,
    pump_thread: Mutex<Option<JoinHandle<()>>>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(Mutex::new(Vec::new())),
            recording_paused: Arc::new(AtomicBool::new(false)),
            hwnd_raw: Arc::new(AtomicIsize::new(0)),
            pump_thread: Mutex::new(None),
        }
    }

    pub fn start<F>(&self, on_action: F)
    where
        F: Fn(HotkeyAction) + Send + 'static,
    {
        if self.pump_thread.lock().is_some() {
            return;
        }

        let (action_tx, action_rx) = mpsc::channel::<HotkeyAction>();
        std::thread::Builder::new()
            .name("memeboard-hotkey-dispatch".to_string())
            .spawn(move || {
                while let Ok(action) = action_rx.recv() {
                    on_action(action);
                }
            })
            .ok();

        let bindings = Arc::clone(&self.bindings);
        let paused = Arc::clone(&self.recording_paused);
        let hwnd_raw = Arc::clone(&self.hwnd_raw);

        let handle = std::thread::Builder::new()
            .name("memeboard-rawinput-hotkeys".to_string())
            .spawn(move || {
                run_raw_input_loop(bindings, paused, hwnd_raw, action_tx);
            })
            .ok();

        *self.pump_thread.lock() = handle;
    }

    pub fn update_bindings(
        &self,
        panic_hotkey: &str,
        overlay_hotkey: &str,
        sound_hotkeys: &[(String, String)],
    ) {
        let mut next = Vec::with_capacity(sound_hotkeys.len() + 2);
        if let Some(combo) = parse_hotkey(panic_hotkey) {
            next.push(HotkeyBinding {
                combo,
                action: HotkeyAction::Panic,
            });
        }
        if let Some(combo) = parse_hotkey(overlay_hotkey) {
            next.push(HotkeyBinding {
                combo,
                action: HotkeyAction::ToggleOverlay,
            });
        }
        for (sound_id, hk) in sound_hotkeys {
            if let Some(combo) = parse_hotkey(hk) {
                next.push(HotkeyBinding {
                    combo,
                    action: HotkeyAction::TriggerSound(sound_id.clone()),
                });
            }
        }
        *self.bindings.lock() = next;
    }

    pub fn set_recording_paused(&self, paused: bool) {
        self.recording_paused.store(paused, Ordering::Release);
    }

    pub fn shutdown(&self) {
        let raw = self.hwnd_raw.swap(0, Ordering::AcqRel);
        if raw != 0 {
            unsafe {
                PostMessageW(raw as HWND, WM_QUIT, 0, 0);
            }
        }
        if let Some(handle) = self.pump_thread.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct LoopContext {
    bindings: Arc<Mutex<Vec<HotkeyBinding>>>,
    paused: Arc<AtomicBool>,
    action_tx: Sender<HotkeyAction>,
    kb_state: KeyboardState,
}

thread_local! {
    static LOOP_CTX: std::cell::RefCell<Option<LoopContext>> = const { std::cell::RefCell::new(None) };
}

unsafe fn parse_raw_keyboard_bytes(buf: &[u8]) -> Option<RAWKEYBOARD> {
    let header_size = std::mem::size_of::<RAWINPUTHEADER>();
    let kb_size = std::mem::size_of::<RAWKEYBOARD>();
    if buf.len() < header_size + kb_size {
        return None;
    }
    let header = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<RAWINPUTHEADER>()) };
    if header.dwType != RIM_TYPEKEYBOARD {
        return None;
    }
    let kb = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(header_size).cast::<RAWKEYBOARD>()) };
    Some(kb)
}

unsafe extern "system" fn raw_input_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_SHOW_MAIN_WINDOW {
        LOOP_CTX.with(|cell| {
            if let Some(ctx) = cell.borrow().as_ref() {
                let _ = ctx.action_tx.send(HotkeyAction::ShowMainWindow);
            }
        });
        return 0;
    }

    if msg == WM_INPUT {
        unsafe {
            let mut size: u32 = 0;
            let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
            GetRawInputData(
                lparam as HRAWINPUT,
                RID_INPUT,
                std::ptr::null_mut(),
                &mut size,
                header_size,
            );
            if size > 0 && size <= 1024 {
                let mut buf = [0u8; 1024];
                let read = GetRawInputData(
                    lparam as HRAWINPUT,
                    RID_INPUT,
                    buf.as_mut_ptr().cast(),
                    &mut size,
                    header_size,
                );
                if read != u32::MAX && read > 0 && (read as usize) <= buf.len() {
                    if let Some(kb) = parse_raw_keyboard_bytes(&buf[..read as usize]) {
                        let is_break = (kb.Flags as u32 & 0x0001) != 0; // RI_KEY_BREAK
                        let is_e0 = (kb.Flags as u32 & 0x0002) != 0; // RI_KEY_E0
                        let is_down = |vk: u16| (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0;
                        LOOP_CTX.with(|cell| {
                            if let Some(ctx) = cell.borrow_mut().as_mut() {
                                if !is_break && !is_modifier_vkey(resolve_vkey(kb.VKey, kb.MakeCode, is_e0)) {
                                    // Recover from missed key-up events across Win+L / UAC secure desktop
                                    ctx.kb_state.sync_modifiers(
                                        is_down(VK_CONTROL),
                                        is_down(VK_SHIFT),
                                        is_down(VK_MENU),
                                        is_down(VK_LWIN),
                                        is_down(VK_RWIN),
                                    );
                                }
                                if let Some(combo) =
                                    ctx.kb_state
                                        .process_event(kb.VKey, kb.MakeCode, is_break, is_e0)
                                {
                                    if !ctx.paused.load(Ordering::Acquire) {
                                        let bindings = ctx.bindings.lock();
                                        for b in bindings.iter() {
                                            if b.combo == combo {
                                                let _ = ctx.action_tx.send(b.action.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub struct SingleInstanceGuard(HANDLE);

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// Acquires single-instance GUI lock; if another Memeboard instance is already running
/// (including minimized to System Tray), activates its main window and returns `None`.
pub fn acquire_single_instance_or_activate() -> Option<SingleInstanceGuard> {
    let mutex_name = wide("Local\\MemeboardGuiSingleInstance_v1");
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if handle.is_null() {
            // Unexpected OS failure: allow startup rather than blocking the app
            return Some(SingleInstanceGuard(std::ptr::null_mut()));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            let class_name = wide(HOTKEY_WINDOW_CLASS);
            for _ in 0..10 {
                let hwnd = FindWindowExW(
                    HWND_MESSAGE,
                    std::ptr::null_mut(),
                    class_name.as_ptr(),
                    std::ptr::null(),
                );
                if !hwnd.is_null() {
                    let _ = AllowSetForegroundWindow(ASFW_ANY);
                    let _ = PostMessageW(hwnd, WM_SHOW_MAIN_WINDOW, 0, 0);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            return None;
        }
        Some(SingleInstanceGuard(handle))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn run_raw_input_loop(
    bindings: Arc<Mutex<Vec<HotkeyBinding>>>,
    paused: Arc<AtomicBool>,
    hwnd_raw: Arc<AtomicIsize>,
    action_tx: Sender<HotkeyAction>,
) {
    LOOP_CTX.with(|cell| {
        *cell.borrow_mut() = Some(LoopContext {
            bindings,
            paused,
            action_tx,
            kb_state: KeyboardState::new(),
        });
    });

    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = wide(HOTKEY_WINDOW_CLASS);
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(raw_input_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null_mut(),
        );

        if hwnd.is_null() {
            eprintln!("[Memeboard] Failed to create Raw Input hotkey listener window");
            LOOP_CTX.with(|cell| *cell.borrow_mut() = None);
            return;
        }

        let rid = RAWINPUTDEVICE {
            usUsagePage: 0x01, // Generic Desktop Controls
            usUsage: 0x06,     // Keyboard
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        if windows_sys::Win32::UI::Input::RegisterRawInputDevices(
            &rid,
            1,
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        ) == 0
        {
            eprintln!("[Memeboard] Warning: RegisterRawInputDevices failed");
            DestroyWindow(hwnd);
            LOOP_CTX.with(|cell| *cell.borrow_mut() = None);
            return;
        }

        hwnd_raw.store(hwnd as isize, Ordering::Release);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        hwnd_raw.store(0, Ordering::Release);
        DestroyWindow(hwnd);
    }

    LOOP_CTX.with(|cell| *cell.borrow_mut() = None);
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
                vkey: b'X' as u16,
            })
        );
        assert_eq!(
            parse_hotkey("F12"),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: false,
                super_key: false,
                vkey: VK_F12,
            })
        );
        assert_eq!(
            parse_hotkey("Alt+Numpad5"),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: true,
                super_key: false,
                vkey: VK_NUMPAD5,
            })
        );
        assert_eq!(
            parse_hotkey("Space"),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: false,
                super_key: false,
                vkey: VK_SPACE,
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
                vkey: VK_OEM_PLUS,
            })
        );
        assert_eq!(
            parse_hotkey("Control++"),
            Some(HotkeyCombo {
                ctrl: true,
                shift: false,
                alt: false,
                super_key: false,
                vkey: VK_OEM_PLUS,
            })
        );
    }

    #[test]
    fn keyboard_state_suppresses_repeat_and_tracks_modifiers() {
        let mut kb = KeyboardState::new();
        // Press LControl + LShift + X
        assert!(kb.process_event(0x11, 0x1D, false, false).is_none());
        assert!(kb.process_event(0x10, 0x2A, false, false).is_none());
        let trigger = kb.process_event(b'X' as u16, 0x2D, false, false);
        assert_eq!(
            trigger,
            Some(HotkeyCombo {
                ctrl: true,
                shift: true,
                alt: false,
                super_key: false,
                vkey: b'X' as u16,
            })
        );
        // Key repeat while held must be ignored
        assert!(kb.process_event(b'X' as u16, 0x2D, false, false).is_none());
        // Key up clears state so subsequent press triggers again
        assert!(kb.process_event(b'X' as u16, 0x2D, true, false).is_none());
        assert!(kb.process_event(b'X' as u16, 0x2D, false, false).is_some());
        // Release modifiers and verify clean state
        kb.process_event(b'X' as u16, 0x2D, true, false);
        kb.process_event(0x10, 0x2A, true, false);
        kb.process_event(0x11, 0x1D, true, false);
        assert_eq!(
            kb.process_event(b'X' as u16, 0x2D, false, false),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: false,
                super_key: false,
                vkey: b'X' as u16,
            })
        );
    }

    #[test]
    fn keyboard_state_sync_modifiers_recovers_after_lock_screen() {
        let mut kb = KeyboardState::new();
        // Simulate Win key pressed before Win+L (release never received across lock screen)
        assert!(kb.process_event(VK_LWIN, 0x5B, false, true).is_none());
        assert!(kb.super_key());

        // User unlocks and presses X; OS reports Win key is no longer held
        kb.sync_modifiers(false, false, false, false, false);
        assert!(!kb.super_key());
        assert_eq!(
            kb.process_event(b'X' as u16, 0x2D, false, false),
            Some(HotkeyCombo {
                ctrl: false,
                shift: false,
                alt: false,
                super_key: false,
                vkey: b'X' as u16,
            })
        );
    }

    #[test]
    fn raw_input_keyboard_payload_decodes_from_40_byte_win64_packet() {
        let header_size = std::mem::size_of::<RAWINPUTHEADER>();
        let kb_size = std::mem::size_of::<RAWKEYBOARD>();
        let packet_len = header_size + kb_size;
        assert!(packet_len <= 40, "Win64 keyboard RAWINPUT is 40 bytes, got {packet_len}");

        let mut buf = vec![0u8; packet_len];
        let header = RAWINPUTHEADER {
            dwType: RIM_TYPEKEYBOARD,
            dwSize: packet_len as u32,
            hDevice: std::ptr::null_mut(),
            wParam: 0,
        };
        let kb = RAWKEYBOARD {
            MakeCode: 0x1F,
            Flags: 0,
            Reserved: 0,
            VKey: b'S' as u16,
            Message: 0x0100,
            ExtraInformation: 0,
        };
        unsafe {
            std::ptr::write_unaligned(buf.as_mut_ptr().cast::<RAWINPUTHEADER>(), header);
            std::ptr::write_unaligned(buf.as_mut_ptr().add(header_size).cast::<RAWKEYBOARD>(), kb);
        }
        let parsed = unsafe { parse_raw_keyboard_bytes(&buf) }.expect("40-byte packet must parse");
        assert_eq!(parsed.VKey, b'S' as u16);
        assert_eq!(parsed.MakeCode, 0x1F);
    }
}
