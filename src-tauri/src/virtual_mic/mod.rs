pub mod delay_line;
pub mod event;
pub mod policy;
pub mod uac2;
pub mod usbip;

#[cfg(windows)]
pub mod windows_svc;

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
    last_error: Mutex<Option<String>>,
    snapshot: Mutex<Option<policy::MicRestoreState>>,
}

impl VirtualMicManager {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            in_process: Mutex::new(None),
            last_error: Mutex::new(None),
            snapshot: Mutex::new(None),
        }
    }

    /// Starts the built-in virtual microphone device (via Windows service when installed,
    /// or elevated helper fallback if driver is present in portable/dev mode), waits briefly
    /// for WASAPI endpoints, guards default playback, and optionally switches Windows Default
    /// Recording & Communications microphone.
    pub fn startup(&self, auto_switch_default_mic: bool) {
        #[cfg(windows)]
        {
            raise_process_priority();

            let snap = policy::snapshot_physical_defaults_before_attach();
            *self.snapshot.lock() = Some(snap.clone());

            if !is_driver_ready() {
                *self.last_error.lock() = Some(
                    "Built-in USB Audio virtual driver (usbip-win2) is not installed.".to_string(),
                );
                return;
            }

            let svc_state = query_service_state();
            let attached_ok = if svc_state != ServiceState::NotInstalled {
                match start_installed_service() {
                    Ok(()) => match ping_service_pipe("STATUS") {
                        Ok(reply) if reply.contains("attached") => true,
                        Ok(reply) => {
                            *self.last_error.lock() = Some(format!("Virtual Mic service: {reply}"));
                            false
                        }
                        Err(err) => {
                            *self.last_error.lock() = Some(format!("Virtual Mic pipe error: {err}"));
                            false
                        }
                    },
                    Err(err) => {
                        *self.last_error.lock() = Some(format!("Failed to start Virtual Mic service: {err}"));
                        false
                    }
                }
            } else {
                // Portable/dev mode without service installed: check if already attached or run in-process
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
                // Wait up to ~4s for Windows usbaudio2.sys + AudioEndpointBuilder to register the endpoint
                for _ in 0..40 {
                    policy::guard_playback_defaults(&snap);
                    let endpoints = policy::enumerate_active_endpoints(true).unwrap_or_default();
                    if endpoints
                        .iter()
                        .any(|e| e.friendly_name.to_lowercase().contains("memeboard virtual mic"))
                    {
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

                // Delayed playback guard in case Windows PnP evaluates speaker policy 400ms later
                let snap_delayed = snap.clone();
                std::thread::spawn(move || {
                    for delay in [250, 500, 1000] {
                        std::thread::sleep(std::time::Duration::from_millis(delay));
                        policy::guard_playback_defaults(&snap_delayed);
                    }
                });
            }
        }
        #[cfg(not(windows))]
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
        #[cfg(not(windows))]
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
        #[cfg(not(windows))]
        {
            let _ = auto_switch_default_mic;
            Err("Built-in virtual driver installation is only supported on Windows.".into())
        }
    }

    pub fn status(&self) -> VirtualMicStatus {
        #[cfg(windows)]
        {
            let driver_installed = is_driver_ready();
            let service_installed = query_service_state() != ServiceState::NotInstalled;
            let virtual_device_attached = policy::enumerate_active_endpoints(true)
                .unwrap_or_default()
                .iter()
                .any(|e| e.friendly_name.to_lowercase().contains("memeboard virtual mic"));
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
        #[cfg(not(windows))]
        {
            VirtualMicStatus {
                driver_installed: false,
                service_installed: false,
                virtual_device_attached: false,
                virtual_mic_is_default: false,
                saved_physical_mic: None,
                status_message: "Not on Windows".to_string(),
            }
        }
    }
}

/// Handles `--service` or `--attach-broker` CLI invocations before GUI startup.
/// Returns `Some(exit_code)` if this invocation was a service/broker helper.
pub fn maybe_run_helper_mode() -> Option<i32> {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.iter().any(|a| a == "--service") {
            return Some(if windows_svc::run_windows_service().is_ok() {
                0
            } else {
                1
            });
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
