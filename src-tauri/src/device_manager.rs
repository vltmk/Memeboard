use crate::audio_thread::{AudioThread, enter_audio_thread};
use crate::mixer::{AudioMixer, CHANNELS, SAMPLE_RATE, StreamProcessor};
#[cfg(not(target_os = "linux"))]
use crate::types::{AudioDevice, AudioDevicesResult};
#[cfg(target_os = "linux")]
use crate::types::AudioDevicesResult;
use crate::virtual_mic::policy;
#[cfg(not(target_os = "linux"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(target_os = "linux"))]
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use parking_lot::Mutex;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
#[cfg(not(target_os = "linux"))]
use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;

pub(crate) const MIC_RING_FRAMES: usize = 4096;
const MIC_CUSHION_FRAMES: usize = 480; // 10 ms jitter cushion at 48 kHz
const MIC_DRIFT_HIGH_OFFSET: usize = 960; // +20 ms above callback window
const MIC_DRIFT_LOW_OFFSET: usize = 160; // ~3.3 ms cushion floor before slow-down
const RESAMPLE_CHUNK: usize = 128;

#[cfg(not(target_os = "linux"))]
thread_local! {
    static PRO_AUDIO_GUARD: RefCell<Option<AudioThread>> = const { RefCell::new(None) };
}

#[cfg(not(target_os = "linux"))]
#[inline]
fn ensure_pro_audio_thread() {
    PRO_AUDIO_GUARD.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(enter_audio_thread());
        }
    });
}

/// Identifies virtual audio endpoints without misclassifying motherboard speaker mixers
/// (e.g. ASUS / Nahimic `Sonic Studio Virtual Mixer`).
pub fn is_virtual_endpoint(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("sonic studio virtual mixer") {
        return false;
    }
    policy::is_virtual_audio_name(name)
        || lower.contains("cable input")
        || lower.contains("cable output")
        || lower.contains("vb-audio")
        || lower.contains("blackhole")
        || lower.contains("voicemeeter")
        || lower.contains("virtual audio cable")
        || lower.ends_with(".monitor")
}

/// Returns priority score (higher is better) if an output device is a known virtual microphone
/// broadcast bridge target, or `None` if it is a regular speaker/headphone or unsupported mixer.
pub(crate) fn broadcast_virtual_priority(name: &str) -> Option<u8> {
    let lower = name.to_lowercase();
    if lower.contains("sonic studio virtual mixer") {
        return None;
    }
    if lower.contains("memeboard virtual mic") || lower.contains("memeboard_virtual") {
        return Some(100);
    }
    if lower.contains("cable input") {
        return Some(80);
    }
    if lower.contains("vb-audio") {
        return Some(75);
    }
    if lower.contains("blackhole") {
        return Some(70);
    }
    if lower.contains("voicemeeter input") || lower.contains("voicemeeter aux input") {
        return Some(65);
    }
    if lower.contains("memeboard") {
        return Some(60);
    }
    None
}

pub(crate) struct StreamingResampler {
    resampler: SincFixedIn<f32>,
    chunk_size: usize,
    pending_l: Vec<f32>,
    pending_r: Vec<f32>,
    scratch_in_l: Vec<f32>,
    scratch_in_r: Vec<f32>,
    out_buf: Vec<Vec<f32>>,
    fifo_l: Vec<f32>,
    fifo_r: Vec<f32>,
    fifo_read: usize,
}

impl StreamingResampler {
    fn new(from_rate: u32, to_rate: u32) -> Option<Self> {
        if from_rate == 0 || to_rate == 0 || from_rate == to_rate {
            return None;
        }
        let ratio = to_rate as f64 / from_rate as f64;
        let params = SincInterpolationParameters {
            sinc_len: 48,
            f_cutoff: 0.94,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 64,
            window: WindowFunction::BlackmanHarris2,
        };
        let resampler =
            SincFixedIn::<f32>::new(ratio, 1.1, params, RESAMPLE_CHUNK, CHANNELS).ok()?;
        let max_out = resampler.output_frames_max().max(256);
        Some(Self {
            resampler,
            chunk_size: RESAMPLE_CHUNK,
            pending_l: Vec::with_capacity(4096),
            pending_r: Vec::with_capacity(4096),
            scratch_in_l: vec![0.0; RESAMPLE_CHUNK],
            scratch_in_r: vec![0.0; RESAMPLE_CHUNK],
            out_buf: vec![vec![0.0; max_out], vec![0.0; max_out]],
            fifo_l: Vec::with_capacity(8192),
            fifo_r: Vec::with_capacity(8192),
            fifo_read: 0,
        })
    }

    fn compact_fifo_if_needed(&mut self) {
        if self.fifo_read > 2048 || self.fifo_read >= self.fifo_l.len() {
            let rem = self.fifo_l.len().saturating_sub(self.fifo_read);
            if rem > 0 {
                self.fifo_l.copy_within(self.fifo_read.., 0);
                self.fifo_r.copy_within(self.fifo_read.., 0);
            }
            self.fifo_l.truncate(rem);
            self.fifo_r.truncate(rem);
            self.fifo_read = 0;
        }
    }

    fn push_frame(&mut self, l: f32, r: f32) {
        if self.pending_l.len() >= 4096 {
            let drop_n = self.pending_l.len() - 2048;
            let rem = self.pending_l.len() - drop_n;
            self.pending_l.copy_within(drop_n.., 0);
            self.pending_r.copy_within(drop_n.., 0);
            self.pending_l.truncate(rem);
            self.pending_r.truncate(rem);
        }
        self.pending_l.push(l);
        self.pending_r.push(r);
        self.process_pending_chunks();
    }

    fn push_interleaved(&mut self, stereo: &[f32]) {
        for frame in stereo.chunks_exact(2) {
            self.pending_l.push(frame[0]);
            self.pending_r.push(frame[1]);
        }
        if self.pending_l.len() >= 4096 {
            let drop_n = self.pending_l.len() - 2048;
            let rem = self.pending_l.len() - drop_n;
            self.pending_l.copy_within(drop_n.., 0);
            self.pending_r.copy_within(drop_n.., 0);
            self.pending_l.truncate(rem);
            self.pending_r.truncate(rem);
        }
        self.process_pending_chunks();
    }

    fn process_pending_chunks(&mut self) {
        while self.pending_l.len() >= self.chunk_size {
            self.scratch_in_l
                .copy_from_slice(&self.pending_l[..self.chunk_size]);
            self.scratch_in_r
                .copy_from_slice(&self.pending_r[..self.chunk_size]);

            let rem = self.pending_l.len() - self.chunk_size;
            self.pending_l.copy_within(self.chunk_size.., 0);
            self.pending_r.copy_within(self.chunk_size.., 0);
            self.pending_l.truncate(rem);
            self.pending_r.truncate(rem);

            let slices = [
                &self.scratch_in_l[..self.chunk_size],
                &self.scratch_in_r[..self.chunk_size],
            ];
            if let Ok((_, written)) =
                self.resampler
                    .process_into_buffer(&slices, &mut self.out_buf, None)
            {
                self.compact_fifo_if_needed();
                if self.fifo_l.len() + written <= 8192 {
                    self.fifo_l.extend_from_slice(&self.out_buf[0][..written]);
                    self.fifo_r.extend_from_slice(&self.out_buf[1][..written]);
                }
            }
        }
    }

    #[inline]
    fn available_frames(&self) -> usize {
        self.fifo_l.len().saturating_sub(self.fifo_read)
    }

    #[inline]
    fn pop_frame(&mut self) -> Option<[f32; 2]> {
        if self.fifo_read < self.fifo_l.len() {
            let frame = [self.fifo_l[self.fifo_read], self.fifo_r[self.fifo_read]];
            self.fifo_read += 1;
            if self.fifo_read >= self.fifo_l.len() {
                self.fifo_l.clear();
                self.fifo_r.clear();
                self.fifo_read = 0;
            }
            Some(frame)
        } else {
            None
        }
    }
}

pub(crate) struct MicConsumerState {
    cons: HeapCons<[f32; 2]>,
    primed: bool,
    raw_scratch: Vec<[f32; 2]>,
    resampled_interleaved: Vec<f32>,
}

impl MicConsumerState {
    pub(crate) fn new(cons: HeapCons<[f32; 2]>) -> Self {
        Self {
            cons,
            primed: false,
            raw_scratch: Vec::with_capacity(4096),
            resampled_interleaved: Vec::with_capacity(8192),
        }
    }

    /// Reads `needed_frames` 48 kHz stereo frames into `self.resampled_interleaved`, applying
    /// callback-aware prefill (~10 ms jitter cushion) and ±0.2% clock-drift lock without clicks.
    fn fill_mic_frames(&mut self, needed_frames: usize, mic_stream_active: bool) -> &[f32] {
        self.resampled_interleaved.clear();
        if !mic_stream_active || needed_frames == 0 {
            self.primed = false;
            return &[];
        }

        let prefill_target = (needed_frames + MIC_CUSHION_FRAMES).clamp(640, MIC_RING_FRAMES / 2);
        let drift_high = (needed_frames + MIC_DRIFT_HIGH_OFFSET).min(MIC_RING_FRAMES - 256);
        let drift_low = needed_frames + MIC_DRIFT_LOW_OFFSET;
        let hard_trim_limit = (needed_frames + 2048).min(MIC_RING_FRAMES - 128);

        let mut avail = self.cons.occupied_len();
        if !self.primed {
            if avail < prefill_target {
                return &[];
            }
            self.primed = true;
        }

        if avail == 0 {
            self.primed = false;
            return &[];
        }

        // Emergency hard trim if buffer grew far beyond latency bounds (e.g., OS sleep/resume)
        if avail > hard_trim_limit {
            let mut discard = [[0.0f32; 2]; 64];
            while avail > prefill_target {
                let n = (avail - prefill_target).min(discard.len());
                let popped = self.cons.pop_slice(&mut discard[..n]);
                if popped == 0 {
                    break;
                }
                avail -= popped;
            }
        }

        let avail = self.cons.occupied_len();
        let take_frames = if avail > drift_high && avail >= needed_frames + 1 {
            needed_frames + 1
        } else if avail < drift_low && needed_frames >= 16 && avail >= needed_frames - 1 {
            needed_frames - 1
        } else {
            needed_frames.min(avail)
        };

        if take_frames == 0 {
            self.primed = false;
            return &[];
        }

        self.raw_scratch.resize(take_frames, [0.0, 0.0]);
        let actually_read = self.cons.pop_slice(&mut self.raw_scratch[..take_frames]);
        if actually_read == 0 {
            self.primed = false;
            return &[];
        }

        if actually_read == needed_frames {
            for f in &self.raw_scratch[..actually_read] {
                self.resampled_interleaved.push(f[0]);
                self.resampled_interleaved.push(f[1]);
            }
        } else if (actually_read == needed_frames + 1 || actually_read + 1 == needed_frames)
            && actually_read >= 2
            && needed_frames >= 2
        {
            // Smooth ±1-frame linear stretch/compress over the callback window for clock-drift lock
            let last = actually_read - 1;
            for out_idx in 0..needed_frames {
                let src_pos =
                    (out_idx as f64) * (last as f64) / ((needed_frames - 1) as f64);
                let idx0 = (src_pos.floor() as usize).min(last);
                let idx1 = (idx0 + 1).min(last);
                let frac = (src_pos - idx0 as f64) as f32;
                let l =
                    self.raw_scratch[idx0][0] * (1.0 - frac) + self.raw_scratch[idx1][0] * frac;
                let r =
                    self.raw_scratch[idx0][1] * (1.0 - frac) + self.raw_scratch[idx1][1] * frac;
                self.resampled_interleaved.push(l);
                self.resampled_interleaved.push(r);
            }
        } else {
            // Genuine underrun: ramp down gracefully to avoid step click, then re-prime
            let fade_len = actually_read.min(16);
            let fade_start = actually_read - fade_len;
            for (i, f) in self.raw_scratch[..actually_read].iter().enumerate() {
                let gain = if i >= fade_start && fade_len > 0 {
                    let rem = actually_read - 1 - i;
                    rem as f32 / fade_len as f32
                } else {
                    1.0
                };
                self.resampled_interleaved.push(f[0] * gain);
                self.resampled_interleaved.push(f[1] * gain);
            }
            while self.resampled_interleaved.len() < needed_frames * CHANNELS {
                self.resampled_interleaved.push(0.0);
                self.resampled_interleaved.push(0.0);
            }
            self.primed = false;
        }

        &self.resampled_interleaved
    }
}

#[derive(Default, Clone)]
pub(crate) struct DeviceSnapshot {
    pub(crate) active_input_id: Option<String>,
    pub(crate) active_broadcast_id: Option<String>,
    pub(crate) active_monitor_id: Option<String>,
    pub(crate) mic_stream_active: bool,
    pub(crate) last_error: Option<String>,
}

enum ActorCmd {
    GetDevices(mpsc::Sender<Result<AudioDevicesResult, String>>),
    RestartStreams {
        mixer: Arc<AudioMixer>,
        input_id: Option<String>,
        broadcast_id: Option<String>,
        monitor_id: Option<String>,
        reply: mpsc::Sender<Result<DeviceSnapshot, String>>,
    },
    #[cfg(target_os = "linux")]
    HotplugCheck,
    StopAll(mpsc::Sender<()>),
}

#[cfg(not(target_os = "linux"))]
struct StreamHolder {
    mic_stream: Option<Stream>,
    broadcast_stream: Option<Stream>,
    monitor_stream: Option<Stream>,
    snapshot: DeviceSnapshot,
}

#[cfg(not(target_os = "linux"))]
impl StreamHolder {
    fn new() -> Self {
        Self {
            mic_stream: None,
            broadcast_stream: None,
            monitor_stream: None,
            snapshot: DeviceSnapshot::default(),
        }
    }

    fn get_audio_devices(&self) -> Result<AudioDevicesResult, String> {
        let host = cpal::default_host();

        let default_input_name = host
            .default_input_device()
            .and_then(|d| d.name().ok());
        let default_output_name = host
            .default_output_device()
            .and_then(|d| d.name().ok());

        let mut input_devices = Vec::new();
        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    let is_virtual = is_virtual_endpoint(&name);
                    let is_default = default_input_name.as_deref() == Some(name.as_str());
                    input_devices.push(AudioDevice {
                        id: name.clone(),
                        name,
                        is_default,
                        is_input: true,
                        is_virtual,
                    });
                }
            }
        }

        let mut output_devices = Vec::new();
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    let is_virtual = is_virtual_endpoint(&name);
                    let is_default = default_output_name.as_deref() == Some(name.as_str());
                    output_devices.push(AudioDevice {
                        id: name.clone(),
                        name,
                        is_default,
                        is_input: false,
                        is_virtual,
                    });
                }
            }
        }

        Ok(AudioDevicesResult {
            input_devices,
            output_devices,
            current_input: self.snapshot.active_input_id.clone(),
            current_broadcast: self.snapshot.active_broadcast_id.clone(),
            current_monitor: self.snapshot.active_monitor_id.clone(),
        })
    }

    fn restart_streams(
        &mut self,
        mixer: Arc<AudioMixer>,
        input_id: Option<String>,
        broadcast_id: Option<String>,
        monitor_id: Option<String>,
    ) -> Result<DeviceSnapshot, String> {
        self.stop_all();
        mixer
            .controls
            .broadcast_stream_active
            .store(false, std::sync::atomic::Ordering::Release);
        mixer
            .controls
            .monitor_stream_active
            .store(false, std::sync::atomic::Ordering::Release);

        let host = cpal::default_host();
        let rb = HeapRb::<[f32; 2]>::new(MIC_RING_FRAMES);
        let (mic_prod, mic_cons) = rb.split();

        let mut errors: Vec<String> = Vec::new();

        // 1. Resolve physical microphone input (never select Memeboard Virtual Mic!)
        let mic_device = DeviceManager::resolve_input_device(&host, input_id.as_deref());
        if let Some(dev) = mic_device {
            let name = dev.name().unwrap_or_else(|_| "Unknown Input".to_string());
            match DeviceManager::start_mic_stream(&dev, mic_prod) {
                Ok(stream) => {
                    self.snapshot.active_input_id = Some(name);
                    self.mic_stream = Some(stream);
                    self.snapshot.mic_stream_active = true;
                }
                Err(e) => {
                    errors.push(format!("Microphone '{name}': {e}"));
                }
            }
        } else if let Some(requested) = input_id.as_deref() {
            errors.push(format!("Microphone '{requested}' was not found"));
        } else {
            errors.push("No physical microphone input device detected".to_string());
        }

        // 2. Resolve virtual broadcast output (strict match, NEVER fall back to speakers)
        let broadcast_device = DeviceManager::resolve_broadcast_device(&host, broadcast_id.as_deref());
        if let Some(dev) = broadcast_device {
            let name = dev.name().unwrap_or_else(|_| "Virtual Mic".to_string());
            let processor = mixer.create_broadcast_processor();
            match DeviceManager::start_broadcast_stream(
                &dev,
                processor,
                mic_cons,
                self.snapshot.mic_stream_active,
            ) {
                Ok(stream) => {
                    self.snapshot.active_broadcast_id = Some(name);
                    self.broadcast_stream = Some(stream);
                    mixer
                        .controls
                        .broadcast_stream_active
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                Err(e) => {
                    errors.push(format!("Virtual mic '{name}': {e}"));
                }
            }
        } else if let Some(requested) = broadcast_id.as_deref() {
            errors.push(format!("Virtual mic output '{requested}' was not found"));
        } else {
            errors.push(
                "Virtual mic output endpoint is not active yet (Memeboard Virtual Mic not attached)"
                    .to_string(),
            );
        }

        // 3. Resolve monitor headphones/speakers output (avoid virtual mic endpoints)
        let monitor_device = DeviceManager::resolve_monitor_device(
            &host,
            monitor_id.as_deref(),
            self.snapshot.active_broadcast_id.as_deref(),
        );
        if let Some(dev) = monitor_device {
            let name = dev.name().unwrap_or_else(|_| "Monitor Output".to_string());
            let processor = mixer.create_monitor_processor();
            match DeviceManager::start_monitor_stream(&dev, processor) {
                Ok(stream) => {
                    self.snapshot.active_monitor_id = Some(name);
                    self.monitor_stream = Some(stream);
                    mixer
                        .controls
                        .monitor_stream_active
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                Err(e) => {
                    errors.push(format!("Monitor output '{name}': {e}"));
                }
            }
        } else if let Some(requested) = monitor_id.as_deref() {
            errors.push(format!("Monitor output '{requested}' was not found"));
        }

        self.snapshot.last_error = if errors.is_empty() {
            None
        } else {
            Some(errors.join(" | "))
        };

        if let Some(err) = &self.snapshot.last_error {
            eprintln!("[Memeboard audio] Stream notices: {err}");
        }

        Ok(self.snapshot.clone())
    }

    fn stop_all(&mut self) {
        self.mic_stream = None;
        self.broadcast_stream = None;
        self.monitor_stream = None;
        self.snapshot = DeviceSnapshot::default();
    }
}

/// Thread-safe handle managing audio streams via a dedicated single-threaded audio control actor
/// so `!Send` OS stream handles never cross thread boundaries.
pub struct DeviceManager {
    tx: Sender<ActorCmd>,
    snapshot: Arc<Mutex<DeviceSnapshot>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<ActorCmd>();
        let snapshot = Arc::new(Mutex::new(DeviceSnapshot::default()));

        #[cfg(target_os = "linux")]
        {
            let snapshot_actor = Arc::clone(&snapshot);
            let tx_hotplug = tx.clone();
            let (hp_tx, hp_rx) = mpsc::channel::<crate::linux_audio_io::HotplugEvent>();

            std::thread::Builder::new()
                .name("memeboard-linux-hotplug".to_string())
                .spawn(move || {
                    while hp_rx.recv().is_ok() {
                        // Debounce rapid PulseAudio/PipeWire node changes (e.g. module setup or BT profile switch)
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        while hp_rx.try_recv().is_ok() {}
                        if tx_hotplug.send(ActorCmd::HotplugCheck).is_err() {
                            break;
                        }
                    }
                })
                .ok();

            std::thread::Builder::new()
                .name("memeboard-audio-actor".to_string())
                .spawn(move || {
                    let mut holder = crate::linux_audio_io::LinuxAudioHolder::new(Some(hp_tx));
                    let mut last_mixer: Option<Arc<AudioMixer>> = None;
                    let mut last_req: (Option<String>, Option<String>, Option<String>) =
                        (None, None, None);

                    while let Ok(cmd) = rx.recv() {
                        match cmd {
                            ActorCmd::GetDevices(reply) => {
                                let _ = reply.send(holder.get_audio_devices());
                            }
                            ActorCmd::RestartStreams {
                                mixer,
                                input_id,
                                broadcast_id,
                                monitor_id,
                                reply,
                            } => {
                                last_mixer = Some(Arc::clone(&mixer));
                                last_req = (
                                    input_id.clone(),
                                    broadcast_id.clone(),
                                    monitor_id.clone(),
                                );
                                let res = holder.restart_streams(
                                    mixer,
                                    input_id,
                                    broadcast_id,
                                    monitor_id,
                                );
                                if let Ok(ref snap) = res {
                                    *snapshot_actor.lock() = snap.clone();
                                }
                                let _ = reply.send(res);
                            }
                            ActorCmd::HotplugCheck => {
                                if let Some(mixer) = last_mixer.as_ref() {
                                    if let Ok((r_in, r_bcast, r_mon)) = holder.resolve_targets(
                                        last_req.0.as_deref(),
                                        last_req.1.as_deref(),
                                        last_req.2.as_deref(),
                                    ) {
                                        let changed = r_in != holder.snapshot.active_input_id
                                            || r_bcast != holder.snapshot.active_broadcast_id
                                            || r_mon != holder.snapshot.active_monitor_id;
                                        if changed {
                                            if let Ok(snap) = holder.restart_streams(
                                                Arc::clone(mixer),
                                                last_req.0.clone(),
                                                last_req.1.clone(),
                                                last_req.2.clone(),
                                            ) {
                                                *snapshot_actor.lock() = snap;
                                            }
                                        }
                                    }
                                }
                            }
                            ActorCmd::StopAll(reply) => {
                                holder.stop_all();
                                last_mixer = None;
                                *snapshot_actor.lock() = DeviceSnapshot::default();
                                let _ = reply.send(());
                            }
                        }
                    }
                    holder.stop_all();
                })
                .expect("Failed to spawn Linux audio device actor");
        }

        #[cfg(not(target_os = "linux"))]
        std::thread::Builder::new()
            .name("memeboard-audio-actor".to_string())
            .spawn(move || {
                let mut holder = StreamHolder::new();
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        ActorCmd::GetDevices(reply) => {
                            let _ = reply.send(holder.get_audio_devices());
                        }
                        ActorCmd::RestartStreams {
                            mixer,
                            input_id,
                            broadcast_id,
                            monitor_id,
                            reply,
                        } => {
                            let _ = reply.send(holder.restart_streams(
                                mixer,
                                input_id,
                                broadcast_id,
                                monitor_id,
                            ));
                        }
                        ActorCmd::StopAll(reply) => {
                            holder.stop_all();
                            let _ = reply.send(());
                        }
                    }
                }
                holder.stop_all();
            })
            .expect("Failed to spawn audio device actor");

        Self { tx, snapshot }
    }

    pub fn is_mic_stream_active(&self) -> bool {
        self.snapshot.lock().mic_stream_active
    }

    pub fn last_error(&self) -> Option<String> {
        self.snapshot.lock().last_error.clone()
    }

    pub fn get_audio_devices(&self) -> Result<AudioDevicesResult, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(ActorCmd::GetDevices(reply_tx))
            .map_err(|_| "Audio device manager actor is unavailable".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "Audio device enumeration failed".to_string())?
    }

    pub fn restart_streams(
        &mut self,
        mixer: Arc<AudioMixer>,
        input_id: Option<String>,
        broadcast_id: Option<String>,
        monitor_id: Option<String>,
    ) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(ActorCmd::RestartStreams {
                mixer,
                input_id,
                broadcast_id,
                monitor_id,
                reply: reply_tx,
            })
            .map_err(|_| "Audio device manager actor is unavailable".to_string())?;
        let snap = reply_rx
            .recv()
            .map_err(|_| "Audio stream restart timed out".to_string())??;
        *self.snapshot.lock() = snap;
        Ok(())
    }

    pub fn stop_all(&mut self) {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self.tx.send(ActorCmd::StopAll(reply_tx)).is_ok() {
            let _ = reply_rx.recv();
        }
        *self.snapshot.lock() = DeviceSnapshot::default();
    }
}

#[cfg(not(target_os = "linux"))]
impl DeviceManager {
    fn resolve_input_device(host: &cpal::Host, requested_id: Option<&str>) -> Option<Device> {
        let names: Vec<String> = host
            .input_devices()
            .ok()?
            .filter_map(|d| d.name().ok())
            .collect();

        let is_loop_endpoint = |n: &str| {
            let lower = n.to_lowercase();
            lower.contains("memeboard virtual mic") || lower.contains("cable output")
        };
        let is_mixer_or_virtual = |n: &str| {
            let lower = n.to_lowercase();
            is_virtual_endpoint(n)
                || lower.contains("sonic studio virtual mixer")
                || lower.contains("stereo mix")
        };

        if let Some(target) = requested_id {
            if !is_loop_endpoint(target) && names.iter().any(|n| n == target) {
                if let Some(dev) = Self::find_input_by_name(host, target) {
                    return Some(dev);
                }
            }
        }

        // Check OS default input — ONLY use it if it is a physical microphone!
        if let Some(default_dev) = host.default_input_device() {
            if let Ok(name) = default_dev.name() {
                if !is_virtual_endpoint(&name) {
                    return Some(default_dev);
                }
            }
        }

        // If Windows default was switched to Memeboard Virtual Mic, use saved physical mic name
        if let Some(saved_name) = policy::saved_original_physical_mic_name() {
            if !is_virtual_endpoint(&saved_name) {
                if let Some(dev) = Self::find_input_by_name(host, &saved_name) {
                    return Some(dev);
                }
            }
        }

        // Otherwise pick first genuine physical microphone (skipping virtual cables, Nahimic, Stereo Mix)
        if let Some(name) = names.iter().find(|n| !is_mixer_or_virtual(n)) {
            return Self::find_input_by_name(host, name);
        }

        names
            .iter()
            .find(|n| !is_virtual_endpoint(n))
            .and_then(|n| Self::find_input_by_name(host, n))
    }

    fn resolve_broadcast_device(host: &cpal::Host, requested_id: Option<&str>) -> Option<Device> {
        if let Some(target) = requested_id {
            if !target.to_lowercase().contains("sonic studio virtual mixer") {
                if let Some(dev) = Self::find_output_by_name(host, target) {
                    return Some(dev);
                }
            }
        }

        // Strictly auto-detect known virtual microphone broadcast targets; never fall back to speakers
        let mut best: Option<(u8, String)> = None;
        if let Ok(devs) = host.output_devices() {
            for dev in devs {
                if let Ok(name) = dev.name() {
                    if let Some(score) = broadcast_virtual_priority(&name) {
                        if best.as_ref().map_or(true, |(s, _)| score > *s) {
                            best = Some((score, name));
                        }
                    }
                }
            }
        }
        best.and_then(|(_, name)| Self::find_output_by_name(host, &name))
    }

    fn resolve_monitor_device(
        host: &cpal::Host,
        requested_id: Option<&str>,
        active_broadcast_name: Option<&str>,
    ) -> Option<Device> {
        if let Some(target) = requested_id {
            if Some(target) != active_broadcast_name {
                if let Some(dev) = Self::find_output_by_name(host, target) {
                    return Some(dev);
                }
            }
        }

        if let Some(default_dev) = host.default_output_device() {
            if let Ok(name) = default_dev.name() {
                if !is_virtual_endpoint(&name) && Some(name.as_str()) != active_broadcast_name {
                    return Some(default_dev);
                }
            }
        }

        if let Ok(devs) = host.output_devices() {
            for dev in devs {
                if let Ok(name) = dev.name() {
                    if !is_virtual_endpoint(&name) && Some(name.as_str()) != active_broadcast_name {
                        return Some(dev);
                    }
                }
            }
        }

        None
    }

    fn find_input_by_name(host: &cpal::Host, target: &str) -> Option<Device> {
        host.input_devices()
            .ok()?
            .find(|d| d.name().ok().as_deref() == Some(target))
    }

    fn find_output_by_name(host: &cpal::Host, target: &str) -> Option<Device> {
        host.output_devices()
            .ok()?
            .find(|d| d.name().ok().as_deref() == Some(target))
    }

    fn start_mic_stream(device: &Device, prod: HeapProd<[f32; 2]>) -> Result<Stream, String> {
        let supported = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {e}"))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let in_rate = config.sample_rate.0;
        let in_channels = (config.channels as usize).max(1);

        let resampler = if in_rate != SAMPLE_RATE {
            StreamingResampler::new(in_rate, SAMPLE_RATE)
        } else {
            None
        };

        let mut state = MicCaptureState {
            prod,
            in_channels,
            resampler,
        };

        let err_fn = |err| eprintln!("[Memeboard mic error] {err}");

        let stream = match sample_format {
            SampleFormat::F32 => device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        ensure_pro_audio_thread();
                        state.push_samples(data, |s| s);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build F32 mic stream: {e}"))?,
            SampleFormat::I16 => device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        ensure_pro_audio_thread();
                        state.push_samples(data, |s| s as f32 / 32768.0);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build I16 mic stream: {e}"))?,
            SampleFormat::U16 => device
                .build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        ensure_pro_audio_thread();
                        state.push_samples(data, |s| (s as f32 - 32768.0) / 32768.0);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build U16 mic stream: {e}"))?,
            SampleFormat::I32 => device
                .build_input_stream(
                    &config,
                    move |data: &[i32], _| {
                        ensure_pro_audio_thread();
                        state.push_samples(data, |s| (s as f64 / 2147483648.0) as f32);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build I32 mic stream: {e}"))?,
            other => {
                return Err(format!("Unsupported microphone sample format: {other:?}"));
            }
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start mic stream: {e}"))?;
        Ok(stream)
    }

    fn start_broadcast_stream(
        device: &Device,
        processor: StreamProcessor,
        mic_cons: HeapCons<[f32; 2]>,
        mic_stream_active: bool,
    ) -> Result<Stream, String> {
        let supported = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default broadcast config: {e}"))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let out_rate = config.sample_rate.0;
        let out_channels = (config.channels as usize).max(1);

        let resampler = if out_rate != SAMPLE_RATE {
            StreamingResampler::new(SAMPLE_RATE, out_rate)
        } else {
            None
        };

        let mut engine = BroadcastOutputEngine {
            processor,
            mic: MicConsumerState::new(mic_cons),
            mic_stream_active,
            out_channels,
            resampler,
            scratch_48k: vec![0.0; 8192 * CHANNELS],
        };

        let err_fn = |err| eprintln!("[Memeboard broadcast error] {err}");

        let stream = match sample_format {
            SampleFormat::F32 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [f32], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| v);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build F32 broadcast stream: {e}"))?,
            SampleFormat::I16 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [i16], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| (v.clamp(-1.0, 1.0) * 32767.0) as i16);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build I16 broadcast stream: {e}"))?,
            SampleFormat::U16 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [u16], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| {
                            ((v.clamp(-1.0, 1.0) * 32767.0) + 32768.0) as u16
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build U16 broadcast stream: {e}"))?,
            SampleFormat::I32 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [i32], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| {
                            (v.clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build I32 broadcast stream: {e}"))?,
            other => {
                return Err(format!("Unsupported broadcast sample format: {other:?}"));
            }
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start broadcast stream: {e}"))?;
        Ok(stream)
    }

    fn start_monitor_stream(
        device: &Device,
        processor: StreamProcessor,
    ) -> Result<Stream, String> {
        let supported = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default monitor config: {e}"))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let out_rate = config.sample_rate.0;
        let out_channels = (config.channels as usize).max(1);

        let resampler = if out_rate != SAMPLE_RATE {
            StreamingResampler::new(SAMPLE_RATE, out_rate)
        } else {
            None
        };

        let mut engine = MonitorOutputEngine {
            processor,
            out_channels,
            resampler,
            scratch_48k: vec![0.0; 8192 * CHANNELS],
        };

        let err_fn = |err| eprintln!("[Memeboard monitor error] {err}");

        let stream = match sample_format {
            SampleFormat::F32 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [f32], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| v);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build F32 monitor stream: {e}"))?,
            SampleFormat::I16 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [i16], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| (v.clamp(-1.0, 1.0) * 32767.0) as i16);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build I16 monitor stream: {e}"))?,
            SampleFormat::U16 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [u16], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| {
                            ((v.clamp(-1.0, 1.0) * 32767.0) + 32768.0) as u16
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build U16 monitor stream: {e}"))?,
            SampleFormat::I32 => device
                .build_output_stream(
                    &config,
                    move |data: &mut [i32], _| {
                        ensure_pro_audio_thread();
                        engine.render_into(data, |v| {
                            (v.clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build I32 monitor stream: {e}"))?,
            other => {
                return Err(format!("Unsupported monitor sample format: {other:?}"));
            }
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start monitor stream: {e}"))?;
        Ok(stream)
    }
}

pub(crate) struct MicCaptureState {
    pub(crate) prod: HeapProd<[f32; 2]>,
    pub(crate) in_channels: usize,
    pub(crate) resampler: Option<StreamingResampler>,
}

impl MicCaptureState {
    pub(crate) fn push_samples<T: Copy>(&mut self, data: &[T], convert: fn(T) -> f32) {
        let ch = self.in_channels;
        if ch == 0 {
            return;
        }
        let frames = data.len() / ch;
        for f in 0..frames {
            let base = f * ch;
            let (l, r) = if ch == 1 {
                let s = convert(data[base]);
                (s, s)
            } else {
                (convert(data[base]), convert(data[base + 1]))
            };

            if let Some(resampler) = &mut self.resampler {
                resampler.push_frame(l, r);
                while let Some(frame_48k) = resampler.pop_frame() {
                    let _ = self.prod.try_push(frame_48k);
                }
            } else {
                let _ = self.prod.try_push([l, r]);
            }
        }
    }
}

pub(crate) struct BroadcastOutputEngine {
    pub(crate) processor: StreamProcessor,
    pub(crate) mic: MicConsumerState,
    pub(crate) mic_stream_active: bool,
    pub(crate) out_channels: usize,
    pub(crate) resampler: Option<StreamingResampler>,
    pub(crate) scratch_48k: Vec<f32>,
}

impl BroadcastOutputEngine {
    pub(crate) fn render_into<T: Copy>(&mut self, data: &mut [T], encode: fn(f32) -> T) {
        let ch = self.out_channels;
        let out_frames = data.len() / ch;
        if out_frames == 0 {
            return;
        }

        if let Some(resampler) = &mut self.resampler {
            let mut iters = 0usize;
            while resampler.available_frames() < out_frames && iters < 128 {
                iters += 1;
                let mic_slice = self
                    .mic
                    .fill_mic_frames(RESAMPLE_CHUNK, self.mic_stream_active);
                let out_slice = &mut self.scratch_48k[..RESAMPLE_CHUNK * CHANNELS];
                self.processor.process_broadcast(mic_slice, out_slice);
                resampler.push_interleaved(out_slice);
            }
            for f in 0..out_frames {
                let [l, r] = resampler.pop_frame().unwrap_or([0.0, 0.0]);
                write_stereo_to_device(data, f, ch, l, r, encode);
            }
        } else {
            let frames = out_frames.min(self.scratch_48k.len() / CHANNELS);
            let mic_slice = self.mic.fill_mic_frames(frames, self.mic_stream_active);
            let out_slice = &mut self.scratch_48k[..frames * CHANNELS];
            self.processor.process_broadcast(mic_slice, out_slice);
            for f in 0..frames {
                write_stereo_to_device(
                    data,
                    f,
                    ch,
                    out_slice[f * 2],
                    out_slice[f * 2 + 1],
                    encode,
                );
            }
            for f in frames..out_frames {
                write_stereo_to_device(data, f, ch, 0.0, 0.0, encode);
            }
        }
    }
}

pub(crate) struct MonitorOutputEngine {
    pub(crate) processor: StreamProcessor,
    pub(crate) out_channels: usize,
    pub(crate) resampler: Option<StreamingResampler>,
    pub(crate) scratch_48k: Vec<f32>,
}

impl MonitorOutputEngine {
    pub(crate) fn render_into<T: Copy>(&mut self, data: &mut [T], encode: fn(f32) -> T) {
        let ch = self.out_channels;
        let out_frames = data.len() / ch;
        if out_frames == 0 {
            return;
        }

        if let Some(resampler) = &mut self.resampler {
            let mut iters = 0usize;
            while resampler.available_frames() < out_frames && iters < 128 {
                iters += 1;
                let out_slice = &mut self.scratch_48k[..RESAMPLE_CHUNK * CHANNELS];
                self.processor.process_monitor(out_slice);
                resampler.push_interleaved(out_slice);
            }
            for f in 0..out_frames {
                let [l, r] = resampler.pop_frame().unwrap_or([0.0, 0.0]);
                write_stereo_to_device(data, f, ch, l, r, encode);
            }
        } else {
            let frames = out_frames.min(self.scratch_48k.len() / CHANNELS);
            let out_slice = &mut self.scratch_48k[..frames * CHANNELS];
            self.processor.process_monitor(out_slice);
            for f in 0..frames {
                write_stereo_to_device(
                    data,
                    f,
                    ch,
                    out_slice[f * 2],
                    out_slice[f * 2 + 1],
                    encode,
                );
            }
            for f in frames..out_frames {
                write_stereo_to_device(data, f, ch, 0.0, 0.0, encode);
            }
        }
    }
}

#[inline]
fn write_stereo_to_device<T: Copy>(
    data: &mut [T],
    frame: usize,
    out_channels: usize,
    l: f32,
    r: f32,
    encode: fn(f32) -> T,
) {
    let base = frame * out_channels;
    if out_channels == 1 {
        data[base] = encode((l + r) * 0.5);
    } else if out_channels == 2 {
        data[base] = encode(l);
        data[base + 1] = encode(r);
    } else {
        data[base] = encode(l);
        data[base + 1] = encode(r);
        let zero = encode(0.0);
        for c in 2..out_channels {
            data[base + c] = zero;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_consumer_survives_wasapi_480_frame_jitter_without_dropout() {
        let rb = HeapRb::<[f32; 2]>::new(MIC_RING_FRAMES);
        let (mut prod, cons) = rb.split();
        let mut mic = MicConsumerState::new(cons);

        // First 480-frame push shouldn't prime yet (needs jitter cushion above 480)
        for i in 0..480 {
            prod.try_push([i as f32 * 0.001, -(i as f32) * 0.001]).unwrap();
        }
        assert!(mic.fill_mic_frames(480, true).is_empty());

        // Second 480-frame push reaches 960 -> primes with 480 frames of cushion remaining
        for i in 480..960 {
            prod.try_push([i as f32 * 0.001, -(i as f32) * 0.001]).unwrap();
        }
        let first = mic.fill_mic_frames(480, true);
        assert_eq!(first.len(), 480 * 2);
        assert!((first[0] - 0.0).abs() < 1e-6);
        assert!((first[1] - 0.0).abs() < 1e-6);

        // Simulate broadcast firing BEFORE the next mic callback arrives (480 frames still buffered!)
        let jittered = mic.fill_mic_frames(480, true);
        assert_eq!(jittered.len(), 480 * 2);
        assert!((jittered[0] - 0.480).abs() < 1e-5);
        assert!((jittered[1] - (-0.480)).abs() < 1e-5);
        assert!(mic.primed, "Should remain primed across 10ms callback jitter");
    }

    #[test]
    fn mic_consumer_underrun_fades_and_reprimes_cleanly() {
        let rb = HeapRb::<[f32; 2]>::new(MIC_RING_FRAMES);
        let (mut prod, cons) = rb.split();
        let mut mic = MicConsumerState::new(cons);

        for _ in 0..960 {
            prod.try_push([0.5, -0.5]).unwrap();
        }
        assert_eq!(mic.fill_mic_frames(480, true).len(), 480 * 2);
        assert_eq!(mic.fill_mic_frames(480, true).len(), 480 * 2);

        // Push only 100 frames (< 480) -> genuine underrun should fade tail to 0 and unprime
        for _ in 0..100 {
            prod.try_push([0.5, -0.5]).unwrap();
        }
        let underrun = mic.fill_mic_frames(480, true);
        assert_eq!(underrun.len(), 480 * 2);
        assert!((underrun[0] - 0.5).abs() < 1e-6);
        assert_eq!(underrun[underrun.len() - 1], 0.0);
        assert!(!mic.primed);
    }

    #[test]
    fn streaming_resampler_44100_to_48000_outputs_frames() {
        let mut rs = StreamingResampler::new(44_100, 48_000).unwrap();
        for i in 0..2048 {
            let s = (i as f32 * 0.05).sin() * 0.5;
            rs.push_frame(s, -s);
        }
        assert!(rs.available_frames() > 2000);
        let frame = rs.pop_frame().unwrap();
        assert!((frame[0] + frame[1]).abs() < 1e-4, "L/R mirror preserved");
    }

    #[test]
    fn virtual_endpoint_ignores_nahimic_and_detects_memeboard() {
        assert!(!is_virtual_endpoint("Speakers (Sonic Studio Virtual Mixer)"));
        assert!(!is_virtual_endpoint("Microphone (4- MMX 150)"));
        assert!(is_virtual_endpoint("Microphone (Memeboard Virtual Mic)"));
        assert!(is_virtual_endpoint("CABLE Input (VB-Audio Virtual Cable)"));
        assert_eq!(broadcast_virtual_priority("Speakers (Sonic Studio Virtual Mixer)"), None);
        assert_eq!(broadcast_virtual_priority("Speakers (Memeboard Virtual Mic)"), Some(100));
    }
}
