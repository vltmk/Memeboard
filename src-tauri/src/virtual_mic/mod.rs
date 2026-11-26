pub mod delay_line;
pub mod event;
pub mod policy;
pub mod uac2;
pub mod usbip;

#[cfg(windows)]
pub mod windows_svc;

#[cfg(target_os = "linux")]
pub mod linux_audio;

#[cfg(windows)]
use parking_lot::Mutex;
use serde::Serialize;

#[cfg(windows)]
use self::windows_svc::{
    InProcessVirtualCable, ServiceState, is_driver_ready, ping_service_pipe,
    query_service_state, raise_process_priority, start_installed_service, stop_installed_service,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualMicStatus {
    pub driver_installed: bool,
    pub service_installed: bool,
    pub virtual_device_attached: bool,
    pub virtual_mic_is_default: bool,
    pub saved_physical_mic: Option<String>,
    pub status_message: String,
}

pub struct VirtualMicManager {
    #[cfg(windows)]
    in_process: Mutex<Option<InProcessVirtualCable>>,
    #[cfg(target_os = "linux")]
    linux: linux_audio::LinuxVirtualMic,
    #[cfg(windows)]
    last_error: Mutex<Option<String>>,
    #[cfg(windows)]
    snapshot: Mutex<Option<policy::MicRestoreState>>,
}

impl VirtualMicManager {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            in_process: Mutex::new(None),
            #[cfg(target_os = "linux")]
            linux: linux_audio::LinuxVirtualMic::new(),
            #[cfg(windows)]
            last_error: Mutex::new(None),
            #[cfg(windows)]
            snapshot: Mutex::new(None),
        }
    }

    /// Starts the built-in virtual microphone device, waits briefly for OS endpoints,
    /// guards default playback, and optionally switches the OS default microphone.
    pub fn startup(&self, auto_switch_default_mic: bool) {
        #[cfg(windows)]
        {
            raise_process_priority();

            let snap = policy::snapshot_physical_defaults_before_attach();
            *self.snapshot.lock() = Some(snap.clone());

            if !auto_switch_default_mic && (snap.active || policy::is_virtual_mic_currently_default())
            {
                let _ = policy::restore_original_mic();
            }

            if !is_driver_ready() {
                *self.last_error.lock() = Some(
                    "Built-in USB Audio virtual driver (usbip-win2) is not installed.".to_string(),
                );
                return;
            }

            let svc_state = query_service_state();
            let attached_ok = if svc_state != ServiceState::NotInstalled {
                let try_svc = || match start_installed_service() {
                    Ok(()) => match ping_service_pipe("STATUS") {
                        Ok(reply) if reply.trim() == "OK attached" => Ok(true),
                        Ok(reply) => Err(format!("Virtual Mic service: {reply}")),
                        Err(err) => Err(format!("Virtual Mic pipe error: {err}")),
                    },
                    Err(err) => Err(format!("Failed to start Virtual Mic service: {err}")),
                };

                match try_svc() {
                    Ok(v) => v,
                    Err(first_err) => {
                        if svc_state == ServiceState::Running {
                            let _ = stop_installed_service();
                            match try_svc() {
                                Ok(v) => v,
                                Err(err) => {
                                    *self.last_error.lock() = Some(err);
                                    false
                                }
                            }
                        } else {
                            *self.last_error.lock() = Some(first_err);
                            false
                        }
                    }
                }
            } else {
                match InProcessVirtualCable::start() {
                    Ok(cable) => {
                        *self.in_process.lock() = Some(cable);
                        true
                    }
                    Err(err) => {
                        *self.last_error.lock() = Some(err);
                        false
                    }
                }
            };

            if attached_ok {
                *self.last_error.lock() = None;
                for _ in 0..80 {
                    policy::guard_playback_defaults(&snap);
                    let cap_ready = policy::enumerate_active_endpoints(true)
                        .unwrap_or_default()
                        .iter()
                        .any(|e| e.friendly_name.to_lowercase().contains("memeboard virtual mic"));
                    let ren_ready = policy::enumerate_active_endpoints(false)
                        .unwrap_or_default()
                        .iter()
                        .any(|e| e.friendly_name.to_lowercase().contains("memeboard virtual mic"));
                    if cap_ready && ren_ready {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }

                policy::guard_playback_defaults(&snap);
                if auto_switch_default_mic {
                    if let Err(err) = policy::switch_default_mic_to_virtual(&snap) {
                        *self.last_error.lock() = Some(err);
                    }
                }

                let snap_delayed = snap.clone();
                std::thread::spawn(move || {
                    for delay in [250, 500, 1000] {
                        std::thread::sleep(std::time::Duration::from_millis(delay));
                        policy::guard_playback_defaults(&snap_delayed);
                    }
                });
            }
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.startup(auto_switch_default_mic);
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = auto_switch_default_mic;
        }
    }

    pub fn shutdown(&self) {
        #[cfg(windows)]
        {
            let _ = policy::restore_original_mic();
            if let Some(cable) = self.in_process.lock().take() {
                cable.stop();
            }
            if query_service_state() != ServiceState::NotInstalled {
                let _ = stop_installed_service();
            }
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.shutdown();
        }
    }

    pub fn set_auto_switch(&self, enabled: bool) -> Result<(), String> {
        #[cfg(windows)]
        {
            if enabled {
                let snap = self
                    .snapshot
                    .lock()
                    .clone()
                    .unwrap_or_else(policy::snapshot_physical_defaults_before_attach);
                policy::switch_default_mic_to_virtual(&snap)?;
            } else {
                policy::restore_original_mic()?;
            }
            Ok(())
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.set_auto_switch(enabled)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = enabled;
            Ok(())
        }
    }

    pub fn install_driver(&self, auto_switch_default_mic: bool) -> Result<VirtualMicStatus, String> {
        #[cfg(windows)]
        {
            windows_svc::install_bundled_driver_elevated()?;
            self.startup(auto_switch_default_mic);
            Ok(self.status())
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.install_driver(auto_switch_default_mic)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = auto_switch_default_mic;
            Err("Built-in virtual driver installation is not supported on this OS.".into())
        }
    }

    pub fn status(&self) -> VirtualMicStatus {
        #[cfg(windows)]
        {
            let driver_installed = is_driver_ready();
            let service_installed = query_service_state() != ServiceState::NotInstalled;
            let cap_attached = policy::enumerate_active_endpoints(true)
                .unwrap_or_default()
                .iter()
                .any(|e| e.friendly_name.to_lowercase().contains("memeboard virtual mic"));
            let ren_attached = policy::enumerate_active_endpoints(false)
                .unwrap_or_default()
                .iter()
                .any(|e| e.friendly_name.to_lowercase().contains("memeboard virtual mic"));
            let virtual_device_attached = cap_attached && ren_attached;
            let virtual_mic_is_default = policy::is_virtual_mic_currently_default();
            let saved_physical_mic = policy::saved_original_physical_mic_name();
            let err = self.last_error.lock().clone();
            let status_message = if virtual_device_attached {
                if virtual_mic_is_default {
                    "Memeboard Virtual Mic Active (Windows Default)".to_string()
                } else {
                    "Memeboard Virtual Mic Ready".to_string()
                }
            } else if let Some(e) = err {
                e
            } else if !driver_installed {
                "Virtual driver not installed".to_string()
            } else {
                "Virtual Mic standby".to_string()
            };
            VirtualMicStatus {
                driver_installed,
                service_installed,
                virtual_device_attached,
                virtual_mic_is_default,
                saved_physical_mic,
                status_message,
            }
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.status()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            VirtualMicStatus {
                driver_installed: false,
                service_installed: false,
                virtual_device_attached: false,
                virtual_mic_is_default: false,
                saved_physical_mic: None,
                status_message: "Unsupported platform".to_string(),
            }
        }
    }
}

/// Handles CLI helper invocations (`--service`, `--restore-mic`, `--prepare-uninstall`,
/// `--attach-broker`) before GUI startup. Returns `Some(exit_code)` if handled.
pub fn maybe_run_helper_mode() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--restore-mic") {
        return Some(if policy::restore_original_mic().is_ok() {
            0
        } else {
            1
        });
    }

    #[cfg(windows)]
    {
        if args.iter().any(|a| a == "--service") {
            return Some(if windows_svc::run_windows_service().is_ok() {
                0
            } else {
                1
            });
        }
        if args.iter().any(|a| a == "--prepare-uninstall") {
            let _ = policy::restore_original_mic();
            let _ = windows_svc::stop_installed_service();
            return Some(0);
        }
        if args.iter().any(|a| a == "--attach-broker") {
            let get_arg = |flag: &str| -> Option<String> {
                args.iter()
                    .position(|a| a == flag)
                    .and_then(|i| args.get(i + 1))
                    .cloned()
            };
            let Some(address) = get_arg("--broker-address") else {
                return Some(2);
            };
            let Some(token) = get_arg("--broker-token") else {
                return Some(2);
            };
            let listen = get_arg("--listen").unwrap_or_else(|| windows_svc::DEFAULT_LISTEN.to_string());
            return Some(
                if windows_svc::run_attach_broker(&address, &token, &listen).is_ok() {
                    0
                } else {
                    1
                },
            );
        }
    }
    None
}
