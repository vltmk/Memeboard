//! Windows Default Audio Endpoint Policy Switching (`IPolicyConfig`) and
//! crash-safe saving/restoration of the user's original physical microphone.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const VIRTUAL_MIC_NAME_PREFIX: &str = "Memeboard Virtual Mic";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MicRestoreState {
    pub active: bool,
    pub original_capture_console_id: Option<String>,
    pub original_capture_comms_id: Option<String>,
    pub original_capture_name: Option<String>,
    pub original_render_console_id: Option<String>,
    pub original_render_comms_id: Option<String>,
    pub switched_at: u64,
}

#[derive(Debug, Clone)]
pub struct EndpointInfo {
    pub id: String,
    pub friendly_name: String,
}

pub fn is_virtual_audio_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("memeboard virtual mic")
        || lower.contains("memeboard_virtual")
        || lower.contains("vb-audio")
        || lower.contains("cable input")
        || lower.contains("cable output")
        || lower.contains("blackhole")
}

pub fn restore_file_path() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Memeboard")
        .join("mic_restore.json")
}

pub fn load_restore_state(path: &Path) -> Option<MicRestoreState> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_restore_state(path: &Path, state: &MicRestoreState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(windows)]
mod win_policy {
    use super::*;
    use std::ffi::c_void;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        eCapture, eCommunications, eConsole, eMultimedia, eRender,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize, STGM_READ,
    };
    use windows::core::{GUID, HRESULT, HSTRING, PCWSTR};

    #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    unsafe trait IPolicyConfig: windows::core::IUnknown {
        unsafe fn get_mix_format(
            &self,
            wsz_device_id: PCWSTR,
            pp_format: *mut *mut c_void,
        ) -> HRESULT;
        unsafe fn get_device_format(
            &self,
            wsz_device_id: PCWSTR,
            b_default: i32,
            pp_format: *mut *mut c_void,
        ) -> HRESULT;
        unsafe fn reset_device_format(&self, wsz_device_id: PCWSTR) -> HRESULT;
        unsafe fn set_device_format(
            &self,
            wsz_device_id: PCWSTR,
            p_endpoint_format: *mut c_void,
            mix_format: *mut c_void,
        ) -> HRESULT;
        unsafe fn get_processing_period(
            &self,
            wsz_device_id: PCWSTR,
            b_default: i32,
            pmft_default: *mut i64,
            pmft_min: *mut i64,
        ) -> HRESULT;
        unsafe fn set_processing_period(
            &self,
            wsz_device_id: PCWSTR,
            pmft_period: *const i64,
        ) -> HRESULT;
        unsafe fn get_share_mode(&self, wsz_device_id: PCWSTR, p_mode: *mut c_void) -> HRESULT;
        unsafe fn set_share_mode(&self, wsz_device_id: PCWSTR, mode: *const c_void) -> HRESULT;
        unsafe fn get_property_value(
            &self,
            wsz_device_id: PCWSTR,
            b_fx_store: i32,
            key: *const c_void,
            pv: *mut c_void,
        ) -> HRESULT;
        unsafe fn set_property_value(
            &self,
            wsz_device_id: PCWSTR,
            b_fx_store: i32,
            key: *const c_void,
            pv: *const c_void,
        ) -> HRESULT;
        unsafe fn set_default_endpoint(&self, wsz_device_id: PCWSTR, role: ERole) -> HRESULT;
        unsafe fn set_endpoint_visibility(
            &self,
            wsz_device_id: PCWSTR,
            b_visible: i32,
        ) -> HRESULT;
    }

    const CLSID_POLICY_CONFIG_CLIENT: GUID =
        GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    struct ComGuard(bool);
    impl ComGuard {
        fn init() -> Self {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            Self(hr.is_ok())
        }
    }
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn device_info(dev: &IMMDevice) -> Result<EndpointInfo, String> {
        unsafe {
            let id_pwstr = dev.GetId().map_err(|e| format!("GetId failed: {e}"))?;
            let id = id_pwstr
                .to_string()
                .map_err(|e| format!("GetId UTF-16 error: {e}"))?;
            CoTaskMemFree(Some(id_pwstr.0.cast()));

            let store = dev
                .OpenPropertyStore(STGM_READ)
                .map_err(|e| format!("OpenPropertyStore failed: {e}"))?;
            let val = store
                .GetValue(&PKEY_Device_FriendlyName)
                .map_err(|e| format!("GetValue FriendlyName failed: {e}"))?;
            let name_pwstr = PropVariantToStringAlloc(&val)
                .map_err(|e| format!("PropVariantToStringAlloc failed: {e}"))?;
            let friendly_name = name_pwstr.to_string().unwrap_or_default();
            CoTaskMemFree(Some(name_pwstr.0.cast()));

            Ok(EndpointInfo { id, friendly_name })
        }
    }

    pub fn enumerate_active_endpoints(capture: bool) -> Result<Vec<EndpointInfo>, String> {
        let _com = ComGuard::init();
        let flow: EDataFlow = if capture { eCapture } else { eRender };
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("MMDeviceEnumerator failed: {e}"))?;
            let collection = enumerator
                .EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)
                .map_err(|e| format!("EnumAudioEndpoints failed: {e}"))?;
            let count = collection
                .GetCount()
                .map_err(|e| format!("GetCount failed: {e}"))?;
            let mut out = Vec::with_capacity(count as usize);
            for i in 0..count {
                if let Ok(dev) = collection.Item(i) {
                    if let Ok(info) = device_info(&dev) {
                        out.push(info);
                    }
                }
            }
            Ok(out)
        }
    }

    pub fn get_default_endpoint(capture: bool, comms: bool) -> Option<EndpointInfo> {
        let _com = ComGuard::init();
        let flow = if capture { eCapture } else { eRender };
        let role = if comms { eCommunications } else { eConsole };
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let dev = enumerator.GetDefaultAudioEndpoint(flow, role).ok()?;
            device_info(&dev).ok()
        }
    }

    pub fn set_default_endpoint_roles(device_id: &str, roles: &[ERole]) -> Result<(), String> {
        let _com = ComGuard::init();
        unsafe {
            let policy: IPolicyConfig =
                CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
                    .map_err(|e| format!("IPolicyConfig failed: {e}"))?;
            let hstr = HSTRING::from(device_id);
            for &role in roles {
                let hr = policy.set_default_endpoint(PCWSTR(hstr.as_ptr()), role);
                if hr.is_err() {
                    return Err(format!("SetDefaultEndpoint({role:?}) failed: {hr:?}"));
                }
            }
            Ok(())
        }
    }

    pub fn set_default_all_roles(device_id: &str) -> Result<(), String> {
        set_default_endpoint_roles(device_id, &[eConsole, eMultimedia, eCommunications])
    }

    pub fn set_default_console_and_comms(
        console_id: Option<&str>,
        comms_id: Option<&str>,
    ) -> Result<(), String> {
        if let Some(id) = console_id {
            set_default_endpoint_roles(id, &[eConsole, eMultimedia])?;
        }
        if let Some(id) = comms_id {
            set_default_endpoint_roles(id, &[eCommunications])?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod os_policy {
    use super::*;
    use crate::virtual_mic::linux_audio;

    pub fn enumerate_active_endpoints(capture: bool) -> Result<Vec<EndpointInfo>, String> {
        linux_audio::enumerate_active_endpoints(capture)
    }
    pub fn get_default_endpoint(capture: bool, comms: bool) -> Option<EndpointInfo> {
        linux_audio::get_default_endpoint(capture, comms)
    }
    pub fn set_default_all_roles(device_id: &str) -> Result<(), String> {
        linux_audio::set_default_endpoint(true, device_id)
    }
    pub fn set_default_console_and_comms(
        capture: bool,
        console_id: Option<&str>,
        comms_id: Option<&str>,
    ) -> Result<(), String> {
        if let Some(id) = console_id.or(comms_id) {
            linux_audio::set_default_endpoint(capture, id)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod os_policy {
    pub use super::win_policy::{
        enumerate_active_endpoints, get_default_endpoint, set_default_all_roles,
    };

    pub fn set_default_console_and_comms(
        _capture: bool,
        console_id: Option<&str>,
        comms_id: Option<&str>,
    ) -> Result<(), String> {
        super::win_policy::set_default_console_and_comms(console_id, comms_id)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod os_policy {
    use super::*;
    pub fn enumerate_active_endpoints(_capture: bool) -> Result<Vec<EndpointInfo>, String> {
        Ok(Vec::new())
    }
    pub fn get_default_endpoint(_capture: bool, _comms: bool) -> Option<EndpointInfo> {
        None
    }
    pub fn set_default_all_roles(_device_id: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn set_default_console_and_comms(
        _capture: bool,
        _console_id: Option<&str>,
        _comms_id: Option<&str>,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub use os_policy::{enumerate_active_endpoints, get_default_endpoint};

/// Snapshots current real (non-virtual) physical defaults into `mic_restore.json`
/// without overwriting an earlier active crash-recovery snapshot.
pub fn snapshot_physical_defaults_before_attach() -> MicRestoreState {
    let path = restore_file_path();
    let existing = load_restore_state(&path);

    let current_cap_console = get_default_endpoint(true, false);
    let current_cap_comms = get_default_endpoint(true, true);
    let current_ren_console = get_default_endpoint(false, false);
    let current_ren_comms = get_default_endpoint(false, true);

    let active_endpoints = enumerate_active_endpoints(true).unwrap_or_default();
    let endpoint_exists = |id: &str| active_endpoints.iter().any(|e| e.id == id);

    // Preserve previous physical capture mic if previous run crashed while switched
    let mut cap_console_id = current_cap_console
        .as_ref()
        .filter(|e| !is_virtual_audio_name(&e.friendly_name))
        .map(|e| e.id.clone());
    let mut cap_console_name = current_cap_console
        .as_ref()
        .filter(|e| !is_virtual_audio_name(&e.friendly_name))
        .map(|e| e.friendly_name.clone());
    let mut cap_comms_id = current_cap_comms
        .as_ref()
        .filter(|e| !is_virtual_audio_name(&e.friendly_name))
        .map(|e| e.id.clone());

    if let Some(prev) = existing.as_ref().filter(|s| s.active) {
        if cap_console_id.is_none() {
            if let Some(prev_id) = prev.original_capture_console_id.as_ref() {
                if endpoint_exists(prev_id) {
                    cap_console_id = Some(prev_id.clone());
                    cap_console_name = prev.original_capture_name.clone();
                }
            }
        }
        if cap_comms_id.is_none() {
            if let Some(prev_id) = prev.original_capture_comms_id.as_ref() {
                if endpoint_exists(prev_id) {
                    cap_comms_id = Some(prev_id.clone());
                }
            }
        }
    }

    // Fallback to first non-virtual physical mic if default is currently virtual
    if cap_console_id.is_none() {
        if let Some(phys) = active_endpoints
            .iter()
            .find(|e| !is_virtual_audio_name(&e.friendly_name))
        {
            cap_console_id = Some(phys.id.clone());
            cap_console_name = Some(phys.friendly_name.clone());
            if cap_comms_id.is_none() {
                cap_comms_id = Some(phys.id.clone());
            }
        }
    }

    let render_endpoints = enumerate_active_endpoints(false).unwrap_or_default();
    let render_exists = |id: &str| render_endpoints.iter().any(|e| e.id == id);
    let ren_console_id = current_ren_console
        .as_ref()
        .filter(|e| !is_virtual_audio_name(&e.friendly_name))
        .map(|e| e.id.clone())
        .or_else(|| {
            existing
                .as_ref()
                .and_then(|p| p.original_render_console_id.clone())
                .filter(|id| render_exists(id))
        });
    let ren_comms_id = current_ren_comms
        .as_ref()
        .filter(|e| !is_virtual_audio_name(&e.friendly_name))
        .map(|e| e.id.clone())
        .or_else(|| {
            existing
                .as_ref()
                .and_then(|p| p.original_render_comms_id.clone())
                .filter(|id| render_exists(id))
        });

    let state = MicRestoreState {
        active: existing.as_ref().is_some_and(|s| s.active),
        original_capture_console_id: cap_console_id,
        original_capture_comms_id: cap_comms_id,
        original_capture_name: cap_console_name,
        original_render_console_id: ren_console_id,
        original_render_comms_id: ren_comms_id,
        switched_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    let _ = save_restore_state(&path, &state);
    state
}

/// Ensures Windows did not accidentally switch the user's speakers/headphones
/// to our virtual device when the UAC2 endpoint arrived.
pub fn guard_playback_defaults(snapshot: &MicRestoreState) {
    let fallback_render = || {
        enumerate_active_endpoints(false)
            .ok()
            .and_then(|eps| {
                eps.into_iter()
                    .find(|e| !is_virtual_audio_name(&e.friendly_name))
                    .map(|e| e.id)
            })
    };

    if let Some(cur) = get_default_endpoint(false, false) {
        if is_virtual_audio_name(&cur.friendly_name) {
            let target = snapshot
                .original_render_console_id
                .clone()
                .or_else(fallback_render);
            if let Some(orig) = target.as_deref() {
                let _ = os_policy::set_default_console_and_comms(false, Some(orig), None);
            }
        }
    }
    if let Some(cur) = get_default_endpoint(false, true) {
        if is_virtual_audio_name(&cur.friendly_name) {
            let target = snapshot
                .original_render_comms_id
                .clone()
                .or_else(fallback_render);
            if let Some(orig) = target.as_deref() {
                let _ = os_policy::set_default_console_and_comms(false, None, Some(orig));
            }
        }
    }
}

/// Switches Windows Default Recording + Communications microphone to `Memeboard Virtual Mic`.
pub fn switch_default_mic_to_virtual(snapshot: &MicRestoreState) -> Result<String, String> {
    let capture_endpoints = enumerate_active_endpoints(true)?;
    let virtual_ep = capture_endpoints
        .iter()
        .find(|e| e.friendly_name.to_lowercase().contains("memeboard virtual mic"))
        .or_else(|| {
            capture_endpoints
                .iter()
                .find(|e| is_virtual_audio_name(&e.friendly_name))
        })
        .ok_or_else(|| "Memeboard Virtual Mic recording endpoint not found in Windows".to_string())?;

    let mut updated = snapshot.clone();
    updated.active = true;
    updated.switched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    save_restore_state(&restore_file_path(), &updated)?;

    os_policy::set_default_all_roles(&virtual_ep.id)?;
    guard_playback_defaults(&updated);
    Ok(virtual_ep.friendly_name.clone())
}

/// Restores the user's original physical microphone as Windows Default Recording & Communications.
pub fn restore_original_mic() -> Result<(), String> {
    let path = restore_file_path();
    let mut state = load_restore_state(&path).unwrap_or_default();

    if !state.active && !is_virtual_mic_currently_default() {
        return Ok(());
    }

    let capture_endpoints = enumerate_active_endpoints(true).unwrap_or_default();
    let exists = |id: &str| capture_endpoints.iter().any(|e| e.id == id);

    let target_console = state
        .original_capture_console_id
        .as_deref()
        .filter(|id| exists(id))
        .map(str::to_string)
        .or_else(|| {
            capture_endpoints
                .iter()
                .find(|e| !is_virtual_audio_name(&e.friendly_name))
                .map(|e| e.id.clone())
        });

    let target_comms = state
        .original_capture_comms_id
        .as_deref()
        .filter(|id| exists(id))
        .map(str::to_string)
        .or_else(|| target_console.clone());

    if target_console.is_some() || target_comms.is_some() {
        os_policy::set_default_console_and_comms(
            true,
            target_console.as_deref(),
            target_comms.as_deref(),
        )?;
    }

    state.active = false;
    let _ = save_restore_state(&path, &state);
    Ok(())
}

pub fn is_virtual_mic_currently_default() -> bool {
    get_default_endpoint(true, false).is_some_and(|e| {
        let f = e.friendly_name.to_lowercase();
        let id = e.id.to_lowercase();
        f.contains("memeboard virtual mic") || id.contains("memeboard_virtual_mic")
    })
}

pub fn saved_original_physical_mic_name() -> Option<String> {
    load_restore_state(&restore_file_path()).and_then(|s| s.original_capture_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_name_detection_avoids_nahimic() {
        assert!(is_virtual_audio_name("Microphone (Memeboard Virtual Mic)"));
        assert!(is_virtual_audio_name("CABLE Output (VB-Audio Virtual Cable)"));
        assert!(!is_virtual_audio_name("Sonic Studio Virtual Mixer"));
        assert!(!is_virtual_audio_name("Microphone (4- MMX 150)"));
    }

    #[test]
    fn restore_state_json_round_trip() {
        let dir = std::env::temp_dir().join(format!("memeboard-policy-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("mic_restore.json");
        let st = MicRestoreState {
            active: true,
            original_capture_console_id: Some("{0.0.1}.{abc}".into()),
            original_capture_comms_id: Some("{0.0.1}.{abc}".into()),
            original_capture_name: Some("Microphone (4- MMX 150)".into()),
            original_render_console_id: Some("{0.0.0}.{def}".into()),
            original_render_comms_id: None,
            switched_at: 12345,
        };
        save_restore_state(&path, &st).unwrap();
        let loaded = load_restore_state(&path).unwrap();
        assert!(loaded.active);
        assert_eq!(loaded.original_capture_name.as_deref(), Some("Microphone (4- MMX 150)"));
        let _ = fs::remove_dir_all(&dir);
    }
}
