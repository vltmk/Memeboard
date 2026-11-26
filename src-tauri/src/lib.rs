pub mod audio_thread;
pub mod commands;
pub mod decoder;
pub mod device_manager;
pub mod hotkeys;
#[cfg(target_os = "linux")]
pub mod linux_audio_io;
pub mod mixer;
pub mod state_manager;
pub mod types;
pub mod virtual_mic;

#[cfg(all(test, target_os = "windows", target_env = "gnu"))]
#[link(name = "test_manifest", kind = "static", modifiers = "+whole-archive")]
unsafe extern "C" {}

use commands::{
    AppState, bring_window_to_front, configure_frameless_window, delete_sound, dock_main_window,
    dock_window_bottom_right, export_pack, export_sound_file, get_audio_devices,
    get_overlay_visibility, get_platform_info, get_playback_status, get_virtual_mic_status,
    import_pack, import_sound_file, install_virtual_driver, is_main_docked,
    is_window_docked_bottom_right, load_soundboard_state, play_sound, play_sound_internal,
    preview_sound, restore_original_mic, save_soundboard_state, set_aec_safe_monitor,
    set_audio_devices, set_auto_switch_mic, set_ducking_settings, set_hotkey_recording,
    set_volumes, stop_all_sounds, stop_preview, stop_sound, sync_hotkeys_from_state,
    toggle_overlay_window, toggle_overlay_window_internal,
};
use device_manager::DeviceManager;
use hotkeys::{HotkeyAction, HotkeyManager};
use mixer::AudioMixer;
use parking_lot::Mutex;
use state_manager::StateManager;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use virtual_mic::VirtualMicManager;

static SHUTDOWN_RAN: AtomicBool = AtomicBool::new(false);

fn perform_clean_shutdown(app: &AppHandle) {
    if SHUTDOWN_RAN.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.mixer.stop_all();
        state.hotkeys.shutdown();
        {
            let mut dm = state.device_manager.lock();
            dm.stop_all();
        }
        state.virtual_mic.shutdown();
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(main_win) = app.get_webview_window("main") {
        configure_frameless_window(&main_win);
        let _ = main_win.unminimize();
        let _ = main_win.show();
        bring_window_to_front(&main_win);
        let _ = main_win.set_focus();
        let _ = app.emit("main-shown", ());
    }
}

fn handle_forwarded_cli_args(app: &AppHandle, args: &[String]) {
    let mut handled_action = false;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--stop-all" => {
                app.state::<AppState>().mixer.stop_all();
                let _ = app.emit("soundboard-panic", ());
                handled_action = true;
            }
            "--toggle-overlay" => {
                let _ = toggle_overlay_window_internal(app);
                handled_action = true;
            }
            "--trigger" => {
                if let Some(sound_id) = iter.next() {
                    let st = app.state::<AppState>();
                    if play_sound_internal(&st, sound_id).is_ok() {
                        let _ = app.emit("sound-triggered", sound_id);
                    }
                    handled_action = true;
                }
            }
            _ => {}
        }
    }

    if !handled_action {
        show_main_window(app);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state_manager = Arc::new(StateManager::new().expect("Failed to initialize Memeboard storage"));
    let initial_state = state_manager.load_state().unwrap_or_else(|e| {
        eprintln!("[Memeboard] State load fallback ({e})");
        types::SoundboardState {
            sounds: Vec::new(),
            settings: types::AudioSettings::default(),
            categories: Vec::new(),
        }
    });

    let mixer = Arc::new(AudioMixer::new());
    mixer.controls.set_volumes(
        initial_state.settings.monitor_volume,
        initial_state.settings.broadcast_volume,
        initial_state.settings.mic_volume,
        initial_state.settings.monitor_muted,
        initial_state.settings.broadcast_muted,
    );
    mixer.controls.set_ducking(
        initial_state.settings.voice_ducking_enabled,
        initial_state.settings.voice_ducking_db,
    );
    mixer
        .controls
        .set_aec_safe_monitor(initial_state.settings.aec_safe_monitor);

    // Preload sound waveforms/samples asynchronously after UI opens
    state_manager.spawn_preload_all(initial_state.clone(), Arc::clone(&mixer));

    let mut device_manager = DeviceManager::new();
    let _ = device_manager.restart_streams(
        Arc::clone(&mixer),
        initial_state.settings.mic_device_id.clone(),
        initial_state.settings.broadcast_device_id.clone(),
        initial_state.settings.monitor_device_id.clone(),
    );

    let virtual_mic = Arc::new(VirtualMicManager::new());
    let hotkeys = Arc::new(HotkeyManager::new());

    let app_state = AppState {
        mixer: Arc::clone(&mixer),
        device_manager: Mutex::new(device_manager),
        state_manager: Arc::clone(&state_manager),
        virtual_mic: Arc::clone(&virtual_mic),
        hotkeys: Arc::clone(&hotkeys),
        board_state: Mutex::new(initial_state.clone()),
        overlay_visible: AtomicBool::new(false),
        tray_available: AtomicBool::new(false),
    };

    sync_hotkeys_from_state(&app_state);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            handle_forwarded_cli_args(app, &args);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .setup(move |app| {
            let handle = app.handle().clone();

            if let Some(main_win) = app.get_webview_window("main") {
                configure_frameless_window(&main_win);
                dock_window_bottom_right(&main_win, 0, 0);
                let _ = main_win.show();
                bring_window_to_front(&main_win);
                let _ = main_win.set_focus();
                eprintln!(
                    "[Memeboard] Main window ready: visible={:?}, pos={:?}, size={:?}",
                    main_win.is_visible(),
                    main_win.outer_position(),
                    main_win.outer_size()
                );
            }
            if let Some(overlay_win) = app.get_webview_window("overlay") {
                configure_frameless_window(&overlay_win);
                dock_window_bottom_right(&overlay_win, 16, 16);
            }

            // 1. Start non-consuming Windows Raw Input global hotkey listener
            {
                let hotkey_app = handle.clone();
                let state = handle.state::<AppState>();
                state.hotkeys.start(move |action| {
                    let st = hotkey_app.state::<AppState>();
                    match action {
                        HotkeyAction::Panic => {
                            st.mixer.stop_all();
                            let _ = hotkey_app.emit("soundboard-panic", ());
                        }
                        HotkeyAction::ToggleOverlay => {
                            let _ = toggle_overlay_window_internal(&hotkey_app);
                        }
                        HotkeyAction::TriggerSound(sound_id) => {
                            if let Err(err) = play_sound_internal(&st, &sound_id) {
                                eprintln!("[Memeboard] Hotkey play failed for '{sound_id}': {err}");
                            } else {
                                let _ = hotkey_app.emit("sound-triggered", &sound_id);
                            }
                        }
                        HotkeyAction::ShowMainWindow => {
                            show_main_window(&hotkey_app);
                        }
                    }
                });
            }

            // 2. Attach built-in Memeboard Virtual Mic & auto-switch Windows Default Mic in background
            {
                let vmic_app = handle.clone();
                let auto_switch = initial_state.settings.auto_switch_default_mic;
                std::thread::Builder::new()
                    .name("memeboard-vmic-startup".to_string())
                    .spawn(move || {
                        let st = vmic_app.state::<AppState>();
                        st.virtual_mic.startup(auto_switch);

                        let vm_status = st.virtual_mic.status();
                        if vm_status.virtual_device_attached {
                            let (mic_id, bcast_id, mon_id) = {
                                let guard = st.board_state.lock();
                                (
                                    guard.settings.mic_device_id.clone(),
                                    guard.settings.broadcast_device_id.clone(),
                                    guard.settings.monitor_device_id.clone(),
                                )
                            };
                            let mut dm = st.device_manager.lock();
                            let _ = dm.restart_streams(
                                Arc::clone(&st.mixer),
                                mic_id,
                                bcast_id,
                                mon_id,
                            );
                        }
                        let _ = vmic_app.emit("virtual-mic-updated", &vm_status);
                    })
                    .ok();
            }

            // 3. Configure System Tray (background routing + clean exit mic restoration)
            let show_item = MenuItemBuilder::with_id("tray_show", "Open Memeboard").build(app)?;
            let overlay_item =
                MenuItemBuilder::with_id("tray_overlay", "Toggle Mini Overlay").build(app)?;
            let stop_item =
                MenuItemBuilder::with_id("tray_stop_all", "Stop All Sounds (Panic)").build(app)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_item =
                MenuItemBuilder::with_id("tray_quit", "Quit Memeboard").build(app)?;

            let tray_menu = MenuBuilder::new(app)
                .items(&[&show_item, &overlay_item, &stop_item, &sep, &quit_item])
                .build()?;

            let mut tray_builder = TrayIconBuilder::with_id("memeboard-tray")
                .menu(&tray_menu)
                .tooltip("Memeboard - Soundboard & Virtual Mic")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "tray_show" => show_main_window(app),
                    "tray_overlay" => {
                        let _ = toggle_overlay_window_internal(app);
                    }
                    "tray_stop_all" => {
                        app.state::<AppState>().mixer.stop_all();
                    }
                    "tray_quit" => {
                        perform_clean_shutdown(app);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            let tray_ok = match tray_builder.build(app) {
                Ok(_) => true,
                Err(err) => {
                    eprintln!("[Memeboard] System tray unavailable ({err}); close will minimize.");
                    false
                }
            };
            handle
                .state::<AppState>()
                .tray_available
                .store(tray_ok, Ordering::Release);

            #[cfg(target_os = "linux")]
            {
                let sig_handle = handle.clone();
                if let Err(err) = ctrlc::set_handler(move || {
                    perform_clean_shutdown(&sig_handle);
                    sig_handle.exit(0);
                }) {
                    eprintln!("[Memeboard] Signal handler registration skipped: {err}");
                }
            }

            eprintln!("[Memeboard] setup() complete; entering event loop");

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                if window.label() == "main" {
                    api.prevent_close();
                    if window
                        .app_handle()
                        .state::<AppState>()
                        .tray_available
                        .load(Ordering::Acquire)
                    {
                        let _ = window.hide();
                    } else {
                        let _ = window.minimize();
                    }
                } else if window.label() == "overlay" {
                    api.prevent_close();
                    let _ = window.hide();
                    let app = window.app_handle();
                    app.state::<AppState>()
                        .overlay_visible
                        .store(false, Ordering::Release);
                    let _ = app.emit("overlay-visibility", false);
                }
            }
            WindowEvent::Moved(_) => {
                if window.label() == "main" {
                    let app = window.app_handle();
                    if let Some(main_win) = app.get_webview_window("main") {
                        let docked = is_window_docked_bottom_right(&main_win);
                        let _ = app.emit("main-dock-changed", docked);
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_audio_devices,
            set_audio_devices,
            play_sound,
            preview_sound,
            stop_sound,
            stop_preview,
            stop_all_sounds,
            set_volumes,
            set_ducking_settings,
            set_aec_safe_monitor,
            get_playback_status,
            import_sound_file,
            export_sound_file,
            export_pack,
            import_pack,
            delete_sound,
            load_soundboard_state,
            save_soundboard_state,
            toggle_overlay_window,
            get_overlay_visibility,
            dock_main_window,
            is_main_docked,
            set_hotkey_recording,
            get_platform_info,
            get_virtual_mic_status,
            install_virtual_driver,
            set_auto_switch_mic,
            restore_original_mic
        ])
        .build(tauri::generate_context!())
        .expect("Error while building Memeboard application");

    app.run(|app_handle, event| {
        if let RunEvent::Exit | RunEvent::ExitRequested { .. } = event {
            if matches!(event, RunEvent::Exit) {
                perform_clean_shutdown(app_handle);
            }
        }
    });
}
