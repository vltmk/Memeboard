use crate::decoder::DecodedAudio;
use crate::types::PlaybackMode;
use parking_lot::{Mutex, RwLock};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const MAX_ACTIVE_CLIPS: usize = 32;
pub const MAX_SOUND_SLOTS: usize = 1024;
const BITSET_WORDS: usize = MAX_SOUND_SLOTS / 64;

const VOICE_OPEN_THRESH: f32 = 0.018;
const VOICE_CLOSE_THRESH: f32 = 0.009;
const VOICE_HOLD_FRAMES: usize = (SAMPLE_RATE as usize) / 4; // 250 ms hold

#[derive(Debug, Clone)]
pub struct ActiveClip {
    pub slot_id: usize,
    pub sound_id: String,
    pub pos: usize,
    pub start_pos: usize,
    pub end_pos: usize,
    pub volume: f32,
    pub normalized_gain: f32,
    pub mode: PlaybackMode,
    pub preview_only: bool,
}

#[derive(Clone)]
pub enum StreamCmd {
    LoadSound {
        slot_id: usize,
        sound_id: String,
        audio: Arc<DecodedAudio>,
    },
    UnloadSound {
        sound_id: String,
    },
    TriggerSound {
        sound_id: String,
        trim_start: f32,
        trim_end: f32,
        volume: f32,
        mode: PlaybackMode,
        normalized_gain: f32,
        preview_only: bool,
    },
    StopSound {
        sound_id: String,
        preview_only_only: bool,
    },
    StopAll,
}

struct RegisteredSound {
    slot_id: usize,
    audio: Arc<DecodedAudio>,
}

/// Lock-free atomic parameters shared between UI/control threads and real-time audio callbacks.
pub struct SharedMixerControls {
    mic_volume_bits: AtomicU32,
    monitor_volume_bits: AtomicU32,
    broadcast_volume_bits: AtomicU32,
    monitor_muted: AtomicBool,
    broadcast_muted: AtomicBool,
    voice_ducking_enabled: AtomicBool,
    voice_ducking_db_bits: AtomicU32,
    aec_safe_monitor: AtomicBool,

    pub is_mic_active: AtomicBool,
    pub broadcast_stream_active: AtomicBool,
    pub monitor_stream_active: AtomicBool,
    pub bcast_active_count: AtomicUsize,
    pub mon_active_count: AtomicUsize,
    bcast_playing_bits: [AtomicU64; BITSET_WORDS],
    mon_playing_bits: [AtomicU64; BITSET_WORDS],
}

impl SharedMixerControls {
    fn new() -> Self {
        Self {
            mic_volume_bits: AtomicU32::new(1.0f32.to_bits()),
            monitor_volume_bits: AtomicU32::new(0.8f32.to_bits()),
            broadcast_volume_bits: AtomicU32::new(1.0f32.to_bits()),
            monitor_muted: AtomicBool::new(false),
            broadcast_muted: AtomicBool::new(false),
            voice_ducking_enabled: AtomicBool::new(true),
            voice_ducking_db_bits: AtomicU32::new((-10.0f32).to_bits()),
            aec_safe_monitor: AtomicBool::new(true),
            is_mic_active: AtomicBool::new(false),
            broadcast_stream_active: AtomicBool::new(false),
            monitor_stream_active: AtomicBool::new(false),
            bcast_active_count: AtomicUsize::new(0),
            mon_active_count: AtomicUsize::new(0),
            bcast_playing_bits: std::array::from_fn(|_| AtomicU64::new(0)),
            mon_playing_bits: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    #[inline]
    pub fn mic_volume(&self) -> f32 {
        f32::from_bits(self.mic_volume_bits.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn monitor_volume(&self) -> f32 {
        f32::from_bits(self.monitor_volume_bits.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn broadcast_volume(&self) -> f32 {
        f32::from_bits(self.broadcast_volume_bits.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn monitor_muted(&self) -> bool {
        self.monitor_muted.load(Ordering::Relaxed)
    }
    #[inline]
    pub fn broadcast_muted(&self) -> bool {
        self.broadcast_muted.load(Ordering::Relaxed)
    }
    #[inline]
    pub fn voice_ducking_enabled(&self) -> bool {
        self.voice_ducking_enabled.load(Ordering::Relaxed)
    }
    #[inline]
    pub fn voice_ducking_db(&self) -> f32 {
        f32::from_bits(self.voice_ducking_db_bits.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn aec_safe_monitor(&self) -> bool {
        self.aec_safe_monitor.load(Ordering::Relaxed)
    }

    pub fn set_volumes(
        &self,
        monitor_volume: f32,
        broadcast_volume: f32,
        mic_volume: f32,
        monitor_muted: bool,
        broadcast_muted: bool,
    ) {
        self.monitor_volume_bits
            .store(monitor_volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
        self.broadcast_volume_bits
            .store(broadcast_volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
        self.mic_volume_bits
            .store(mic_volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
        self.monitor_muted.store(monitor_muted, Ordering::Relaxed);
        self.broadcast_muted.store(broadcast_muted, Ordering::Relaxed);
    }

    pub fn set_ducking(&self, enabled: bool, ducking_db: f32) {
        self.voice_ducking_enabled.store(enabled, Ordering::Relaxed);
        self.voice_ducking_db_bits
            .store(ducking_db.clamp(-30.0, 0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_aec_safe_monitor(&self, enabled: bool) {
        self.aec_safe_monitor.store(enabled, Ordering::Relaxed);
    }
}

const HILBERT_COEFFS_A: [f32; 4] = [
    0.6923878,
    0.9360654322959,
    0.9882295226860,
    0.9987488452737,
];
const HILBERT_COEFFS_B: [f32; 4] = [
    0.4021921162426,
    0.8561710882420,
    0.9722909545651,
    0.9952884791278,
];
const AEC_SHIFT_HZ: f64 = 7.0;

#[derive(Clone, Copy, Default)]
struct AllpassStage {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl AllpassStage {
    #[inline]
    fn process(&mut self, coeff: f32, input: f32) -> f32 {
        let output = coeff * (input + self.y2) - self.x2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

#[derive(Clone, Default)]
struct ChannelHilbert {
    chain_a: [AllpassStage; 4],
    chain_b: [AllpassStage; 4],
    delay_a: f32,
}

impl ChannelHilbert {
    #[inline]
    fn process_iq(&mut self, input: f32) -> (f32, f32) {
        let mut a = input;
        for (stage, &coeff) in self.chain_a.iter_mut().zip(HILBERT_COEFFS_A.iter()) {
            a = stage.process(coeff, a);
        }
        let i_out = self.delay_a;
        self.delay_a = a;

        let mut b = input;
        for (stage, &coeff) in self.chain_b.iter_mut().zip(HILBERT_COEFFS_B.iter()) {
            b = stage.process(coeff, b);
        }
        (i_out, b)
    }
}

/// Zero-allocation +7 Hz single-sideband phase-decorrelating shifter applied strictly to the
/// local headphone monitor so linear Acoustic Echo Cancellation (TeamSpeak/Discord/WebRTC AEC)
/// cannot lock onto and cancel soundboard clips.
struct StereoHilbertShifter {
    left: ChannelHilbert,
    right: ChannelHilbert,
    phase: f64,
    phase_step: f64,
}

impl StereoHilbertShifter {
    fn new() -> Self {
        Self {
            left: ChannelHilbert::default(),
            right: ChannelHilbert::default(),
            phase: 0.0,
            phase_step: (2.0 * std::f64::consts::PI * AEC_SHIFT_HZ) / (SAMPLE_RATE as f64),
        }
    }

    fn reset(&mut self) {
        self.left = ChannelHilbert::default();
        self.right = ChannelHilbert::default();
        self.phase = 0.0;
    }

    #[inline]
    fn process_frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (il, ql) = self.left.process_iq(l);
        let (ir, qr) = self.right.process_iq(r);

        let c = self.phase.cos() as f32;
        let s = self.phase.sin() as f32;

        self.phase += self.phase_step;
        if self.phase >= 2.0 * std::f64::consts::PI {
            self.phase -= 2.0 * std::f64::consts::PI;
        }

        (il * c - ql * s, ir * c - qr * s)
    }
}

/// Real-time lock-free processor state owned exclusively by one audio stream thread.
pub struct StreamProcessor {
    cmd_rx: HeapCons<StreamCmd>,
    loaded_sounds: HashMap<String, (usize, Arc<DecodedAudio>)>,
    active_clips: Vec<ActiveClip>,
    controls: Arc<SharedMixerControls>,
    is_broadcast: bool,

    // Voice activity / ducking envelope state (used by broadcast processor)
    voice_detected: bool,
    voice_hold_remaining: usize,
    current_ducking_gain: f32,

    // Headphone-only AEC decorrelator (used by monitor processor)
    aec_shifter: StereoHilbertShifter,
}

impl StreamProcessor {
    fn new(
        cmd_rx: HeapCons<StreamCmd>,
        initial_sounds: HashMap<String, (usize, Arc<DecodedAudio>)>,
        controls: Arc<SharedMixerControls>,
        is_broadcast: bool,
    ) -> Self {
        Self {
            cmd_rx,
            loaded_sounds: initial_sounds,
            active_clips: Vec::with_capacity(MAX_ACTIVE_CLIPS),
            controls,
            is_broadcast,
            voice_detected: false,
            voice_hold_remaining: 0,
            current_ducking_gain: 1.0,
            aec_shifter: StereoHilbertShifter::new(),
        }
    }

    fn drain_commands(&mut self) {
        while let Some(cmd) = self.cmd_rx.try_pop() {
            match cmd {
                StreamCmd::LoadSound {
                    slot_id,
                    sound_id,
                    audio,
                } => {
                    self.loaded_sounds.insert(sound_id, (slot_id, audio));
                }
                StreamCmd::UnloadSound { sound_id } => {
                    self.loaded_sounds.remove(&sound_id);
                    self.active_clips.retain(|c| c.sound_id != sound_id);
                }
                StreamCmd::TriggerSound {
                    sound_id,
                    trim_start,
                    trim_end,
                    volume,
                    mode,
                    normalized_gain,
                    preview_only,
                } => {
                    self.trigger_clip(
                        &sound_id,
                        trim_start,
                        trim_end,
                        volume,
                        mode,
                        normalized_gain,
                        preview_only,
                    );
                }
                StreamCmd::StopSound {
                    sound_id,
                    preview_only_only,
                } => {
                    if preview_only_only {
                        self.active_clips
                            .retain(|c| !(c.sound_id == sound_id && c.preview_only));
                    } else {
                        self.active_clips.retain(|c| c.sound_id != sound_id);
                    }
                }
                StreamCmd::StopAll => {
                    self.active_clips.clear();
                }
            }
        }
    }

    fn trigger_clip(
        &mut self,
        sound_id: &str,
        trim_start: f32,
        trim_end: f32,
        volume: f32,
        mode: PlaybackMode,
        normalized_gain: f32,
        preview_only: bool,
    ) {
        let Some((slot_id, audio)) = self.loaded_sounds.get(sound_id) else {
            return;
        };
        let slot_id = *slot_id;
        let total_samples = audio.samples.len();
        if total_samples < CHANNELS {
            return;
        }

        let start_pos = ((trim_start.max(0.0) * SAMPLE_RATE as f32 * CHANNELS as f32) as usize)
            .min(total_samples.saturating_sub(2))
            & !1;
        let raw_end = (trim_end.max(0.0) * SAMPLE_RATE as f32 * CHANNELS as f32) as usize;
        let end_pos = if raw_end > start_pos {
            raw_end.min(total_samples) & !1
        } else {
            total_samples & !1
        };
        if end_pos <= start_pos {
            return;
        }

        match mode {
            PlaybackMode::Restart => {
                if let Some(existing) = self
                    .active_clips
                    .iter_mut()
                    .find(|c| c.sound_id == sound_id && c.preview_only == preview_only)
                {
                    existing.pos = start_pos;
                    existing.start_pos = start_pos;
                    existing.end_pos = end_pos;
                    existing.volume = volume;
                    existing.normalized_gain = normalized_gain;
                    existing.mode = mode;
                } else if self.active_clips.len() < MAX_ACTIVE_CLIPS {
                    self.active_clips.push(ActiveClip {
                        slot_id,
                        sound_id: sound_id.to_string(),
                        pos: start_pos,
                        start_pos,
                        end_pos,
                        volume,
                        normalized_gain,
                        mode,
                        preview_only,
                    });
                }
            }
            PlaybackMode::Stop => {
                if let Some(pos) = self
                    .active_clips
                    .iter()
                    .position(|c| c.sound_id == sound_id && c.preview_only == preview_only)
                {
                    self.active_clips.remove(pos);
                } else if self.active_clips.len() < MAX_ACTIVE_CLIPS {
                    self.active_clips.push(ActiveClip {
                        slot_id,
                        sound_id: sound_id.to_string(),
                        pos: start_pos,
                        start_pos,
                        end_pos,
                        volume,
                        normalized_gain,
                        mode,
                        preview_only,
                    });
                }
            }
            PlaybackMode::Overlap => {
                if self.active_clips.len() >= MAX_ACTIVE_CLIPS {
                    self.active_clips.remove(0);
                }
                self.active_clips.push(ActiveClip {
                    slot_id,
                    sound_id: sound_id.to_string(),
                    pos: start_pos,
                    start_pos,
                    end_pos,
                    volume,
                    normalized_gain,
                    mode,
                    preview_only,
                });
            }
        }
    }

    fn publish_status(&self) {
        let mut mask = [0u64; BITSET_WORDS];
        for clip in &self.active_clips {
            if clip.slot_id < MAX_SOUND_SLOTS {
                mask[clip.slot_id / 64] |= 1u64 << (clip.slot_id % 64);
            }
        }
        let target = if self.is_broadcast {
            self.controls
                .bcast_active_count
                .store(self.active_clips.len(), Ordering::Relaxed);
            &self.controls.bcast_playing_bits
        } else {
            self.controls
                .mon_active_count
                .store(self.active_clips.len(), Ordering::Relaxed);
            &self.controls.mon_playing_bits
        };
        for i in 0..BITSET_WORDS {
            target[i].store(mask[i], Ordering::Relaxed);
        }
    }

    pub fn process_broadcast(&mut self, mic_input: &[f32], broadcast_out: &mut [f32]) {
        self.drain_commands();

        let frames_count = broadcast_out.len() / CHANNELS;
        broadcast_out.fill(0.0);

        // 1. Voice activity detection with hysteresis + 250ms hold (scaled by mic pass-through level)
        let mic_gain = self.controls.mic_volume();
        let mic_available = !mic_input.is_empty();
        let mic_rms = if mic_available && mic_gain > 0.001 {
            let mut sum_sq = 0.0f32;
            for &s in mic_input {
                sum_sq += s * s;
            }
            (sum_sq / mic_input.len() as f32).sqrt() * mic_gain
        } else {
            0.0
        };

        if mic_rms > VOICE_OPEN_THRESH
            || (self.voice_detected && mic_rms > VOICE_CLOSE_THRESH)
        {
            self.voice_detected = true;
            self.voice_hold_remaining = VOICE_HOLD_FRAMES;
        } else if self.voice_hold_remaining > 0 {
            self.voice_hold_remaining = self.voice_hold_remaining.saturating_sub(frames_count);
            if self.voice_hold_remaining == 0 {
                self.voice_detected = false;
            }
        } else {
            self.voice_detected = false;
        }

        self.controls
            .is_mic_active
            .store(self.voice_detected, Ordering::Relaxed);

        let duck_enabled = self.controls.voice_ducking_enabled();
        let duck_db = self.controls.voice_ducking_db().clamp(-30.0, 0.0);
        let target_duck = if self.voice_detected && duck_enabled {
            10.0f32.powf(duck_db / 20.0)
        } else {
            1.0f32
        };

        // Smooth exponential attack (~15ms) and release (~300ms)
        let alpha_attack = 1.0 - (-1.0 / (0.015 * SAMPLE_RATE as f32)).exp();
        let alpha_release = 1.0 - (-1.0 / (0.300 * SAMPLE_RATE as f32)).exp();

        let bcast_gain = if self.controls.broadcast_muted() {
            0.0
        } else {
            perceptual_volume_gain(self.controls.broadcast_volume())
        };

        // 2. Mix active soundboard clips into broadcast_out
        for clip in self.active_clips.iter_mut() {
            let Some((_, sound_audio)) = self.loaded_sounds.get(&clip.sound_id) else {
                clip.pos = clip.end_pos;
                continue;
            };
            let samples = &sound_audio.samples;
            let total_samples = samples.len();
            let clip_gain = clip.volume * clip.normalized_gain;

            for frame in 0..frames_count {
                if clip.pos + 1 >= clip.end_pos || clip.pos + 1 >= total_samples {
                    clip.pos = clip.end_pos;
                    break;
                }
                broadcast_out[frame * 2] += samples[clip.pos] * clip_gain;
                broadcast_out[frame * 2 + 1] += samples[clip.pos + 1] * clip_gain;
                clip.pos += 2;
            }
        }

        self.active_clips.retain(|c| c.pos < c.end_pos);
        self.publish_status();

        // 3. Apply ducking, soft-limit ONLY the sound bus, keep mic voice pristine
        for frame in 0..frames_count {
            let alpha = if target_duck < self.current_ducking_gain {
                alpha_attack
            } else {
                alpha_release
            };
            self.current_ducking_gain += alpha * (target_duck - self.current_ducking_gain);
            let duck = self.current_ducking_gain;

            let m_l = if mic_available && frame * 2 < mic_input.len() {
                mic_input[frame * 2] * mic_gain
            } else {
                0.0
            };
            let m_r = if mic_available && frame * 2 + 1 < mic_input.len() {
                mic_input[frame * 2 + 1] * mic_gain
            } else {
                0.0
            };

            let s_l = soft_peak_limiter(broadcast_out[frame * 2] * duck * bcast_gain);
            let s_r = soft_peak_limiter(broadcast_out[frame * 2 + 1] * duck * bcast_gain);

            broadcast_out[frame * 2] = blend_preserving_voice(m_l, s_l);
            broadcast_out[frame * 2 + 1] = blend_preserving_voice(m_r, s_r);
        }
    }

    pub fn process_monitor(&mut self, monitor_out: &mut [f32]) {
        self.drain_commands();

        let frames_count = monitor_out.len() / CHANNELS;
        monitor_out.fill(0.0);

        let mon_gain = if self.controls.monitor_muted() {
            0.0
        } else {
            perceptual_volume_gain(self.controls.monitor_volume())
        };

        let has_broadcast_clip = self.active_clips.iter().any(|c| !c.preview_only);

        for clip in self.active_clips.iter_mut() {
            let Some((_, sound_audio)) = self.loaded_sounds.get(&clip.sound_id) else {
                clip.pos = clip.end_pos;
                continue;
            };
            let samples = &sound_audio.samples;
            let total_samples = samples.len();
            let clip_gain = clip.volume * clip.normalized_gain;

            for frame in 0..frames_count {
                if clip.pos + 1 >= clip.end_pos || clip.pos + 1 >= total_samples {
                    clip.pos = clip.end_pos;
                    break;
                }
                monitor_out[frame * 2] += samples[clip.pos] * clip_gain;
                monitor_out[frame * 2 + 1] += samples[clip.pos + 1] * clip_gain;
                clip.pos += 2;
            }
        }

        self.active_clips.retain(|c| c.pos < c.end_pos);
        self.publish_status();

        let apply_aec_shift =
            mon_gain > 0.0 && self.controls.aec_safe_monitor() && has_broadcast_clip;
        if apply_aec_shift {
            for frame in 0..frames_count {
                let (sl, sr) = self
                    .aec_shifter
                    .process_frame(monitor_out[frame * 2], monitor_out[frame * 2 + 1]);
                monitor_out[frame * 2] = soft_peak_limiter(sl * mon_gain);
                monitor_out[frame * 2 + 1] = soft_peak_limiter(sr * mon_gain);
            }
        } else {
            if self.active_clips.is_empty() {
                self.aec_shifter.reset();
            }
            for frame in 0..frames_count {
                monitor_out[frame * 2] = soft_peak_limiter(monitor_out[frame * 2] * mon_gain);
                monitor_out[frame * 2 + 1] = soft_peak_limiter(monitor_out[frame * 2 + 1] * mon_gain);
            }
        }
    }

    pub fn active_clip_count(&self) -> usize {
        self.active_clips.len()
    }
}

/// Keeps the user's mic voice completely uncolored when `|mic + sound| <= 0.99`,
/// yielding sound headroom first if a spike occurs, with hard safety clamp at `±0.995`.
#[inline]
fn blend_preserving_voice(mic: f32, sound: f32) -> f32 {
    let sum = mic + sound;
    if sum.abs() <= 0.99 {
        sum
    } else {
        let ceiling = 0.995;
        let m = mic.clamp(-ceiling, ceiling);
        let headroom = (ceiling - m.abs()).max(0.0);
        let s_capped = sound.clamp(-headroom, headroom);
        (m + s_capped).clamp(-ceiling, ceiling)
    }
}

/// Perceptual quadratic audio taper for `0.0..=1.0` so lowering the volume slider
/// produces natural decibel attenuation (`50%` = `-12 dB`), while keeping `100%` at unity.
#[inline]
pub fn perceptual_volume_gain(vol: f32) -> f32 {
    let v = vol.clamp(0.0, 2.0);
    if v <= 1.0 {
        v * v
    } else {
        v
    }
}

/// Smooth soft-knee peak limiter applied to the soundboard bus (never speech).
#[inline]
pub fn soft_peak_limiter(sample: f32) -> f32 {
    let abs_val = sample.abs();
    if abs_val <= 0.85 {
        sample
    } else {
        let sign = sample.signum();
        let excess = abs_val - 0.85;
        let compressed = 0.85 + 0.14 * (excess / 0.14).tanh();
        sign * compressed
    }
}

pub struct AudioMixer {
    pub controls: Arc<SharedMixerControls>,
    registry: RwLock<HashMap<String, RegisteredSound>>,
    next_slot_id: AtomicUsize,
    bcast_tx: Mutex<Option<HeapProd<StreamCmd>>>,
    mon_tx: Mutex<Option<HeapProd<StreamCmd>>>,
    // Optional synchronous processors used when no hardware streams are running or in tests
    sync_fallback: Mutex<Option<(StreamProcessor, StreamProcessor)>>,
}

impl AudioMixer {
    pub fn new() -> Self {
        Self {
            controls: Arc::new(SharedMixerControls::new()),
            registry: RwLock::new(HashMap::new()),
            next_slot_id: AtomicUsize::new(0),
            bcast_tx: Mutex::new(None),
            mon_tx: Mutex::new(None),
            sync_fallback: Mutex::new(None),
        }
    }

    pub fn load_sound(&self, sound_id: String, audio: DecodedAudio) {
        let arc_audio = Arc::new(audio);
        let slot_id = {
            let mut reg = self.registry.write();
            let slot_id = if let Some(existing) = reg.get(&sound_id) {
                existing.slot_id
            } else {
                let used: std::collections::HashSet<usize> =
                    reg.values().map(|v| v.slot_id).collect();
                let mut chosen = None;
                for _ in 0..MAX_SOUND_SLOTS {
                    let candidate = self.next_slot_id.fetch_add(1, Ordering::Relaxed) % MAX_SOUND_SLOTS;
                    if !used.contains(&candidate) {
                        chosen = Some(candidate);
                        break;
                    }
                }
                chosen.unwrap_or_else(|| {
                    self.next_slot_id.fetch_add(1, Ordering::Relaxed) % MAX_SOUND_SLOTS
                })
            };
            reg.insert(
                sound_id.clone(),
                RegisteredSound {
                    slot_id,
                    audio: Arc::clone(&arc_audio),
                },
            );
            slot_id
        };

        let cmd = StreamCmd::LoadSound {
            slot_id,
            sound_id,
            audio: arc_audio,
        };
        if let Some(tx) = self.bcast_tx.lock().as_mut() {
            let _ = tx.try_push(cmd.clone());
        }
        if let Some(tx) = self.mon_tx.lock().as_mut() {
            let _ = tx.try_push(cmd);
        }
    }

    pub fn unload_sound(&self, sound_id: &str) {
        if let Some(removed) = self.registry.write().remove(sound_id) {
            let slot = removed.slot_id;
            if slot < MAX_SOUND_SLOTS {
                let word = slot / 64;
                let mask = !(1u64 << (slot % 64));
                self.controls.bcast_playing_bits[word].fetch_and(mask, Ordering::Relaxed);
                self.controls.mon_playing_bits[word].fetch_and(mask, Ordering::Relaxed);
            }
        }
        let cmd = StreamCmd::UnloadSound {
            sound_id: sound_id.to_string(),
        };
        if let Some(tx) = self.bcast_tx.lock().as_mut() {
            let _ = tx.try_push(cmd.clone());
        }
        if let Some(tx) = self.mon_tx.lock().as_mut() {
            let _ = tx.try_push(cmd);
        }
    }

    pub fn update_sound_gain(&self, sound_id: &str, normalized_gain: f32) {
        // Reload updated gain metadata into active streams if audio is already loaded
        let audio = {
            let mut reg = self.registry.write();
            if let Some(entry) = reg.get_mut(sound_id) {
                let mut cloned = (*entry.audio).clone();
                cloned.normalized_gain = normalized_gain;
                let arc = Arc::new(cloned);
                entry.audio = Arc::clone(&arc);
                Some((entry.slot_id, arc))
            } else {
                None
            }
        };
        if let Some((slot_id, arc)) = audio {
            let cmd = StreamCmd::LoadSound {
                slot_id,
                sound_id: sound_id.to_string(),
                audio: arc,
            };
            if let Some(tx) = self.bcast_tx.lock().as_mut() {
                let _ = tx.try_push(cmd.clone());
            }
            if let Some(tx) = self.mon_tx.lock().as_mut() {
                let _ = tx.try_push(cmd);
            }
        }
    }

    pub fn has_sound_loaded(&self, sound_id: &str) -> bool {
        self.registry.read().contains_key(sound_id)
    }

    pub fn get_loaded_audio(&self, sound_id: &str) -> Option<Arc<DecodedAudio>> {
        self.registry.read().get(sound_id).map(|e| Arc::clone(&e.audio))
    }

    pub fn trigger_sound(
        &self,
        sound_id: &str,
        trim_start: f32,
        trim_end: f32,
        volume: f32,
        mode: PlaybackMode,
        normalized_gain: f32,
    ) {
        let cmd = StreamCmd::TriggerSound {
            sound_id: sound_id.to_string(),
            trim_start,
            trim_end,
            volume,
            mode,
            normalized_gain,
            preview_only: false,
        };
        if let Some(tx) = self.bcast_tx.lock().as_mut() {
            let _ = tx.try_push(cmd.clone());
        }
        if let Some(tx) = self.mon_tx.lock().as_mut() {
            let _ = tx.try_push(cmd);
        }
    }

    /// Triggers preview playback ONLY on the local Monitor stream (never broadcast to Discord/calls).
    pub fn trigger_preview(
        &self,
        sound_id: &str,
        trim_start: f32,
        trim_end: f32,
        volume: f32,
        normalized_gain: f32,
    ) {
        let cmd = StreamCmd::TriggerSound {
            sound_id: sound_id.to_string(),
            trim_start,
            trim_end,
            volume,
            mode: PlaybackMode::Restart,
            normalized_gain,
            preview_only: true,
        };
        if let Some(tx) = self.mon_tx.lock().as_mut() {
            let _ = tx.try_push(cmd);
        }
    }

    pub fn stop_sound(&self, sound_id: &str) {
        let cmd = StreamCmd::StopSound {
            sound_id: sound_id.to_string(),
            preview_only_only: false,
        };
        if let Some(tx) = self.bcast_tx.lock().as_mut() {
            let _ = tx.try_push(cmd.clone());
        }
        if let Some(tx) = self.mon_tx.lock().as_mut() {
            let _ = tx.try_push(cmd);
        }
    }

    pub fn stop_preview(&self, sound_id: &str) {
        let cmd = StreamCmd::StopSound {
            sound_id: sound_id.to_string(),
            preview_only_only: true,
        };
        if let Some(tx) = self.mon_tx.lock().as_mut() {
            let _ = tx.try_push(cmd);
        }
    }

    pub fn stop_all(&self) {
        if let Some(tx) = self.bcast_tx.lock().as_mut() {
            let _ = tx.try_push(StreamCmd::StopAll);
        }
        if let Some(tx) = self.mon_tx.lock().as_mut() {
            let _ = tx.try_push(StreamCmd::StopAll);
        }
    }

    fn snapshot_sounds(&self) -> HashMap<String, (usize, Arc<DecodedAudio>)> {
        self.registry
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), (v.slot_id, Arc::clone(&v.audio))))
            .collect()
    }

    /// Creates an exclusively owned lock-free Broadcast StreamProcessor and wires its SPSC producer.
    pub fn create_broadcast_processor(&self) -> StreamProcessor {
        let rb = HeapRb::<StreamCmd>::new(2048);
        let (tx, rx) = rb.split();
        *self.bcast_tx.lock() = Some(tx);
        self.controls.bcast_active_count.store(0, Ordering::Relaxed);
        for w in &self.controls.bcast_playing_bits {
            w.store(0, Ordering::Relaxed);
        }
        StreamProcessor::new(
            rx,
            self.snapshot_sounds(),
            Arc::clone(&self.controls),
            true,
        )
    }

    /// Creates an exclusively owned lock-free Monitor StreamProcessor and wires its SPSC producer.
    pub fn create_monitor_processor(&self) -> StreamProcessor {
        let rb = HeapRb::<StreamCmd>::new(2048);
        let (tx, rx) = rb.split();
        *self.mon_tx.lock() = Some(tx);
        self.controls.mon_active_count.store(0, Ordering::Relaxed);
        for w in &self.controls.mon_playing_bits {
            w.store(0, Ordering::Relaxed);
        }
        StreamProcessor::new(
            rx,
            self.snapshot_sounds(),
            Arc::clone(&self.controls),
            false,
        )
    }

    pub fn clear_broadcast_channel(&self) {
        *self.bcast_tx.lock() = None;
        self.controls.bcast_active_count.store(0, Ordering::Relaxed);
        for w in &self.controls.bcast_playing_bits {
            w.store(0, Ordering::Relaxed);
        }
    }

    pub fn clear_monitor_channel(&self) {
        *self.mon_tx.lock() = None;
        self.controls.mon_active_count.store(0, Ordering::Relaxed);
        for w in &self.controls.mon_playing_bits {
            w.store(0, Ordering::Relaxed);
        }
    }

    pub fn get_playing_sound_ids(&self) -> Vec<String> {
        let mut combined = [0u64; BITSET_WORDS];
        let b_active = self.controls.broadcast_stream_active.load(Ordering::Relaxed);
        let m_active = self.controls.monitor_stream_active.load(Ordering::Relaxed);

        for i in 0..BITSET_WORDS {
            let mut w = 0u64;
            if b_active {
                w |= self.controls.bcast_playing_bits[i].load(Ordering::Relaxed);
            }
            if m_active {
                w |= self.controls.mon_playing_bits[i].load(Ordering::Relaxed);
            }
            combined[i] = w;
        }

        let reg = self.registry.read();
        let mut ids = Vec::new();
        for (sound_id, entry) in reg.iter() {
            if entry.slot_id < MAX_SOUND_SLOTS {
                let word = combined[entry.slot_id / 64];
                if (word & (1u64 << (entry.slot_id % 64))) != 0 {
                    ids.push(sound_id.clone());
                }
            }
        }
        ids
    }

    pub fn active_clip_count(&self) -> usize {
        let b = if self.controls.broadcast_stream_active.load(Ordering::Relaxed) {
            self.controls.bcast_active_count.load(Ordering::Relaxed)
        } else {
            0
        };
        let m = if self.controls.monitor_stream_active.load(Ordering::Relaxed) {
            self.controls.mon_active_count.load(Ordering::Relaxed)
        } else {
            0
        };
        b.max(m)
    }

    /// Initializes synchronous processors for unit tests and `test_audio` CLI verification.
    pub fn init_sync_for_test(&self) {
        let bcast = self.create_broadcast_processor();
        let mon = self.create_monitor_processor();
        self.controls
            .broadcast_stream_active
            .store(true, Ordering::Relaxed);
        self.controls
            .monitor_stream_active
            .store(true, Ordering::Relaxed);
        *self.sync_fallback.lock() = Some((bcast, mon));
    }

    pub fn process_frames(
        &self,
        mic_input: &[f32],
        broadcast_out: &mut [f32],
        monitor_out: &mut [f32],
    ) {
        let mut guard = self.sync_fallback.lock();
        if guard.is_none() {
            drop(guard);
            self.init_sync_for_test();
            guard = self.sync_fallback.lock();
        }
        let (bcast, mon) = guard.as_mut().unwrap();
        bcast.process_broadcast(mic_input, broadcast_out);
        mon.process_monitor(monitor_out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_audio(val: f32, duration: f32) -> DecodedAudio {
        DecodedAudio {
            samples: vec![val; (SAMPLE_RATE as f32 * duration) as usize * CHANNELS],
            duration,
            peaks: vec![val.abs(); 20],
            normalized_gain: 1.0,
        }
    }

    #[test]
    fn test_peak_limiter_within_normal_range() {
        for &s in &[-0.85, -0.5, 0.0, 0.5, 0.85] {
            assert!((soft_peak_limiter(s) - s).abs() < 1e-6);
        }
    }

    #[test]
    fn test_peak_limiter_smoothly_saturates_loud_signals() {
        for &s in &[0.9, 1.5, 5.0, -1.5, -10.0] {
            let limited = soft_peak_limiter(s);
            assert!(limited.abs() < 1.0);
            if s > 0.0 {
                assert!(limited > 0.85);
            } else {
                assert!(limited < -0.85);
            }
        }
    }

    #[test]
    fn test_voice_passthrough_is_uncolored_by_limiter() {
        let mixer = AudioMixer::new();
        mixer.init_sync_for_test();
        mixer.controls.set_volumes(0.8, 1.0, 1.0, false, false);
        mixer.controls.set_ducking(false, -10.0);

        // Loud clean speech peaking at 0.92 (> 0.85 soft knee threshold!)
        let mic = vec![0.92f32; 256 * CHANNELS];
        let mut bcast = vec![0.0f32; 256 * CHANNELS];
        let mut mon = vec![0.0f32; 256 * CHANNELS];
        mixer.process_frames(&mic, &mut bcast, &mut mon);

        // Voice should pass completely bit-exact without tanh saturation!
        assert!((bcast[0] - 0.92).abs() < 1e-6, "got {}", bcast[0]);
    }

    #[test]
    fn test_preview_is_monitor_only() {
        let mixer = AudioMixer::new();
        mixer.init_sync_for_test();
        mixer.load_sound("s1".into(), test_audio(0.5, 1.0));
        mixer.trigger_preview("s1", 0.0, 1.0, 1.0, 1.0);

        let mut bcast = vec![0.0f32; 256 * CHANNELS];
        let mut mon = vec![0.0f32; 256 * CHANNELS];
        mixer.process_frames(&[], &mut bcast, &mut mon);

        assert_eq!(bcast[0], 0.0, "Preview must NOT bleed into broadcast stream");
        assert!(mon[0] > 0.3, "Preview must play on monitor headphones");
    }

    #[test]
    fn test_ducking_supports_minus_24db_and_hold() {
        let mixer = AudioMixer::new();
        mixer.init_sync_for_test();
        mixer.load_sound("s1".into(), test_audio(0.5, 2.0));
        mixer.controls.set_ducking(true, -24.0);
        mixer.trigger_sound("s1", 0.0, 2.0, 1.0, PlaybackMode::Restart, 1.0);

        let loud_mic = vec![0.25f32; 4096 * CHANNELS];
        let mut bcast = vec![0.0f32; 4096 * CHANNELS];
        let mut mon = vec![0.0f32; 4096 * CHANNELS];
        mixer.process_frames(&loud_mic, &mut bcast, &mut mon);

        // -24dB = 0.0631x; after ~85ms attack (> 5*15ms), 0.5 sound ducks near ~0.033
        let last_sound_only = bcast[bcast.len() - 2] - 0.25;
        assert!(
            last_sound_only < 0.045,
            "Expected -24dB ducked sound < 0.045, got {last_sound_only}"
        );

        // Brief silence (<250ms hold) must NOT immediately pump ducking back up
        let quiet_mic = vec![0.0f32; 256 * CHANNELS];
        let mut bcast2 = vec![0.0f32; 256 * CHANNELS];
        let mut mon2 = vec![0.0f32; 256 * CHANNELS];
        mixer.process_frames(&quiet_mic, &mut bcast2, &mut mon2);
        assert!(mixer.controls.is_mic_active.load(Ordering::Relaxed));
        assert!(bcast2[0] < 0.045);
    }

    #[test]
    fn test_aec_safe_monitor_decorrelates_without_altering_broadcast() {
        let mixer = AudioMixer::new();
        mixer.init_sync_for_test();
        mixer.controls.set_volumes(1.0, 1.0, 1.0, false, false);
        mixer.controls.set_aec_safe_monitor(true);

        let frames = SAMPLE_RATE as usize / 2; // 0.5s 440 Hz tone
        let mut samples = Vec::with_capacity(frames * CHANNELS);
        for f in 0..frames {
            let v = (2.0 * std::f32::consts::PI * 440.0 * f as f32 / SAMPLE_RATE as f32).sin() * 0.5;
            samples.push(v);
            samples.push(v);
        }
        mixer.load_sound(
            "sine440".into(),
            DecodedAudio {
                samples: samples.clone(),
                duration: 0.5,
                peaks: vec![0.5; 20],
                normalized_gain: 1.0,
            },
        );
        mixer.trigger_sound("sine440", 0.0, 0.5, 1.0, PlaybackMode::Restart, 1.0);

        let mut bcast = vec![0.0f32; frames * CHANNELS];
        let mut mon = vec![0.0f32; frames * CHANNELS];
        mixer.process_frames(&[], &mut bcast, &mut mon);

        // Broadcast remains bit-exact to original 440 Hz sound
        let max_bcast_err = samples
            .iter()
            .zip(bcast.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_bcast_err < 1e-5, "Broadcast altered: {max_bcast_err}");

        // Monitor preserves full RMS loudness while decorrelating linearly from broadcast
        let skip = 4096 * CHANNELS;
        let mut b_sq = 0.0f64;
        let mut m_sq = 0.0f64;
        let mut dot = 0.0f64;
        for (&b, &m) in bcast[skip..].iter().zip(mon[skip..].iter()) {
            b_sq += (b as f64) * (b as f64);
            m_sq += (m as f64) * (m as f64);
            dot += (b as f64) * (m as f64);
        }
        let rms_ratio = (m_sq / b_sq).sqrt();
        let norm_corr = (dot / (b_sq * m_sq).sqrt()).abs();
        assert!(
            (rms_ratio - 1.0).abs() < 0.08,
            "Monitor RMS drifted unexpectedly: {rms_ratio}"
        );
        assert!(
            norm_corr < 0.20,
            "Expected linear AEC decorrelation < 0.20, got {norm_corr}"
        );
    }

    #[test]
    fn test_perceptual_volume_taper_preserves_unity_and_attenuates_mid_slider() {
        assert!((perceptual_volume_gain(1.0) - 1.0).abs() < 1e-6);
        assert!((perceptual_volume_gain(0.5) - 0.25).abs() < 1e-6);
        assert!((perceptual_volume_gain(0.0) - 0.0).abs() < 1e-6);
    }
}
