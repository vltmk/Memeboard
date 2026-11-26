pub mod parser;

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

pub use parser::{HotkeyAction, HotkeyBinding, HotkeyCombo, LogicalKey, parse_hotkey};

use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct HotkeyManager {
    bindings: Arc<Mutex<Vec<HotkeyBinding>>>,
    recording_paused: Arc<AtomicBool>,
    #[cfg(windows)]
    backend: windows::WindowsHotkeyBackend,
    #[cfg(target_os = "linux")]
    backend: linux::LinuxHotkeyBackend,
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(Mutex::new(Vec::new())),
            recording_paused: Arc::new(AtomicBool::new(false)),
            #[cfg(windows)]
            backend: windows::WindowsHotkeyBackend::new(),
            #[cfg(target_os = "linux")]
            backend: linux::LinuxHotkeyBackend::new(),
        }
    }

    pub fn start<F>(&self, on_action: F)
    where
        F: Fn(HotkeyAction) + Send + 'static,
    {
        #[cfg(windows)]
        self.backend
            .start(Arc::clone(&self.bindings), Arc::clone(&self.recording_paused), on_action);

        #[cfg(target_os = "linux")]
        self.backend
            .start(Arc::clone(&self.bindings), Arc::clone(&self.recording_paused), on_action);

        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = on_action;
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
        #[cfg(any(windows, target_os = "linux"))]
        self.backend.update_bindings(&next);

        *self.bindings.lock() = next;
    }

    pub fn set_recording_paused(&self, paused: bool) {
        self.recording_paused.store(paused, Ordering::Release);
    }

    pub fn shutdown(&self) {
        #[cfg(any(windows, target_os = "linux"))]
        self.backend.shutdown();
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
