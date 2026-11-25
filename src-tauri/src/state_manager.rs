use crate::decoder::{
    compute_normalization_gain_range, decode_audio_file_with_lufs,
};
use crate::mixer::AudioMixer;
use crate::types::{AudioSettings, PlaybackMode, SoundItem, SoundboardState};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const CURRENT_SCHEMA_VERSION: u32 = 1;
const LEGACY_DEMO_TIMESTAMP: u64 = 1_700_000_000;
const LEGACY_DEMO_IDS: [&str; 5] = ["airhorn", "boom", "bruh", "quack", "horn"];

pub struct StateManager {
    pub data_dir: PathBuf,
    pub sounds_dir: PathBuf,
    pub state_file: PathBuf,
}

impl StateManager {
    pub fn new() -> Result<Self, String> {
        let base = dirs::data_local_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".memeboard")))
            .ok_or_else(|| "Could not determine application data directory".to_string())?;
        let data_dir = base.join("Memeboard");
        let sounds_dir = data_dir.join("sounds");
        let state_file = data_dir.join("state.json");

        fs::create_dir_all(&sounds_dir)
            .map_err(|e| format!("Failed to create Memeboard directories: {e}"))?;

        Ok(Self {
            data_dir,
            sounds_dir,
            state_file,
        })
    }

    pub fn load_state(&self) -> Result<SoundboardState, String> {
        if !self.state_file.exists() {
            let clean = SoundboardState {
                sounds: Vec::new(),
                settings: AudioSettings::default(),
            };
            self.save_state(&clean)?;
            return Ok(clean);
        }

        let raw = fs::read_to_string(&self.state_file)
            .map_err(|e| format!("Failed to read state.json: {e}"))?;

        let mut state: SoundboardState = match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(e) => {
                // Back up unreadable state rather than silently discarding it
                let backup = self.data_dir.join("state.json.corrupt");
                let _ = fs::copy(&self.state_file, &backup);
                eprintln!("[Memeboard] Warning: corrupted state.json ({e}); starting fresh.");
                SoundboardState {
                    sounds: Vec::new(),
                    settings: AudioSettings::default(),
                }
            }
        };

        let mut migrated = false;
        if state.settings.schema_version < CURRENT_SCHEMA_VERSION {
            // Migrate old Escape panic default -> non-conflicting Control+Shift+X
            if state.settings.panic_hotkey.eq_ignore_ascii_case("Escape") {
                state.settings.panic_hotkey = "Control+Shift+X".to_string();
            }

            // Remove only the 5 legacy synthetic demo clips; preserve all user sounds
            let mut removed_paths = Vec::new();
            state.sounds.retain(|s| {
                let is_legacy_demo = s.created_at == LEGACY_DEMO_TIMESTAMP
                    && LEGACY_DEMO_IDS.contains(&s.id.as_str());
                if is_legacy_demo {
                    removed_paths.push(s.file_path.clone());
                    false
                } else {
                    true
                }
            });
            for p in removed_paths {
                let _ = self.delete_sound_file(&p);
            }

            state.settings.schema_version = CURRENT_SCHEMA_VERSION;
            migrated = true;
        }

        if migrated {
            let _ = self.save_state(&state);
        }

        Ok(state)
    }

    pub fn save_state(&self, state: &SoundboardState) -> Result<(), String> {
        fs::create_dir_all(&self.data_dir)
            .map_err(|e| format!("Failed to create data directory: {e}"))?;
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| format!("Failed to serialize state: {e}"))?;

        let tmp_file = self.data_dir.join("state.json.tmp");
        fs::write(&tmp_file, json).map_err(|e| format!("Failed to write state temp file: {e}"))?;
        if let Err(rename_err) = fs::rename(&tmp_file, &self.state_file) {
            // Windows rename can fail briefly if AV/indexer holds state.json; fall back to direct write
            fs::write(
                &self.state_file,
                fs::read(&tmp_file).unwrap_or_default(),
            )
            .map_err(|e| format!("Failed to persist state.json ({rename_err}): {e}"))?;
            let _ = fs::remove_file(&tmp_file);
        }
        Ok(())
    }

    /// Preloads all user sounds in a background thread so UI startup is instantaneous.
    pub fn spawn_preload_all(&self, state: SoundboardState, mixer: Arc<AudioMixer>) {
        std::thread::Builder::new()
            .name("memeboard-preload".to_string())
            .spawn(move || {
                let target_lufs = state.settings.loudness_target_lufs;
                for item in state.sounds {
                    if mixer.has_sound_loaded(&item.id) {
                        continue;
                    }
                    match decode_audio_file_with_lufs(&item.file_path, target_lufs) {
                        Ok(mut decoded) => {
                            decoded.normalized_gain = compute_normalization_gain_range(
                                &decoded.samples,
                                item.trim_start,
                                item.trim_end,
                                target_lufs,
                            );
                            mixer.load_sound(item.id, decoded);
                        }
                        Err(e) => {
                            eprintln!(
                                "[Memeboard] Preload decode skipped for '{}' ({}): {}",
                                item.name, item.file_path, e
                            );
                        }
                    }
                }
            })
            .ok();
    }

    /// Ensures a single sound is decoded and loaded into `mixer` (used on-demand before playback).
    pub fn ensure_sound_loaded(
        sound: &SoundItem,
        target_lufs: f32,
        mixer: &AudioMixer,
    ) -> Result<(), String> {
        if mixer.has_sound_loaded(&sound.id) {
            return Ok(());
        }
        let mut decoded = decode_audio_file_with_lufs(&sound.file_path, target_lufs)?;
        decoded.normalized_gain = compute_normalization_gain_range(
            &decoded.samples,
            sound.trim_start,
            sound.trim_end,
            target_lufs,
        );
        mixer.load_sound(sound.id.clone(), decoded);
        Ok(())
    }

    pub fn import_sound_file(
        &self,
        source_path: &str,
        name: Option<String>,
        emoji: Option<String>,
        category: Option<String>,
        target_lufs: f32,
        mixer: &AudioMixer,
    ) -> Result<SoundItem, String> {
        let src = Path::new(source_path);
        if !src.is_file() {
            return Err(format!("Selected file does not exist: {source_path}"));
        }

        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or_else(|| "wav".to_string());

        let id = Uuid::new_v4().to_string();
        let dest = self.sounds_dir.join(format!("{id}.{ext}"));

        fs::copy(src, &dest).map_err(|e| format!("Failed to copy sound file: {e}"))?;

        let decoded = match decode_audio_file_with_lufs(&dest, target_lufs) {
            Ok(d) => d,
            Err(e) => {
                let _ = fs::remove_file(&dest);
                return Err(format!("Failed to decode audio file: {e}"));
            }
        };

        let duration = decoded.duration;
        let peaks = decoded.peaks.clone();
        let normalized_gain = decoded.normalized_gain;

        mixer.load_sound(id.clone(), decoded);

        let fallback_name = src
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Untitled Sound".to_string());

        let final_name = name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or(fallback_name);

        let final_emoji = emoji
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "🔊".to_string());

        let final_category = category
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "General".to_string());

        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(SoundItem {
            id,
            name: final_name,
            emoji: final_emoji,
            file_path: dest.to_string_lossy().to_string(),
            category: final_category,
            volume: 1.0,
            hotkey: None,
            trim_start: 0.0,
            trim_end: duration,
            duration,
            peaks,
            playback_mode: PlaybackMode::Restart,
            normalized_gain,
            created_at,
        })
    }

    pub fn recompute_normalization(
        &self,
        sound: &SoundItem,
        trim_start: f32,
        trim_end: f32,
        target_lufs: f32,
        mixer: &AudioMixer,
    ) -> Result<f32, String> {
        let gain = if let Some(audio) = mixer.get_loaded_audio(&sound.id) {
            compute_normalization_gain_range(&audio.samples, trim_start, trim_end, target_lufs)
        } else {
            let mut decoded = decode_audio_file_with_lufs(&sound.file_path, target_lufs)?;
            let gain = compute_normalization_gain_range(
                &decoded.samples,
                trim_start,
                trim_end,
                target_lufs,
            );
            decoded.normalized_gain = gain;
            mixer.load_sound(sound.id.clone(), decoded);
            gain
        };
        mixer.update_sound_gain(&sound.id, gain);
        Ok(gain)
    }

    /// Safely deletes an audio file ONLY if its canonical path resides within `self.sounds_dir`.
    pub fn delete_sound_file(&self, file_path: &str) -> Result<(), String> {
        let target = Path::new(file_path);
        let canon_dir = fs::canonicalize(&self.sounds_dir)
            .map_err(|e| format!("Failed to resolve sounds directory: {e}"))?;

        if !target.exists() {
            // Check parent directory when file already vanished so arbitrary paths still error
            if let Some(parent) = target.parent() {
                if let Ok(canon_parent) = fs::canonicalize(parent) {
                    if canon_parent.starts_with(&canon_dir) {
                        return Ok(());
                    }
                }
            }
            return Err("Refusing to remove path outside Memeboard sounds directory".to_string());
        }

        let canon_target = fs::canonicalize(target)
            .map_err(|e| format!("Failed to resolve sound path: {e}"))?;

        if !canon_target.starts_with(&canon_dir) {
            return Err(format!(
                "Security check failed: '{}' is outside Memeboard sounds directory",
                canon_target.display()
            ));
        }

        match fs::remove_file(&canon_target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Failed to delete sound file: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_sound_file_enforces_sounds_dir_containment() {
        let base = std::env::temp_dir().join(format!("memeboard_sm_test_{}", Uuid::new_v4()));
        let sounds_dir = base.join("sounds");
        fs::create_dir_all(&sounds_dir).unwrap();

        let outside = base.join("outside.wav");
        fs::write(&outside, b"keep-me").unwrap();
        let inside = sounds_dir.join("inside.wav");
        fs::write(&inside, b"delete-me").unwrap();

        let sm = StateManager {
            data_dir: base.clone(),
            sounds_dir: sounds_dir.clone(),
            state_file: base.join("state.json"),
        };

        assert!(sm.delete_sound_file(&outside.to_string_lossy()).is_err());
        assert!(outside.exists());

        assert!(sm.delete_sound_file(&inside.to_string_lossy()).is_ok());
        assert!(!inside.exists());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn load_state_migrates_legacy_demo_sounds_and_escape_hotkey() {
        let base = std::env::temp_dir().join(format!("memeboard_sm_mig_{}", Uuid::new_v4()));
        let sounds_dir = base.join("sounds");
        fs::create_dir_all(&sounds_dir).unwrap();

        let demo_path = sounds_dir.join("airhorn.wav");
        let user_path = sounds_dir.join("user_clip.wav");
        fs::write(&demo_path, b"demo").unwrap();
        fs::write(&user_path, b"user").unwrap();

        let legacy_json = serde_json::json!({
            "sounds": [
                {
                    "id": "airhorn",
                    "name": "Airhorn",
                    "emoji": "A",
                    "filePath": demo_path.to_string_lossy(),
                    "category": "Classic",
                    "volume": 1.0,
                    "hotkey": null,
                    "trimStart": 0.0,
                    "trimEnd": 1.0,
                    "duration": 1.0,
                    "peaks": [0.5],
                    "playbackMode": "restart",
                    "normalizedGain": 1.0,
                    "createdAt": 1700000000u64
                },
                {
                    "id": "custom-user-1",
                    "name": "My Clip",
                    "emoji": "M",
                    "filePath": user_path.to_string_lossy(),
                    "category": "Memes",
                    "volume": 1.0,
                    "hotkey": "F8",
                    "trimStart": 0.0,
                    "trimEnd": 2.0,
                    "duration": 2.0,
                    "peaks": [0.5],
                    "playbackMode": "restart",
                    "normalizedGain": 1.0,
                    "createdAt": 1750000000u64
                }
            ],
            "settings": {
                "micDeviceId": null,
                "broadcastDeviceId": null,
                "monitorDeviceId": null,
                "micVolume": 1.0,
                "monitorVolume": 0.8,
                "broadcastVolume": 1.0,
                "monitorMuted": false,
                "broadcastMuted": false,
                "voiceDuckingEnabled": true,
                "voiceDuckingDb": -10.0,
                "loudnessTargetLufs": -14.0,
                "panicHotkey": "Escape",
                "overlayHotkey": "Control+Shift+S"
            }
        });
        let state_file = base.join("state.json");
        fs::write(&state_file, serde_json::to_string_pretty(&legacy_json).unwrap()).unwrap();

        let sm = StateManager {
            data_dir: base.clone(),
            sounds_dir: sounds_dir.clone(),
            state_file,
        };
        let loaded = sm.load_state().unwrap();
        assert_eq!(loaded.settings.schema_version, 1);
        assert_eq!(loaded.settings.panic_hotkey, "Control+Shift+X");
        assert!(loaded.settings.auto_switch_default_mic);
        assert_eq!(loaded.sounds.len(), 1);
        assert_eq!(loaded.sounds[0].id, "custom-user-1");
        assert!(!demo_path.exists());
        assert!(user_path.exists());

        let _ = fs::remove_dir_all(&base);
    }
}
