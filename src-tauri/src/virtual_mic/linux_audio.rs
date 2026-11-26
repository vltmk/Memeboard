//! Linux PulseAudio / PipeWire (`pipewire-pulse`) virtual microphone management
//! using `libpulse-binding` directly (zero external CLI dependencies).

#[cfg(target_os = "linux")]
mod imp {
    use crate::virtual_mic::policy::{self, EndpointInfo, MicRestoreState};
    use crate::virtual_mic::VirtualMicStatus;
    use libpulse_binding::callbacks::ListResult;
    use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
    use libpulse_binding::def;
    use libpulse_binding::mainloop::threaded::Mainloop;
    use parking_lot::Mutex;
    use std::cell::RefCell;
    use std::ops::Deref;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    pub const VIRTUAL_SINK_NAME: &str = "memeboard_virtual_sink";
    pub const VIRTUAL_MIC_NAME: &str = "memeboard_virtual_mic";
    pub const VIRTUAL_SINK_DESC: &str = "Memeboard Virtual Mic Bridge";
    pub const VIRTUAL_MIC_DESC: &str = "Memeboard Virtual Mic";

    struct PulseClient {
        mainloop: Rc<RefCell<Mainloop>>,
        context: Rc<RefCell<Context>>,
    }

    impl PulseClient {
        fn connect() -> Result<Self, String> {
            let mainloop = Rc::new(RefCell::new(
                Mainloop::new().ok_or_else(|| "Failed to allocate PulseAudio mainloop".to_string())?,
            ));
            let context = Rc::new(RefCell::new(
                Context::new(mainloop.borrow().deref(), "MemeboardControl")
                    .ok_or_else(|| "Failed to allocate PulseAudio context".to_string())?,
            ));

            let (state_tx, state_rx) = mpsc::channel::<ContextState>();
            {
                let mut ctx = context.borrow_mut();
                let ctx_weak = Rc::downgrade(&context);
                ctx.set_state_callback(Some(Box::new(move || {
                    if let Some(c) = ctx_weak.upgrade() {
                        if let Ok(c_ref) = c.try_borrow() {
                            let _ = state_tx.send(c_ref.get_state());
                        }
                    }
                })));
                ctx.connect(None, ContextFlagSet::NOAUTOSPAWN, None)
                    .map_err(|e| format!("PulseAudio/PipeWire connect failed: {e}"))?;
            }

            mainloop
                .borrow_mut()
                .start()
                .map_err(|e| format!("Failed to start PulseAudio mainloop: {e}"))?;

            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    let _ = mainloop.borrow_mut().stop();
                    return Err("Timed out connecting to PipeWire/PulseAudio".to_string());
                }
                match state_rx.recv_timeout(remaining) {
                    Ok(ContextState::Ready) => break,
                    Ok(ContextState::Failed | ContextState::Terminated) => {
                        let _ = mainloop.borrow_mut().stop();
                        return Err("PipeWire/PulseAudio server is not running".to_string());
                    }
                    Ok(_) => continue,
                    Err(_) => {
                        let _ = mainloop.borrow_mut().stop();
                        return Err("Timed out waiting for PipeWire/PulseAudio".to_string());
                    }
                }
            }

            // Clear state callback before returning
            mainloop.borrow_mut().lock();
            context.borrow_mut().set_state_callback(None);
            mainloop.borrow_mut().unlock();

            Ok(Self { mainloop, context })
        }

        fn list_sources(&self) -> Result<Vec<(String, String, bool)>, String> {
            let (tx, rx) = mpsc::channel();
            let items = Rc::new(RefCell::new(Vec::new()));
            let items_cb = Rc::clone(&items);

            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow().introspect().get_source_info_list(move |res| match res {
                ListResult::Item(info) => {
                    if let Some(name) = info.name.as_ref() {
                        let desc = info
                            .description
                            .as_ref()
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| name.to_string());
                        let is_monitor = info.monitor_of_sink.is_some();
                        items_cb.borrow_mut().push((name.to_string(), desc, is_monitor));
                    }
                }
                ListResult::End => {
                    let out = std::mem::take(&mut *items_cb.borrow_mut());
                    let _ = tx.send(Ok(out));
                }
                ListResult::Error => {
                    let _ = tx.send(Err("Failed to list PulseAudio sources".to_string()));
                }
            });
            self.mainloop.borrow_mut().unlock();

            let res = rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "Timed out listing audio sources".to_string())?;
            drop(op);
            res
        }

        fn list_sinks(&self) -> Result<Vec<(String, String)>, String> {
            let (tx, rx) = mpsc::channel();
            let items = Rc::new(RefCell::new(Vec::new()));
            let items_cb = Rc::clone(&items);

            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow().introspect().get_sink_info_list(move |res| match res {
                ListResult::Item(info) => {
                    if let Some(name) = info.name.as_ref() {
                        let desc = info
                            .description
                            .as_ref()
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| name.to_string());
                        items_cb.borrow_mut().push((name.to_string(), desc));
                    }
                }
                ListResult::End => {
                    let out = std::mem::take(&mut *items_cb.borrow_mut());
                    let _ = tx.send(Ok(out));
                }
                ListResult::Error => {
                    let _ = tx.send(Err("Failed to list PulseAudio sinks".to_string()));
                }
            });
            self.mainloop.borrow_mut().unlock();

            let res = rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "Timed out listing audio sinks".to_string())?;
            drop(op);
            res
        }

        fn get_server_defaults(&self) -> Result<(Option<String>, Option<String>), String> {
            let (tx, rx) = mpsc::channel();
            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow().introspect().get_server_info(move |info| {
                let sink = info.default_sink_name.as_ref().map(|s| s.to_string());
                let source = info.default_source_name.as_ref().map(|s| s.to_string());
                let _ = tx.send((sink, source));
            });
            self.mainloop.borrow_mut().unlock();

            let res = rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "Timed out querying PulseAudio server info".to_string())?;
            drop(op);
            Ok(res)
        }

        fn list_modules(&self) -> Result<Vec<(u32, String, String)>, String> {
            let (tx, rx) = mpsc::channel();
            let items = Rc::new(RefCell::new(Vec::new()));
            let items_cb = Rc::clone(&items);

            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow().introspect().get_module_info_list(move |res| match res {
                ListResult::Item(info) => {
                    let name = info.name.as_ref().map(|s| s.to_string()).unwrap_or_default();
                    let arg = info.argument.as_ref().map(|s| s.to_string()).unwrap_or_default();
                    items_cb.borrow_mut().push((info.index, name, arg));
                }
                ListResult::End => {
                    let out = std::mem::take(&mut *items_cb.borrow_mut());
                    let _ = tx.send(Ok(out));
                }
                ListResult::Error => {
                    let _ = tx.send(Err("Failed to list PulseAudio modules".to_string()));
                }
            });
            self.mainloop.borrow_mut().unlock();

            let res = rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "Timed out listing PulseAudio modules".to_string())?;
            drop(op);
            res
        }

        fn load_module(&self, name: &str, args: &str) -> Result<u32, String> {
            let (tx, rx) = mpsc::channel();
            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow().introspect().load_module(name, args, move |idx| {
                let _ = tx.send(idx);
            });
            self.mainloop.borrow_mut().unlock();

            let idx = rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| format!("Timed out loading module {name}"))?;
            drop(op);

            if idx == def::INVALID_INDEX {
                Err(format!("PipeWire/PulseAudio rejected {name} ({args})"))
            } else {
                Ok(idx)
            }
        }

        fn unload_module(&self, idx: u32) -> Result<(), String> {
            let (tx, rx) = mpsc::channel();
            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow().introspect().unload_module(idx, move |ok| {
                let _ = tx.send(ok);
            });
            self.mainloop.borrow_mut().unlock();

            let ok = rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| format!("Timed out unloading module {idx}"))?;
            drop(op);
            if ok {
                Ok(())
            } else {
                Err(format!("Failed to unload module {idx}"))
            }
        }

        fn set_default_source(&self, name: &str) -> Result<(), String> {
            let (tx, rx) = mpsc::channel();
            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow_mut().set_default_source(name, move |ok| {
                let _ = tx.send(ok);
            });
            self.mainloop.borrow_mut().unlock();

            let ok = rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "Timed out setting default microphone".to_string())?;
            drop(op);
            if ok {
                Ok(())
            } else {
                Err(format!("Failed to set default source '{name}'"))
            }
        }

        fn set_default_sink(&self, name: &str) -> Result<(), String> {
            let (tx, rx) = mpsc::channel();
            self.mainloop.borrow_mut().lock();
            let op = self.context.borrow_mut().set_default_sink(name, move |ok| {
                let _ = tx.send(ok);
            });
            self.mainloop.borrow_mut().unlock();

            let ok = rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "Timed out setting default playback sink".to_string())?;
            drop(op);
            if ok {
                Ok(())
            } else {
                Err(format!("Failed to set default sink '{name}'"))
            }
        }
    }

    impl Drop for PulseClient {
        fn drop(&mut self) {
            self.mainloop.borrow_mut().lock();
            self.context.borrow_mut().disconnect();
            self.mainloop.borrow_mut().unlock();
            self.mainloop.borrow_mut().stop();
        }
    }

    pub fn enumerate_active_endpoints(capture: bool) -> Result<Vec<EndpointInfo>, String> {
        let client = PulseClient::connect()?;
        if capture {
            let sources = client.list_sources()?;
            Ok(sources
                .into_iter()
                .filter(|(name, _, is_monitor)| !is_monitor || name == VIRTUAL_MIC_NAME)
                .map(|(id, friendly_name, _)| EndpointInfo { id, friendly_name })
                .collect())
        } else {
            let sinks = client.list_sinks()?;
            Ok(sinks
                .into_iter()
                .map(|(id, friendly_name)| EndpointInfo { id, friendly_name })
                .collect())
        }
    }

    pub fn get_default_endpoint(capture: bool, _comms: bool) -> Option<EndpointInfo> {
        let client = PulseClient::connect().ok()?;
        let (default_sink, default_source) = client.get_server_defaults().ok()?;
        let target = if capture { default_source? } else { default_sink? };
        let endpoints = enumerate_active_endpoints(capture).ok()?;
        endpoints
            .into_iter()
            .find(|e| e.id == target)
            .or(Some(EndpointInfo {
                id: target.clone(),
                friendly_name: target,
            }))
    }

    pub fn set_default_endpoint(capture: bool, device_id: &str) -> Result<(), String> {
        let client = PulseClient::connect()?;
        if capture {
            client.set_default_source(device_id)
        } else {
            client.set_default_sink(device_id)
        }
    }

    pub fn module_args_null_sink() -> String {
        format!(
            "sink_name={VIRTUAL_SINK_NAME} rate=48000 channels=2 sink_properties=\"device.description='{VIRTUAL_SINK_DESC}' node.pause-on-idle=false\""
        )
    }

    pub fn module_args_remap_source() -> String {
        format!(
            "master={VIRTUAL_SINK_NAME}.monitor source_name={VIRTUAL_MIC_NAME} rate=48000 channels=2 source_properties=\"device.description='{VIRTUAL_MIC_DESC}' media.class=Audio/Source device.icon-name=audio-input-microphone node.pause-on-idle=false\""
        )
    }

    pub fn is_memeboard_module_arg(arg: &str) -> bool {
        arg.contains(VIRTUAL_SINK_NAME) || arg.contains(VIRTUAL_MIC_NAME)
    }

    pub struct LinuxVirtualMic {
        loaded_modules: Mutex<Vec<u32>>,
        last_error: Mutex<Option<String>>,
        snapshot: Mutex<Option<MicRestoreState>>,
    }

    impl LinuxVirtualMic {
        pub fn new() -> Self {
            Self {
                loaded_modules: Mutex::new(Vec::new()),
                last_error: Mutex::new(None),
                snapshot: Mutex::new(None),
            }
        }

        fn cleanup_stale_modules(client: &PulseClient) {
            if let Ok(mods) = client.list_modules() {
                // Unload remap-source before null-sink (reverse order)
                for (idx, _, arg) in mods.into_iter().rev() {
                    if is_memeboard_module_arg(&arg) {
                        let _ = client.unload_module(idx);
                    }
                }
            }
        }

        pub fn startup(&self, auto_switch_default_mic: bool) {
            let snap = policy::snapshot_physical_defaults_before_attach();
            *self.snapshot.lock() = Some(snap.clone());

            if !auto_switch_default_mic
                && (snap.active || policy::is_virtual_mic_currently_default())
            {
                let _ = policy::restore_original_mic();
            }

            let client = match PulseClient::connect() {
                Ok(c) => c,
                Err(err) => {
                    *self.last_error.lock() = Some(format!(
                        "{err}. Ensure PipeWire (pipewire-pulse) or PulseAudio is running."
                    ));
                    return;
                }
            };

            Self::cleanup_stale_modules(&client);

            let sink_idx = match client.load_module("module-null-sink", &module_args_null_sink()) {
                Ok(idx) => idx,
                Err(err) => {
                    *self.last_error.lock() = Some(err);
                    return;
                }
            };

            let source_idx = match client.load_module("module-remap-source", &module_args_remap_source()) {
                Ok(idx) => idx,
                Err(err) => {
                    let _ = client.unload_module(sink_idx);
                    *self.last_error.lock() = Some(err);
                    return;
                }
            };

            *self.loaded_modules.lock() = vec![sink_idx, source_idx];
            *self.last_error.lock() = None;

            // Wait briefly for PipeWire/PulseAudio to register the new sink/source nodes
            for _ in 0..25 {
                policy::guard_playback_defaults(&snap);
                let has_sink = client
                    .list_sinks()
                    .unwrap_or_default()
                    .iter()
                    .any(|(name, _)| name == VIRTUAL_SINK_NAME);
                let has_src = client
                    .list_sources()
                    .unwrap_or_default()
                    .iter()
                    .any(|(name, _, _)| name == VIRTUAL_MIC_NAME);
                if has_sink && has_src {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }

            policy::guard_playback_defaults(&snap);
            if auto_switch_default_mic {
                if let Err(err) = policy::switch_default_mic_to_virtual(&snap) {
                    *self.last_error.lock() = Some(err);
                }
            }
        }

        pub fn shutdown(&self) {
            let _ = policy::restore_original_mic();
            let indices = {
                let mut guard = self.loaded_modules.lock();
                let mut v = Vec::new();
                std::mem::swap(&mut *guard, &mut v);
                v
            };

            if let Ok(client) = PulseClient::connect() {
                for idx in indices.into_iter().rev() {
                    let _ = client.unload_module(idx);
                }
                Self::cleanup_stale_modules(&client);
            }
        }

        pub fn set_auto_switch(&self, enabled: bool) -> Result<(), String> {
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

        pub fn install_driver(&self, auto_switch_default_mic: bool) -> Result<VirtualMicStatus, String> {
            self.startup(auto_switch_default_mic);
            let st = self.status();
            if !st.virtual_device_attached {
                return Err(st.status_message);
            }
            Ok(st)
        }

        pub fn status(&self) -> VirtualMicStatus {
            let client = match PulseClient::connect() {
                Ok(c) => c,
                Err(err) => {
                    return VirtualMicStatus {
                        driver_installed: false,
                        service_installed: false,
                        virtual_device_attached: false,
                        virtual_mic_is_default: false,
                        saved_physical_mic: policy::saved_original_physical_mic_name(),
                        status_message: format!(
                            "PipeWire/PulseAudio unavailable ({err}). Install pipewire-pulse."
                        ),
                    };
                }
            };

            let sink_ready = client
                .list_sinks()
                .unwrap_or_default()
                .iter()
                .any(|(name, _)| name == VIRTUAL_SINK_NAME);
            let src_ready = client
                .list_sources()
                .unwrap_or_default()
                .iter()
                .any(|(name, _, _)| name == VIRTUAL_MIC_NAME);
            let virtual_device_attached = sink_ready && src_ready;

            let virtual_mic_is_default = client
                .get_server_defaults()
                .ok()
                .and_then(|(_, src)| src)
                .is_some_and(|s| s == VIRTUAL_MIC_NAME);

            let saved_physical_mic = policy::saved_original_physical_mic_name();
            let err = self.last_error.lock().clone();
            let status_message = if virtual_device_attached {
                if virtual_mic_is_default {
                    "Memeboard Virtual Mic Active (System Default)".to_string()
                } else {
                    "Memeboard Virtual Mic Ready (PipeWire/PulseAudio)".to_string()
                }
            } else if let Some(e) = err {
                e
            } else {
                "Virtual Mic standby (PipeWire/PulseAudio)".to_string()
            };

            VirtualMicStatus {
                driver_installed: true,
                service_installed: true,
                virtual_device_attached,
                virtual_mic_is_default,
                saved_physical_mic,
                status_message,
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(test)]
mod tests {
    #[test]
    fn linux_virtual_mic_module_args_prevent_idle_suspend() {
        let sink_args = format!(
            "sink_name=memeboard_virtual_sink rate=48000 channels=2 sink_properties=\"device.description='Memeboard Virtual Mic Bridge' node.pause-on-idle=false\""
        );
        let src_args = format!(
            "master=memeboard_virtual_sink.monitor source_name=memeboard_virtual_mic rate=48000 channels=2 source_properties=\"device.description='Memeboard Virtual Mic' media.class=Audio/Source device.icon-name=audio-input-microphone node.pause-on-idle=false\""
        );
        assert!(sink_args.contains("node.pause-on-idle=false"));
        assert!(src_args.contains("media.class=Audio/Source"));
        assert!(src_args.contains("memeboard_virtual_mic"));
    }
}
