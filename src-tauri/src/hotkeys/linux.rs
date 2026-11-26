use super::parser::{HotkeyAction, HotkeyBinding, HotkeyCombo, LogicalKey};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use x11rb::connection::Connection;
use x11rb::protocol::Event as XEvent;
use x11rb::protocol::xinput::{self, ConnectionExt as _, XIEventMask};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use xkbcommon::xkb::{self, keysyms};

pub fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

#[derive(Default)]
struct ModifierTracker {
    ctrl_l: bool,
    ctrl_r: bool,
    shift_l: bool,
    shift_r: bool,
    alt_l: bool,
    alt_r: bool,
    super_l: bool,
    super_r: bool,
    pressed_keycodes: [bool; 256],
}

impl ModifierTracker {
    fn ctrl(&self) -> bool {
        self.ctrl_l || self.ctrl_r
    }
    fn shift(&self) -> bool {
        self.shift_l || self.shift_r
    }
    fn alt(&self) -> bool {
        self.alt_l || self.alt_r
    }
    fn super_key(&self) -> bool {
        self.super_l || self.super_r
    }

    fn handle_key(
        &mut self,
        keycode: u32,
        sym: u32,
        is_press: bool,
    ) -> Option<HotkeyCombo> {
        match sym {
            keysyms::KEY_Control_L => {
                self.ctrl_l = is_press;
                return None;
            }
            keysyms::KEY_Control_R => {
                self.ctrl_r = is_press;
                return None;
            }
            keysyms::KEY_Shift_L => {
                self.shift_l = is_press;
                return None;
            }
            keysyms::KEY_Shift_R => {
                self.shift_r = is_press;
                return None;
            }
            keysyms::KEY_Alt_L => {
                self.alt_l = is_press;
                return None;
            }
            keysyms::KEY_Alt_R | keysyms::KEY_ISO_Level3_Shift => {
                self.alt_r = is_press;
                return None;
            }
            keysyms::KEY_Super_L | keysyms::KEY_Meta_L => {
                self.super_l = is_press;
                return None;
            }
            keysyms::KEY_Super_R | keysyms::KEY_Meta_R => {
                self.super_r = is_press;
                return None;
            }
            _ => {}
        }

        if let Some(slot) = self.pressed_keycodes.get_mut(keycode as usize) {
            if !is_press {
                *slot = false;
                return None;
            }
            if *slot {
                return None;
            }
            *slot = true;
        } else if !is_press {
            return None;
        }

        let logical = keysym_to_logical(sym)?;
        Some(HotkeyCombo {
            ctrl: self.ctrl(),
            shift: self.shift(),
            alt: self.alt(),
            super_key: self.super_key(),
            key: logical,
        })
    }
}

fn keysym_to_logical(sym: u32) -> Option<LogicalKey> {
    match sym {
        0x0041..=0x005A => Some(LogicalKey::Char((sym as u8) as char)),
        0x0061..=0x007A => Some(LogicalKey::Char(((sym as u8) as char).to_ascii_uppercase())),
        0x0030..=0x0039 => Some(LogicalKey::Digit((sym as u8) - b'0')),
        s if (keysyms::KEY_F1..=keysyms::KEY_F24).contains(&s) => {
            Some(LogicalKey::F((s - keysyms::KEY_F1 + 1) as u8))
        }
        s if (keysyms::KEY_KP_0..=keysyms::KEY_KP_9).contains(&s) => {
            Some(LogicalKey::Numpad((s - keysyms::KEY_KP_0) as u8))
        }
        keysyms::KEY_KP_Add => Some(LogicalKey::NumpadAdd),
        keysyms::KEY_KP_Subtract => Some(LogicalKey::NumpadSubtract),
        keysyms::KEY_KP_Multiply => Some(LogicalKey::NumpadMultiply),
        keysyms::KEY_KP_Divide => Some(LogicalKey::NumpadDivide),
        keysyms::KEY_KP_Decimal | keysyms::KEY_KP_Delete => Some(LogicalKey::NumpadDecimal),
        keysyms::KEY_KP_Enter => Some(LogicalKey::NumpadEnter),
        keysyms::KEY_KP_Insert => Some(LogicalKey::Numpad(0)),
        keysyms::KEY_KP_End => Some(LogicalKey::Numpad(1)),
        keysyms::KEY_KP_Down => Some(LogicalKey::Numpad(2)),
        keysyms::KEY_KP_Page_Down => Some(LogicalKey::Numpad(3)),
        keysyms::KEY_KP_Left => Some(LogicalKey::Numpad(4)),
        keysyms::KEY_KP_Begin => Some(LogicalKey::Numpad(5)),
        keysyms::KEY_KP_Right => Some(LogicalKey::Numpad(6)),
        keysyms::KEY_KP_Home => Some(LogicalKey::Numpad(7)),
        keysyms::KEY_KP_Up => Some(LogicalKey::Numpad(8)),
        keysyms::KEY_KP_Page_Up => Some(LogicalKey::Numpad(9)),
        keysyms::KEY_space => Some(LogicalKey::Space),
        keysyms::KEY_Escape => Some(LogicalKey::Escape),
        keysyms::KEY_Return => Some(LogicalKey::Enter),
        keysyms::KEY_Tab | keysyms::KEY_ISO_Left_Tab => Some(LogicalKey::Tab),
        keysyms::KEY_BackSpace => Some(LogicalKey::Backspace),
        keysyms::KEY_Delete => Some(LogicalKey::Delete),
        keysyms::KEY_Insert => Some(LogicalKey::Insert),
        keysyms::KEY_Home => Some(LogicalKey::Home),
        keysyms::KEY_End => Some(LogicalKey::End),
        keysyms::KEY_Prior => Some(LogicalKey::PageUp),
        keysyms::KEY_Next => Some(LogicalKey::PageDown),
        keysyms::KEY_Up => Some(LogicalKey::Up),
        keysyms::KEY_Down => Some(LogicalKey::Down),
        keysyms::KEY_Left => Some(LogicalKey::Left),
        keysyms::KEY_Right => Some(LogicalKey::Right),
        keysyms::KEY_Caps_Lock => Some(LogicalKey::CapsLock),
        keysyms::KEY_Pause => Some(LogicalKey::Pause),
        keysyms::KEY_Print | keysyms::KEY_Sys_Req => Some(LogicalKey::PrintScreen),
        keysyms::KEY_minus | keysyms::KEY_underscore => Some(LogicalKey::Minus),
        keysyms::KEY_equal | keysyms::KEY_plus => Some(LogicalKey::Equal),
        keysyms::KEY_bracketleft | keysyms::KEY_braceleft => Some(LogicalKey::BracketLeft),
        keysyms::KEY_bracketright | keysyms::KEY_braceright => Some(LogicalKey::BracketRight),
        keysyms::KEY_backslash | keysyms::KEY_bar => Some(LogicalKey::Backslash),
        keysyms::KEY_semicolon | keysyms::KEY_colon => Some(LogicalKey::Semicolon),
        keysyms::KEY_apostrophe | keysyms::KEY_quotedbl => Some(LogicalKey::Quote),
        keysyms::KEY_comma | keysyms::KEY_less => Some(LogicalKey::Comma),
        keysyms::KEY_period | keysyms::KEY_greater => Some(LogicalKey::Period),
        keysyms::KEY_slash | keysyms::KEY_question => Some(LogicalKey::Slash),
        keysyms::KEY_grave | keysyms::KEY_asciitilde => Some(LogicalKey::Backquote),
        _ => None,
    }
}

fn read_xkb_rmlvo<C: Connection>(conn: &C, root: xproto::Window) -> (String, String, String, String, Option<String>) {
    let Ok(atom_cookie) = conn.intern_atom(true, b"_XKB_RULES_NAMES") else {
        return ("".into(), "".into(), "".into(), "".into(), None);
    };
    let Ok(atom_reply) = atom_cookie.reply() else {
        return ("".into(), "".into(), "".into(), "".into(), None);
    };
    if atom_reply.atom == x11rb::NONE {
        return ("".into(), "".into(), "".into(), "".into(), None);
    }
    let Ok(prop_cookie) = conn.get_property(false, root, atom_reply.atom, xproto::AtomEnum::STRING, 0, 1024) else {
        return ("".into(), "".into(), "".into(), "".into(), None);
    };
    let Ok(prop) = prop_cookie.reply() else {
        return ("".into(), "".into(), "".into(), "".into(), None);
    };
    let parts: Vec<String> = prop
        .value
        .split(|&b| b == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    let rules = parts.first().cloned().unwrap_or_default();
    let model = parts.get(1).cloned().unwrap_or_default();
    let layout = parts.get(2).cloned().unwrap_or_default();
    let variant = parts.get(3).cloned().unwrap_or_default();
    let options = parts.get(4).filter(|s| !s.is_empty()).cloned();
    (rules, model, layout, variant, options)
}

pub struct LinuxHotkeyBackend {
    stop_flag: Arc<AtomicBool>,
    x11_wake_win: Arc<AtomicU32>,
    x11_conn: Arc<Mutex<Option<Arc<x11rb::rust_connection::RustConnection>>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    portal_tx: Mutex<Option<Sender<Vec<HotkeyBinding>>>>,
}

impl LinuxHotkeyBackend {
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            x11_wake_win: Arc::new(AtomicU32::new(0)),
            x11_conn: Arc::new(Mutex::new(None)),
            thread: Mutex::new(None),
            portal_tx: Mutex::new(None),
        }
    }

    pub fn start<F>(
        &self,
        bindings: Arc<Mutex<Vec<HotkeyBinding>>>,
        paused: Arc<AtomicBool>,
        on_action: F,
    ) where
        F: Fn(HotkeyAction) + Send + 'static,
    {
        if self.thread.lock().is_some() {
            return;
        }
        self.stop_flag.store(false, Ordering::Release);

        let (action_tx, action_rx) = mpsc::channel::<HotkeyAction>();
        std::thread::Builder::new()
            .name("memeboard-hotkey-dispatch".to_string())
            .spawn(move || {
                while let Ok(action) = action_rx.recv() {
                    on_action(action);
                }
            })
            .ok();

        if is_wayland_session() {
            let (ptx, prx) = mpsc::channel::<Vec<HotkeyBinding>>();
            *self.portal_tx.lock() = Some(ptx.clone());
            let initial = bindings.lock().clone();
            if !initial.is_empty() {
                let _ = ptx.send(initial);
            }
            let stop = Arc::clone(&self.stop_flag);
            let paused_w = Arc::clone(&paused);
            let bindings_w = Arc::clone(&bindings);
            let wake_win = Arc::clone(&self.x11_wake_win);
            let conn_slot = Arc::clone(&self.x11_conn);

            let handle = std::thread::Builder::new()
                .name("memeboard-wayland-hotkeys".to_string())
                .spawn(move || {
                    let portal_ok = run_wayland_portal_loop(prx, paused_w.clone(), action_tx.clone(), stop.clone());
                    if !portal_ok && !stop.load(Ordering::Acquire) && std::env::var_os("DISPLAY").is_some() {
                        eprintln!("[Memeboard] Wayland GlobalShortcuts unavailable; trying XWayland XInput2 fallback.");
                        run_x11_loop(bindings_w, paused_w, action_tx, stop, wake_win, conn_slot);
                    }
                })
                .ok();
            *self.thread.lock() = handle;
        } else {
            let stop = Arc::clone(&self.stop_flag);
            let wake_win = Arc::clone(&self.x11_wake_win);
            let conn_slot = Arc::clone(&self.x11_conn);
            let handle = std::thread::Builder::new()
                .name("memeboard-x11-hotkeys".to_string())
                .spawn(move || {
                    run_x11_loop(bindings, paused, action_tx, stop, wake_win, conn_slot);
                })
                .ok();
            *self.thread.lock() = handle;
        }
    }

    pub fn update_bindings(&self, bindings: &[HotkeyBinding]) {
        if let Some(tx) = self.portal_tx.lock().as_ref() {
            let _ = tx.send(bindings.to_vec());
        }
    }

    pub fn shutdown(&self) {
        self.stop_flag.store(true, Ordering::Release);
        *self.portal_tx.lock() = None;

        let win = self.x11_wake_win.swap(0, Ordering::AcqRel);
        if win != 0 {
            if let Some(conn) = self.x11_conn.lock().take() {
                let _ = conn.destroy_window(win);
                let _ = conn.flush();
            }
        }

        if let Some(handle) = self.thread.lock().take() {
            // Wayland/X11 threads exit promptly on channel drop or stop flag
            drop(handle);
        }
    }
}

fn run_x11_loop(
    bindings: Arc<Mutex<Vec<HotkeyBinding>>>,
    paused: Arc<AtomicBool>,
    action_tx: Sender<HotkeyAction>,
    stop: Arc<AtomicBool>,
    wake_win_slot: Arc<AtomicU32>,
    conn_slot: Arc<Mutex<Option<Arc<x11rb::rust_connection::RustConnection>>>>,
) {
    let Ok((conn, screen_num)) = x11rb::rust_connection::RustConnection::connect(None) else {
        eprintln!("[Memeboard] X11 hotkey listener unavailable (failed to connect to DISPLAY)");
        return;
    };
    let conn = Arc::new(conn);
    *conn_slot.lock() = Some(Arc::clone(&conn));

    let root = conn.setup().roots[screen_num].root;
    if xinput::xi_query_version(&*conn, 2, 0)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_none()
    {
        eprintln!("[Memeboard] XInput2 extension unavailable");
        return;
    }

    if let Ok(win) = conn.generate_id() {
        let _ = conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            win,
            root,
            0,
            0,
            1,
            1,
            0,
            xproto::WindowClass::INPUT_ONLY,
            0,
            &xproto::CreateWindowAux::new().event_mask(xproto::EventMask::STRUCTURE_NOTIFY),
        );
        wake_win_slot.store(win, Ordering::Release);
    }

    let raw_mask = XIEventMask::from(
        u32::from(XIEventMask::RAW_KEY_PRESS) | u32::from(XIEventMask::RAW_KEY_RELEASE),
    );
    let masks = [xinput::EventMask {
        deviceid: u16::from(xinput::Device::ALL_MASTER),
        mask: vec![raw_mask],
    }];
    if xinput::xi_select_events(&*conn, root, &masks).is_err() {
        eprintln!("[Memeboard] Failed to register XInput2 raw keyboard events");
        return;
    }
    let _ = conn.flush();

    let (rules, model, layout, variant, options) = read_xkb_rmlvo(&*conn, root);
    let xkb_ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(
        &xkb_ctx,
        &rules,
        &model,
        &layout,
        &variant,
        options,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .or_else(|| {
        xkb::Keymap::new_from_names(&xkb_ctx, "", "", "", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS)
    });
    let Some(keymap) = keymap else {
        eprintln!("[Memeboard] Failed to compile XKB keymap for X11 hotkeys");
        return;
    };
    let xkb_state = xkb::State::new(&keymap);
    let mut tracker = ModifierTracker::default();

    while !stop.load(Ordering::Acquire) {
        let Ok(event) = conn.wait_for_event() else {
            break;
        };
        if stop.load(Ordering::Acquire) {
            break;
        }
        match event {
            XEvent::XinputRawKeyPress(ev) => {
                let sym = xkb_state.key_get_one_sym(xkb::Keycode::new(ev.detail)).raw();
                if let Some(combo) = tracker.handle_key(ev.detail, sym, true) {
                    if !paused.load(Ordering::Acquire) {
                        let guard = bindings.lock();
                        for b in guard.iter() {
                            if b.combo == combo {
                                let _ = action_tx.send(b.action.clone());
                            }
                        }
                    }
                }
            }
            XEvent::XinputRawKeyRelease(ev) => {
                let sym = xkb_state.key_get_one_sym(xkb::Keycode::new(ev.detail)).raw();
                let _ = tracker.handle_key(ev.detail, sym, false);
            }
            XEvent::DestroyNotify(_) => break,
            _ => {}
        }
    }
}

fn binding_to_portal_id(action: &HotkeyAction) -> String {
    match action {
        HotkeyAction::Panic => "panic".to_string(),
        HotkeyAction::ToggleOverlay => "overlay".to_string(),
        HotkeyAction::TriggerSound(id) => format!("sound:{id}"),
        HotkeyAction::ShowMainWindow => "show".to_string(),
    }
}

fn portal_id_to_action(id: &str) -> Option<HotkeyAction> {
    if id == "panic" {
        Some(HotkeyAction::Panic)
    } else if id == "overlay" {
        Some(HotkeyAction::ToggleOverlay)
    } else if let Some(sound_id) = id.strip_prefix("sound:") {
        Some(HotkeyAction::TriggerSound(sound_id.to_string()))
    } else {
        None
    }
}

fn run_wayland_portal_loop(
    rx: mpsc::Receiver<Vec<HotkeyBinding>>,
    paused: Arc<AtomicBool>,
    action_tx: Sender<HotkeyAction>,
    stop: Arc<AtomicBool>,
) -> bool {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let (ready_tx, ready_rx) = mpsc::channel::<bool>();

    tauri::async_runtime::spawn(async move {
        let Ok(portal) = GlobalShortcuts::new().await else {
            let _ = ready_tx.send(false);
            return;
        };
        let Ok(session) = portal.create_session().await else {
            let _ = ready_tx.send(false);
            return;
        };
        let Ok(mut activated) = portal.receive_activated().await else {
            let _ = ready_tx.send(false);
            return;
        };
        let _ = ready_tx.send(true);

        let mut last_signature: Vec<(String, String)> = Vec::new();
        loop {
            while let Ok(mut next) = rx.try_recv() {
                // Debounce rapid UI saves
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                while let Ok(newer) = rx.try_recv() {
                    next = newer;
                }
                let sig: Vec<(String, String)> = next
                    .iter()
                    .map(|b| (binding_to_portal_id(&b.action), b.combo.to_portal_trigger()))
                    .collect();
                if sig != last_signature {
                    let shortcuts: Vec<NewShortcut> = next
                        .iter()
                        .map(|b| {
                            let id = binding_to_portal_id(&b.action);
                            let desc = match &b.action {
                                HotkeyAction::Panic => "Memeboard: Panic Stop All".to_string(),
                                HotkeyAction::ToggleOverlay => "Memeboard: Toggle Overlay".to_string(),
                                HotkeyAction::TriggerSound(sid) => format!("Memeboard Sound ({sid})"),
                                HotkeyAction::ShowMainWindow => "Memeboard: Open Window".to_string(),
                            };
                            let trigger = b.combo.to_portal_trigger();
                            NewShortcut::new(id, desc).preferred_trigger(trigger.as_str())
                        })
                        .collect();
                    if !shortcuts.is_empty() {
                        if let Ok(req) = portal.bind_shortcuts(&session, &shortcuts, None).await {
                            let _ = req.response();
                        }
                    }
                    last_signature = sig;
                }
            }

            if stop.load(Ordering::Acquire) {
                break;
            }

            tokio::select! {
                ev = activated.next() => {
                    match ev {
                        Some(act) => {
                            if !paused.load(Ordering::Acquire) {
                                if let Some(action) = portal_id_to_action(act.shortcut_id()) {
                                    let _ = action_tx.send(action);
                                }
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
            }
        }
    });

    ready_rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap_or(false)
}
