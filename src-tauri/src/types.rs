use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackMode {
    Restart,
    Stop,
    Overlap,
}

impl Default for PlaybackMode {
    fn default() -> Self {
        PlaybackMode::Restart
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundItem {
    pub id: String,
    pub name: String,
    pub emoji: String,
    pub file_path: String,
    pub category: String,
    pub volume: f32,
    pub hotkey: Option<String>,
    pub trim_start: f32,
    pub trim_end: f32,
    pub duration: f32,
    pub peaks: Vec<f32>,
    pub playback_mode: PlaybackMode,
    pub normalized_gain: f32,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub is_input: bool,
    #[serde(default)]
    pub is_virtual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevicesResult {
    pub input_devices: Vec<AudioDevice>,
    pub output_devices: Vec<AudioDevice>,
    pub current_input: Option<String>,
    pub current_broadcast: Option<String>,
    pub current_monitor: Option<String>,
}

fn default_auto_switch() -> bool {
    true
}

fn default_aec_safe_monitor() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettings {
    #[serde(default)]
    pub schema_version: u32,
    pub mic_device_id: Option<String>,
    pub broadcast_device_id: Option<String>,
    pub monitor_device_id: Option<String>,
    pub mic_volume: f32,
    pub monitor_volume: f32,
    pub broadcast_volume: f32,
    pub monitor_muted: bool,
    pub broadcast_muted: bool,
    pub voice_ducking_enabled: bool,
    pub voice_ducking_db: f32,
    pub loudness_target_lufs: f32,
    pub panic_hotkey: String,
    pub overlay_hotkey: String,
    #[serde(default = "default_auto_switch")]
    pub auto_switch_default_mic: bool,
    #[serde(default = "default_aec_safe_monitor")]
    pub aec_safe_monitor: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mic_device_id: None,
            broadcast_device_id: None,
            monitor_device_id: None,
            mic_volume: 1.0,
            monitor_volume: 0.8,
            broadcast_volume: 1.0,
            monitor_muted: false,
            broadcast_muted: false,
            voice_ducking_enabled: true,
            voice_ducking_db: -10.0,
            loudness_target_lufs: -14.0,
            panic_hotkey: "Control+Shift+X".to_string(),
            overlay_hotkey: "Control+Shift+S".to_string(),
            auto_switch_default_mic: true,
            aec_safe_monitor: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundboardState {
    pub sounds: Vec<SoundItem>,
    pub settings: AudioSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPlaybackStatus {
    pub playing_sound_ids: Vec<String>,
    pub active_clip_count: usize,
    pub mic_active: bool,
    pub mic_stream_active: bool,
    pub broadcast_stream_active: bool,
    pub monitor_stream_active: bool,
    pub last_audio_error: Option<String>,
}
