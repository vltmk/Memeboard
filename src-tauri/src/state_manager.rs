use crate::decoder::{
    compute_normalization_gain_range, decode_audio_file_with_lufs,
};
use crate::mixer::AudioMixer;
use crate::types::{
    AudioSettings, MemeboardPackManifest, PackImportResult, PackSoundTemplate, PlaybackMode,
    SoundItem, SoundboardState,
};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

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
                categories: Vec::new(),
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
                    categories: Vec::new(),
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

    /// Resolves a stored audio file strictly inside `self.sounds_dir`.
    fn resolve_stored_sound_path(&self, file_path: &str) -> Result<PathBuf, String> {
        let target = Path::new(file_path);
        let canon_dir = fs::canonicalize(&self.sounds_dir)
            .map_err(|e| format!("Failed to resolve sounds directory: {e}"))?;
        let canon_target = fs::canonicalize(target)
            .map_err(|e| format!("Audio file is unavailable: {e}"))?;
        if !canon_target.starts_with(&canon_dir) {
            return Err("Security check failed: audio file is outside Memeboard sounds directory".to_string());
        }
        Ok(canon_target)
    }

    /// Exports a single managed audio clip to a user-selected destination path.
    pub fn export_sound_file(&self, file_path: &str, dest_path: &str) -> Result<(), String> {
        let src = self.resolve_stored_sound_path(file_path)?;
        let dest = Path::new(dest_path);
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create destination folder: {e}"))?;
            }
        }
        fs::copy(&src, dest).map_err(|e| format!("Failed to export audio file: {e}"))?;
        Ok(())
    }

    /// Exports sounds (optionally filtered by category) along with their Memeboard templates into a
    /// portable `.memeboard` / `.zip` archive containing `memeboard.json` and `audio/*`.
    pub fn export_pack(
        &self,
        state: &SoundboardState,
        dest_path: &str,
        category_filter: Option<&str>,
    ) -> Result<usize, String> {
        let filter_norm = category_filter
            .map(|c| c.trim())
            .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("All"));

        let selected: Vec<&SoundItem> = state
            .sounds
            .iter()
            .filter(|s| match filter_norm {
                Some(cat) => {
                    let s_cat = if s.category.trim().is_empty() {
                        "General"
                    } else {
                        s.category.trim()
                    };
                    s_cat.eq_ignore_ascii_case(cat)
                }
                None => true,
            })
            .collect();

        if selected.is_empty() {
            return Err("No sounds matched the selected category to export".to_string());
        }

        let dest = Path::new(dest_path);
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create export folder: {e}"))?;
            }
        }

        let file = File::create(dest).map_err(|e| format!("Failed to create pack file: {e}"))?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated);

        let mut templates = Vec::with_capacity(selected.len());
        let mut used_categories = HashSet::new();
        let mut buf = Vec::new();

        for (idx, sound) in selected.iter().enumerate() {
            let src = self.resolve_stored_sound_path(&sound.file_path)?;
            let ext = src
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or_else(|| "wav".to_string());

            let clean_name = sanitize_filename(&sound.name);
            let archive_rel = format!("audio/{:03}-{}.{}", idx + 1, clean_name, ext);

            buf.clear();
            File::open(&src)
                .and_then(|mut f| f.read_to_end(&mut buf))
                .map_err(|e| format!("Failed to read '{}' for export: {e}", sound.name))?;

            zip.start_file(&archive_rel, options)
                .map_err(|e| format!("Failed to write pack entry: {e}"))?;
            zip.write_all(&buf)
                .map_err(|e| format!("Failed to write pack audio bytes: {e}"))?;

            let cat = if sound.category.trim().is_empty() {
                "General".to_string()
            } else {
                sound.category.trim().to_string()
            };
            used_categories.insert(cat.clone());

            templates.push(PackSoundTemplate {
                name: sound.name.clone(),
                emoji: sound.emoji.clone(),
                category: cat,
                volume: sound.volume,
                hotkey: sound.hotkey.clone(),
                trim_start: sound.trim_start,
                trim_end: sound.trim_end,
                playback_mode: sound.playback_mode,
                audio_file: archive_rel,
            });
        }

        if filter_norm.is_none() {
            for cat in &state.categories {
                let c = cat.trim();
                if !c.is_empty() {
                    used_categories.insert(c.to_string());
                }
            }
        }
        let mut categories: Vec<String> = used_categories.into_iter().collect();
        categories.sort();

        let exported_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let manifest = MemeboardPackManifest {
            format_version: 1,
            app: "Memeboard".to_string(),
            exported_at,
            categories,
            sounds: templates,
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("Failed to serialize pack manifest: {e}"))?;

        zip.start_file("memeboard.json", options)
            .map_err(|e| format!("Failed to write memeboard.json: {e}"))?;
        zip.write_all(manifest_json.as_bytes())
            .map_err(|e| format!("Failed to write memeboard.json contents: {e}"))?;

        zip.finish()
            .map_err(|e| format!("Failed to finalize pack archive: {e}"))?;

        Ok(selected.len())
    }

    /// Imports a `.memeboard` / `.zip` pack (with `memeboard.json` templates, or a plain zip of audio files).
    pub fn import_pack(
        &self,
        pack_path: &str,
        target_lufs: f32,
        mixer: &AudioMixer,
    ) -> Result<PackImportResult, String> {
        let file = File::open(pack_path).map_err(|e| format!("Failed to open pack: {e}"))?;
        let mut archive =
            ZipArchive::new(file).map_err(|e| format!("Invalid pack archive: {e}"))?;

        let temp_dir = std::env::temp_dir().join(format!("memeboard-pack-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temporary unpack folder: {e}"))?;
        let _cleanup = TempDirGuard(temp_dir.clone());

        let manifest_raw = {
            match archive.by_name("memeboard.json") {
                Ok(mut entry) => {
                    let mut text = String::new();
                    entry
                        .read_to_string(&mut text)
                        .map_err(|e| format!("Failed to read memeboard.json: {e}"))?;
                    Some(text)
                }
                Err(_) => None,
            }
        };

        let mut imported_sounds = Vec::new();
        let mut imported_categories_set = HashSet::new();
        let mut skipped_count = 0usize;
        let mut warnings = Vec::new();

        if let Some(json) = manifest_raw {
            let manifest: MemeboardPackManifest = serde_json::from_str(&json)
                .map_err(|e| format!("Invalid memeboard.json manifest: {e}"))?;

            for cat in manifest.categories {
                let c = cat.trim().to_string();
                if !c.is_empty() {
                    imported_categories_set.insert(c);
                }
            }

            for (idx, tpl) in manifest.sounds.into_iter().enumerate() {
                let normalized_entry = normalize_archive_rel_path(&tpl.audio_file);
                let Some(rel_path) = normalized_entry else {
                    skipped_count += 1;
                    warnings.push(format!("Skipped unsafe path for '{}'", tpl.name));
                    continue;
                };

                let alt = rel_path.replace('/', "\\");
                let entry_idx = archive
                    .index_for_name(&rel_path)
                    .or_else(|| archive.index_for_name(&alt));
                let Some(entry_idx) = entry_idx else {
                    skipped_count += 1;
                    warnings.push(format!("Missing audio file '{}' for '{}'", rel_path, tpl.name));
                    continue;
                };
                let mut entry = archive
                    .by_index(entry_idx)
                    .map_err(|e| format!("Failed to read archive entry '{}': {e}", rel_path))?;

                let ext = Path::new(&rel_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .filter(|e| is_supported_audio_ext(e))
                    .unwrap_or_else(|| "wav".to_string());

                let temp_audio = temp_dir.join(format!("item-{idx}.{ext}"));
                {
                    let mut out = File::create(&temp_audio)
                        .map_err(|e| format!("Failed to extract '{}': {e}", tpl.name))?;
                    std::io::copy(&mut entry, &mut out)
                        .map_err(|e| format!("Failed to write extracted '{}': {e}", tpl.name))?;
                }

                match self.import_sound_file(
                    &temp_audio.to_string_lossy(),
                    Some(tpl.name.clone()),
                    Some(tpl.emoji.clone()),
                    Some(tpl.category.clone()),
                    target_lufs,
                    mixer,
                ) {
                    Ok(mut item) => {
                        item.volume = tpl.volume.clamp(0.0, 2.0);
                        item.playback_mode = tpl.playback_mode;
                        item.hotkey = tpl.hotkey.filter(|h| !h.trim().is_empty());

                        let max_dur = item.duration.max(0.05);
                        let start = tpl.trim_start.clamp(0.0, (max_dur - 0.05).max(0.0));
                        let end = if tpl.trim_end > start + 0.05 {
                            tpl.trim_end.min(max_dur)
                        } else {
                            max_dur
                        };
                        item.trim_start = start;
                        item.trim_end = end;

                        if start > 0.005 || (max_dur - end).abs() > 0.005 {
                            if let Ok(gain) = self.recompute_normalization(
                                &item,
                                start,
                                end,
                                target_lufs,
                                mixer,
                            ) {
                                item.normalized_gain = gain;
                            }
                        }

                        if !item.category.trim().is_empty() {
                            imported_categories_set.insert(item.category.trim().to_string());
                        }
                        imported_sounds.push(item);
                    }
                    Err(e) => {
                        skipped_count += 1;
                        warnings.push(format!("Could not import '{}': {e}", tpl.name));
                    }
                }
            }
        } else {
            // Plain zip containing raw audio files without manifest
            for idx in 0..archive.len() {
                let mut entry = match archive.by_index(idx) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if entry.is_dir() {
                    continue;
                }
                let entry_name = entry.name().to_string();
                let Some(rel) = normalize_archive_rel_path(&entry_name) else {
                    continue;
                };
                let ext_opt = Path::new(&rel)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .filter(|e| is_supported_audio_ext(e));
                let Some(ext) = ext_opt else {
                    continue;
                };
                let stem = Path::new(&rel)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("Sound {}", idx + 1));

                let temp_audio = temp_dir.join(format!("raw-{idx}.{ext}"));
                {
                    let mut out = match File::create(&temp_audio) {
                        Ok(f) => f,
                        Err(e) => {
                            skipped_count += 1;
                            warnings.push(format!("Failed to unpack '{stem}': {e}"));
                            continue;
                        }
                    };
                    if let Err(e) = std::io::copy(&mut entry, &mut out) {
                        skipped_count += 1;
                        warnings.push(format!("Failed to unpack '{stem}': {e}"));
                        continue;
                    }
                }

                match self.import_sound_file(
                    &temp_audio.to_string_lossy(),
                    Some(stem.clone()),
                    None,
                    None,
                    target_lufs,
                    mixer,
                ) {
                    Ok(item) => {
                        if !item.category.trim().is_empty() {
                            imported_categories_set.insert(item.category.trim().to_string());
                        }
                        imported_sounds.push(item);
                    }
                    Err(e) => {
                        skipped_count += 1;
                        warnings.push(format!("Could not import '{stem}': {e}"));
                    }
                }
            }
        }

        let mut imported_categories: Vec<String> = imported_categories_set.into_iter().collect();
        imported_categories.sort();

        Ok(PackImportResult {
            imported_sounds,
            imported_categories,
            skipped_count,
            warnings,
        })
    }
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn is_supported_audio_ext(ext: &str) -> bool {
    matches!(
        ext,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" | "webm"
    )
}

fn normalize_archive_rel_path(raw: &str) -> Option<String> {
    let slash_norm = raw.replace('\\', "/");
    let p = Path::new(&slash_norm);
    let mut parts = Vec::new();
    for comp in p.components() {
        match comp {
            Component::Normal(s) => {
                parts.push(s.to_str()?.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "sound".to_string()
    } else {
        trimmed.chars().take(64).collect()
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

    #[test]
    fn pack_export_and_import_round_trip_preserves_templates() {
        let base = std::env::temp_dir().join(format!("memeboard_pack_test_{}", Uuid::new_v4()));
        let sounds_dir = base.join("sounds");
        fs::create_dir_all(&sounds_dir).unwrap();

        let wav_path = base.join("src.wav");
        {
            let mut f = File::create(&wav_path).unwrap();
            let frames = 4800u32;
            let data_len = frames * 2;
            f.write_all(b"RIFF").unwrap();
            f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
            f.write_all(b"WAVEfmt ").unwrap();
            f.write_all(&16u32.to_le_bytes()).unwrap();
            f.write_all(&1u16.to_le_bytes()).unwrap();
            f.write_all(&1u16.to_le_bytes()).unwrap();
            f.write_all(&48000u32.to_le_bytes()).unwrap();
            f.write_all(&(48000u32 * 2).to_le_bytes()).unwrap();
            f.write_all(&2u16.to_le_bytes()).unwrap();
            f.write_all(&16u16.to_le_bytes()).unwrap();
            f.write_all(b"data").unwrap();
            f.write_all(&data_len.to_le_bytes()).unwrap();
            for i in 0..frames {
                let s = ((i as f32 * 0.05).sin() * 12000.0) as i16;
                f.write_all(&s.to_le_bytes()).unwrap();
            }
        }

        let sm = StateManager {
            data_dir: base.clone(),
            sounds_dir: sounds_dir.clone(),
            state_file: base.join("state.json"),
        };
        let mixer = AudioMixer::new();
        let mut imported = sm
            .import_sound_file(
                &wav_path.to_string_lossy(),
                Some("Test Tone".into()),
                Some("⚡".into()),
                Some("Voice".into()),
                -16.0,
                &mixer,
            )
            .unwrap();
        imported.volume = 0.65;
        imported.playback_mode = PlaybackMode::Stop;
        imported.hotkey = Some("F9".into());

        let state = SoundboardState {
            sounds: vec![imported.clone()],
            settings: AudioSettings::default(),
            categories: vec!["Voice".into(), "EmptyCat".into()],
        };

        let pack_file = base.join("test.memeboard");
        let count = sm
            .export_pack(&state, &pack_file.to_string_lossy(), None)
            .unwrap();
        assert_eq!(count, 1);
        assert!(pack_file.exists());

        let exported_single = base.join("single.wav");
        sm.export_sound_file(&imported.file_path, &exported_single.to_string_lossy())
            .unwrap();
        assert!(exported_single.exists());

        let res = sm
            .import_pack(&pack_file.to_string_lossy(), -16.0, &mixer)
            .unwrap();
        assert_eq!(res.imported_sounds.len(), 1);
        assert_eq!(res.skipped_count, 0);
        let restored = &res.imported_sounds[0];
        assert_eq!(restored.name, "Test Tone");
        assert_eq!(restored.emoji, "⚡");
        assert_eq!(restored.category, "Voice");
        assert!((restored.volume - 0.65).abs() < 1e-4);
        assert_eq!(restored.playback_mode, PlaybackMode::Stop);
        assert_eq!(restored.hotkey.as_deref(), Some("F9"));
        assert!(res.imported_categories.contains(&"EmptyCat".to_string()));

        let _ = fs::remove_dir_all(&base);
    }
}
