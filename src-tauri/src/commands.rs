use crate::decoder::compute_normalization_gain_range;
use crate::device_manager::DeviceManager;
use crate::hotkeys::HotkeyManager;
use crate::mixer::AudioMixer;
use crate::state_manager::StateManager;
use crate::types::{
    AudioDevicesResult, AudioPlaybackStatus, SoundItem, SoundboardState,
};
use crate::virtual_mic::{VirtualMicManager, VirtualMicStatus};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub mixer: Arc<AudioMixer>,
    pub device_manager: Mutex<DeviceManager>,
    pub state_manager: Arc<StateManager>,
    pub virtual_mic: Arc<VirtualMicManager>,
    pub hotkeys: Arc<HotkeyManager>,
    pub board_state: Mutex<SoundboardState>,
    pub overlay_visible: AtomicBool,
}

pub fn sync_hotkeys_from_state(state: &AppState) {
    let guard = state.board_state.lock();
    let sound_hotkeys: Vec<(String, String)> = guard
        .sounds
        .iter()
        .filter_map(|s| {
            s.hotkey
                .as_ref()
                .map(|h| h.trim())
                .filter(|h| !h.is_empty())
                .map(|h| (s.id.clone(), h.to_string()))
        })
        .collect();
    state.hotkeys.update_bindings(
        &guard.settings.panic_hotkey,
        &guard.settings.overlay_hotkey,
        &sound_hotkeys,
    );
}

pub fn play_sound_internal(state: &AppState, sound_id: &str) -> Result<(), String> {
    let (sound, target_lufs) = {
        let guard = state.board_state.lock();
        let found = guard
            .sounds
            .iter()
            .find(|s| s.id == sound_id)
            .cloned()
            .ok_or_else(|| format!("Sound '{sound_id}' not found"))?;
        (found, guard.settings.loudness_target_lufs)
    };

    StateManager::ensure_sound_loaded(&sound, target_lufs, &state.mixer)?;
    state.mixer.trigger_sound(
        &sound.id,
        sound.trim_start,
        sound.trim_end,
        sound.volume,
        sound.playback_mode,
        sound.normalized_gain,
    );
    Ok(())
}

pub fn dock_window_bottom_right(window: &tauri::WebviewWindow, margin: i32) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, HWND_TOP, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
            SetWindowPos, SystemParametersInfoW,
        };

        if let Ok(hwnd) = window.hwnd() {
            let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
            unsafe {
                let mut work = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                if SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0) != 0
                {
                    let mut win_rect = RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    };
                    if GetWindowRect(hwnd, &mut win_rect) != 0 {
                        let w = (win_rect.right - win_rect.left).max(1);
                        let h = (win_rect.bottom - win_rect.top).max(1);
                        let x = (work.right - w - margin).max(work.left);
                        let y = (work.bottom - h - margin).max(work.top);
                        SetWindowPos(
                            hwnd,
                            HWND_TOP,
                            x,
                            y,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, margin);
    }
}

pub fn toggle_overlay_window_internal(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "Overlay window not found".to_string())?;

    let currently_visible = state.overlay_visible.load(Ordering::Acquire)
        && overlay.is_visible().unwrap_or(false);
    if currently_visible {
        overlay
            .hide()
            .map_err(|e| format!("Failed to hide overlay: {e}"))?;
        state.overlay_visible.store(false, Ordering::Release);
        let _ = app.emit("overlay-visibility", false);
        Ok(false)
    } else {
        let _ = overlay.set_focusable(false);
        let _ = overlay.set_always_on_top(true);
        dock_window_bottom_right(&overlay, 16);
        show_window_no_activate(&overlay)?;
        dock_window_bottom_right(&overlay, 16);
        state.overlay_visible.store(true, Ordering::Release);
        let _ = app.emit("overlay-visibility", true);
        Ok(true)
    }
}

fn show_window_no_activate(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowLongPtrW,
            SetWindowPos, ShowWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        };

        if let Ok(hwnd) = window.hwnd() {
            let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let next = style | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize;
                if next != style {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
                }
                SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                );
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }

    let _ = window.show();
    Ok(())
}

#[tauri::command]
pub fn dock_main_window(app: AppHandle) {
    if let Some(main_win) = app.get_webview_window("main") {
        dock_window_bottom_right(&main_win, 16);
    }
}

#[tauri::command]
pub fn get_audio_devices(state: State<'_, AppState>) -> Result<AudioDevicesResult, String> {
    let dm = state.device_manager.lock();
    dm.get_audio_devices()
}

#[tauri::command]
pub fn set_audio_devices(
    state: State<'_, AppState>,
    mic_device_id: Option<String>,
    broadcast_device_id: Option<String>,
    monitor_device_id: Option<String>,
) -> Result<(), String> {
    {
        let mut dm = state.device_manager.lock();
        dm.restart_streams(
            Arc::clone(&state.mixer),
            mic_device_id.clone(),
            broadcast_device_id.clone(),
            monitor_device_id.clone(),
        )?;
    }

    let snapshot = {
        let mut guard = state.board_state.lock();
        guard.settings.mic_device_id = mic_device_id;
        guard.settings.broadcast_device_id = broadcast_device_id;
        guard.settings.monitor_device_id = monitor_device_id;
        guard.clone()
    };
    state.state_manager.save_state(&snapshot)?;
    Ok(())
}

#[tauri::command]
pub fn play_sound(state: State<'_, AppState>, sound_id: String) -> Result<(), String> {
    play_sound_internal(&state, &sound_id)
}

#[tauri::command]
pub fn preview_sound(
    state: State<'_, AppState>,
    sound_id: String,
    trim_start: f32,
    trim_end: f32,
    volume: f32,
    seek_start: Option<f32>,
) -> Result<(), String> {
    let (sound, target_lufs) = {
        let guard = state.board_state.lock();
        let found = guard
            .sounds
            .iter()
            .find(|s| s.id == sound_id)
            .cloned()
            .ok_or_else(|| format!("Sound '{sound_id}' not found"))?;
        (found, guard.settings.loudness_target_lufs)
    };

    StateManager::ensure_sound_loaded(&sound, target_lufs, &state.mixer)?;
    let gain = state
        .mixer
        .get_loaded_audio(&sound.id)
        .map(|audio| {
            compute_normalization_gain_range(&audio.samples, trim_start, trim_end, target_lufs)
        })
        .unwrap_or(sound.normalized_gain);

    let play_start = seek_start
        .unwrap_or(trim_start)
        .max(trim_start)
        .min((trim_end - 0.01).max(trim_start));

    state
        .mixer
        .trigger_preview(&sound.id, play_start, trim_end, volume, gain);
    Ok(())
}

#[tauri::command]
pub fn stop_sound(state: State<'_, AppState>, sound_id: String) {
    state.mixer.stop_sound(&sound_id);
}

#[tauri::command]
pub fn stop_preview(state: State<'_, AppState>, sound_id: String) {
    state.mixer.stop_preview(&sound_id);
}

#[tauri::command]
pub fn stop_all_sounds(state: State<'_, AppState>) {
    state.mixer.stop_all();
}

#[tauri::command]
pub fn set_volumes(
    state: State<'_, AppState>,
    mic_volume: f32,
    monitor_volume: f32,
    broadcast_volume: f32,
    monitor_muted: bool,
    broadcast_muted: bool,
) {
    state.mixer.controls.set_volumes(
        monitor_volume,
        broadcast_volume,
        mic_volume,
        monitor_muted,
        broadcast_muted,
    );
}

#[tauri::command]
pub fn set_ducking_settings(state: State<'_, AppState>, enabled: bool, ducking_db: f32) {
    state.mixer.controls.set_ducking(enabled, ducking_db);
}

#[tauri::command]
pub fn set_aec_safe_monitor(state: State<'_, AppState>, enabled: bool) {
    state.mixer.controls.set_aec_safe_monitor(enabled);
}

#[tauri::command]
pub fn get_playback_status(state: State<'_, AppState>) -> AudioPlaybackStatus {
    let (mic_stream_active, last_audio_error) = {
        let dm = state.device_manager.lock();
        (dm.is_mic_stream_active(), dm.last_error())
    };
    AudioPlaybackStatus {
        playing_sound_ids: state.mixer.get_playing_sound_ids(),
        active_clip_count: state.mixer.active_clip_count(),
        mic_active: state.mixer.controls.is_mic_active.load(Ordering::Relaxed),
        mic_stream_active,
        broadcast_stream_active: state
            .mixer
            .controls
            .broadcast_stream_active
            .load(Ordering::Relaxed),
        monitor_stream_active: state
            .mixer
            .controls
            .monitor_stream_active
            .load(Ordering::Relaxed),
        last_audio_error,
    }
}

#[tauri::command]
pub async fn import_sound_file(
    state: State<'_, AppState>,
    source_path: String,
    name: Option<String>,
    emoji: Option<String>,
    category: Option<String>,
) -> Result<SoundItem, String> {
    let target_lufs = state.board_state.lock().settings.loudness_target_lufs;
    let sm = Arc::clone(&state.state_manager);
    let mixer = Arc::clone(&state.mixer);
    tauri::async_runtime::spawn_blocking(move || {
        sm.import_sound_file(&source_path, name, emoji, category, target_lufs, &mixer)
    })
    .await
    .map_err(|e| format!("Audio import task failed: {e}"))?
}

#[tauri::command]
pub fn delete_sound(
    app: AppHandle,
    state: State<'_, AppState>,
    sound_id: String,
    file_path: String,
) -> Result<(), String> {
    state.mixer.unload_sound(&sound_id);
    state.state_manager.delete_sound_file(&file_path)?;

    let snapshot = {
        let mut guard = state.board_state.lock();
        guard.sounds.retain(|s| s.id != sound_id);
        guard.clone()
    };
    state.state_manager.save_state(&snapshot)?;
    sync_hotkeys_from_state(&state);
    let _ = app.emit("soundboard-updated", ());
    Ok(())
}

#[tauri::command]
pub fn load_soundboard_state(state: State<'_, AppState>) -> Result<SoundboardState, String> {
    Ok(state.board_state.lock().clone())
}

#[tauri::command]
pub fn save_soundboard_state(
    app: AppHandle,
    state: State<'_, AppState>,
    mut new_state: SoundboardState,
) -> Result<(), String> {
    // Recompute normalized gains if trim bounds or target LUFS changed
    let prev_sounds = { state.board_state.lock().sounds.clone() };
    let prev_lufs = { state.board_state.lock().settings.loudness_target_lufs };
    let lufs_changed = (prev_lufs - new_state.settings.loudness_target_lufs).abs() > 0.05;
    let target_lufs = new_state.settings.loudness_target_lufs;

    for sound in &mut new_state.sounds {
        let trim_changed = prev_sounds
            .iter()
            .find(|p| p.id == sound.id)
            .map_or(false, |p| {
                (p.trim_start - sound.trim_start).abs() > 0.005
                    || (p.trim_end - sound.trim_end).abs() > 0.005
            });
        if lufs_changed || trim_changed {
            if let Ok(gain) = state.state_manager.recompute_normalization(
                sound,
                sound.trim_start,
                sound.trim_end,
                target_lufs,
                &state.mixer,
            ) {
                sound.normalized_gain = gain;
            }
        }
    }

    // Apply all live DSP controls atomically
    state.mixer.controls.set_volumes(
        new_state.settings.monitor_volume,
        new_state.settings.broadcast_volume,
        new_state.settings.mic_volume,
        new_state.settings.monitor_muted,
        new_state.settings.broadcast_muted,
    );
    state.mixer.controls.set_ducking(
        new_state.settings.voice_ducking_enabled,
        new_state.settings.voice_ducking_db,
    );
    state
        .mixer
        .controls
        .set_aec_safe_monitor(new_state.settings.aec_safe_monitor);

    if new_state.settings.schema_version == 0 {
        new_state.settings.schema_version = 1;
    }

    state.state_manager.save_state(&new_state)?;
    *state.board_state.lock() = new_state;
    sync_hotkeys_from_state(&state);
    let _ = app.emit("soundboard-updated", ());
    Ok(())
}

#[tauri::command]
pub fn toggle_overlay_window(app: AppHandle) -> Result<bool, String> {
    toggle_overlay_window_internal(&app)
}

#[tauri::command]
pub fn get_overlay_visibility(state: State<'_, AppState>) -> bool {
    state.overlay_visible.load(Ordering::Acquire)
}

#[tauri::command]
pub fn set_hotkey_recording(state: State<'_, AppState>, paused: bool) {
    state.hotkeys.set_recording_paused(paused);
}

#[tauri::command]
pub fn get_virtual_mic_status(state: State<'_, AppState>) -> VirtualMicStatus {
    state.virtual_mic.status()
}

#[tauri::command]
pub async fn install_virtual_driver(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<VirtualMicStatus, String> {
    let vm = Arc::clone(&state.virtual_mic);
    let auto_switch = state.board_state.lock().settings.auto_switch_default_mic;
    let status = tauri::async_runtime::spawn_blocking(move || vm.install_driver(auto_switch))
        .await
        .map_err(|e| format!("Virtual driver setup task panicked: {e}"))??;

    // Re-bind WASAPI streams to attach Memeboard Virtual Mic cleanly
    let (mic_id, bcast_id, mon_id) = {
        let guard = state.board_state.lock();
        (
            guard.settings.mic_device_id.clone(),
            guard.settings.broadcast_device_id.clone(),
            guard.settings.monitor_device_id.clone(),
        )
    };
    {
        let mut dm = state.device_manager.lock();
        let _ = dm.restart_streams(Arc::clone(&state.mixer), mic_id, bcast_id, mon_id);
    }

    let _ = app.emit("virtual-mic-updated", &status);
    Ok(status)
}

#[tauri::command]
pub async fn set_auto_switch_mic(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<VirtualMicStatus, String> {
    let vm = Arc::clone(&state.virtual_mic);
    let sm = Arc::clone(&state.state_manager);

    tauri::async_runtime::spawn_blocking(move || vm.set_auto_switch(enabled))
        .await
        .map_err(|e| format!("Auto-switch task failed: {e}"))??;

    let snapshot = {
        let mut guard = state.board_state.lock();
        guard.settings.auto_switch_default_mic = enabled;
        guard.clone()
    };
    sm.save_state(&snapshot)?;

    let status = state.virtual_mic.status();
    let _ = app.emit("virtual-mic-updated", &status);
    Ok(status)
}

#[tauri::command]
pub async fn restore_original_mic(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<VirtualMicStatus, String> {
    set_auto_switch_mic(app, state, false).await
}
