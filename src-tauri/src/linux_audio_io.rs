//! Native PulseAudio / PipeWire (`pipewire-pulse`) low-latency audio I/O for Linux.
//! Reuses `DeviceManager`'s lock-free `MicCaptureState`, `BroadcastOutputEngine`, and
//! `MonitorOutputEngine` with exact sink/source targeting and hot-plug recovery.

use crate::audio_thread::{AudioThread, enter_audio_thread};
use crate::device_manager::{
    BroadcastOutputEngine, DeviceSnapshot, MIC_RING_FRAMES, MicCaptureState, MicConsumerState,
    MonitorOutputEngine, broadcast_virtual_priority, is_virtual_endpoint,
};
use crate::mixer::{AudioMixer, CHANNELS, SAMPLE_RATE, StreamProcessor};
use crate::types::{AudioDevice, AudioDevicesResult};
use crate::virtual_mic::linux_audio::{VIRTUAL_MIC_NAME, VIRTUAL_SINK_NAME};
use crate::virtual_mic::policy;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet, Operation as SubOp};
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use libpulse_binding::def::BufferAttr;
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::{FlagSet as StreamFlags, PeekResult, SeekMode, State as StreamState, Stream};
use ringbuf::traits::Split;
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

const CHUNK_FRAMES: usize = 480; // 10 ms at 48 kHz (matches MicConsumerState jitter math)
const FRAME_BYTES: usize = CHANNELS * std::mem::size_of::<f32>(); // 8 bytes

#[derive(Clone, Debug)]
struct PulseEndpoint {
    id: String,
    description: String,
    is_monitor: bool,
    is_virtual: bool,
}

pub enum HotplugEvent {
    DevicesChanged,
}

struct PulseSession {
    mainloop: Rc<RefCell<Mainloop>>,
    context: Rc<RefCell<Context>>,
}

impl PulseSession {
    fn connect(hotplug_tx: Option<Sender<HotplugEvent>>) -> Result<Self, String> {
        let mainloop = Rc::new(RefCell::new(
            Mainloop::new().ok_or_else(|| "Failed to create PulseAudio mainloop".to_string())?,
        ));
        let context = Rc::new(RefCell::new(
            Context::new(mainloop.borrow().deref(), "MemeboardAudio")
                .ok_or_else(|| "Failed to create PulseAudio context".to_string())?,
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
                .map_err(|e| format!("Failed to connect to PipeWire/PulseAudio: {e}"))?;
        }

        mainloop
            .borrow_mut()
            .start()
            .map_err(|e| format!("Failed to start PulseAudio mainloop: {e}"))?;

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let rem = deadline.saturating_duration_since(Instant::now());
            if rem.is_zero() {
                let _ = mainloop.borrow_mut().stop();
                return Err("Timed out connecting to PipeWire/PulseAudio".to_string());
            }
            match state_rx.recv_timeout(rem) {
                Ok(ContextState::Ready) => break,
                Ok(ContextState::Failed | ContextState::Terminated) => {
                    let _ = mainloop.borrow_mut().stop();
                    return Err("PipeWire/PulseAudio server connection failed".to_string());
                }
                Ok(_) => continue,
                Err(_) => {
                    let _ = mainloop.borrow_mut().stop();
                    return Err("Timed out waiting for PipeWire/PulseAudio".to_string());
                }
            }
        }

        mainloop.borrow_mut().lock();
        context.borrow_mut().set_state_callback(None);
        if let Some(tx) = hotplug_tx {
            let mut ctx = context.borrow_mut();
            ctx.set_subscribe_callback(Some(Box::new(move |fac, op, _| {
                let interesting_fac = matches!(
                    fac,
                    Some(Facility::Sink) | Some(Facility::Source) | Some(Facility::Server)
                );
                let interesting_op = matches!(
                    op,
                    Some(SubOp::New) | Some(SubOp::Removed) | Some(SubOp::Changed)
                );
                if interesting_fac && interesting_op {
                    let _ = tx.send(HotplugEvent::DevicesChanged);
                }
            })));
            let _ = ctx.subscribe(
                InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
                |_| {},
            );
        }
        mainloop.borrow_mut().unlock();

        Ok(Self { mainloop, context })
    }

    fn is_ready(&self) -> bool {
        self.mainloop.borrow_mut().lock();
        let ready = self.context.borrow().get_state() == ContextState::Ready;
        self.mainloop.borrow_mut().unlock();
        ready
    }

    fn query_topology(
        &self,
    ) -> Result<(Vec<PulseEndpoint>, Vec<PulseEndpoint>, Option<String>, Option<String>), String>
    {
        // 1. Server defaults
        let (srv_tx, srv_rx) = mpsc::channel();
        self.mainloop.borrow_mut().lock();
        let op_srv = self.context.borrow().introspect().get_server_info(move |info| {
            let def_sink = info.default_sink_name.as_ref().map(|s| s.to_string());
            let def_src = info.default_source_name.as_ref().map(|s| s.to_string());
            let _ = srv_tx.send((def_sink, def_src));
        });
        self.mainloop.borrow_mut().unlock();
        let (def_sink, def_src) = srv_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Timed out reading PipeWire/PulseAudio defaults".to_string())?;
        drop(op_srv);

        // 2. Sources
        let (src_tx, src_rx) = mpsc::channel();
        let src_items = Rc::new(RefCell::new(Vec::new()));
        let src_cb = Rc::clone(&src_items);
        self.mainloop.borrow_mut().lock();
        let op_src = self.context.borrow().introspect().get_source_info_list(move |res| match res {
            ListResult::Item(info) => {
                if let Some(name) = info.name.as_ref() {
                    let id = name.to_string();
                    let desc = info
                        .description
                        .as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| id.clone());
                    let is_monitor = info.monitor_of_sink.is_some();
                    let is_virtual = id == VIRTUAL_MIC_NAME
                        || is_monitor
                        || is_virtual_endpoint(&id)
                        || is_virtual_endpoint(&desc);
                    src_cb.borrow_mut().push(PulseEndpoint {
                        id,
                        description: desc,
                        is_monitor,
                        is_virtual,
                    });
                }
            }
            ListResult::End => {
                let _ = src_tx.send(Ok(std::mem::take(&mut *src_cb.borrow_mut())));
            }
            ListResult::Error => {
                let _ = src_tx.send(Err("Failed to enumerate audio sources".to_string()));
            }
        });
        self.mainloop.borrow_mut().unlock();
        let sources = src_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Timed out enumerating audio sources".to_string())??;
        drop(op_src);

        // 3. Sinks
        let (sink_tx, sink_rx) = mpsc::channel();
        let sink_items = Rc::new(RefCell::new(Vec::new()));
        let sink_cb = Rc::clone(&sink_items);
        self.mainloop.borrow_mut().lock();
        let op_sink = self.context.borrow().introspect().get_sink_info_list(move |res| match res {
            ListResult::Item(info) => {
                if let Some(name) = info.name.as_ref() {
                    let id = name.to_string();
                    let desc = info
                        .description
                        .as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| id.clone());
                    let is_virtual = id == VIRTUAL_SINK_NAME
                        || is_virtual_endpoint(&id)
                        || is_virtual_endpoint(&desc);
                    sink_cb.borrow_mut().push(PulseEndpoint {
                        id,
                        description: desc,
                        is_monitor: false,
                        is_virtual,
                    });
                }
            }
            ListResult::End => {
                let _ = sink_tx.send(Ok(std::mem::take(&mut *sink_cb.borrow_mut())));
            }
            ListResult::Error => {
                let _ = sink_tx.send(Err("Failed to enumerate audio sinks".to_string()));
            }
        });
        self.mainloop.borrow_mut().unlock();
        let sinks = sink_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Timed out enumerating audio sinks".to_string())??;
        drop(op_sink);

        Ok((sources, sinks, def_src, def_sink))
    }
}

impl Drop for PulseSession {
    fn drop(&mut self) {
        self.mainloop.borrow_mut().lock();
        self.context.borrow_mut().set_subscribe_callback(None);
        self.context.borrow_mut().disconnect();
        self.mainloop.borrow_mut().unlock();
        self.mainloop.borrow_mut().stop();
    }
}

struct ActiveStreams {
    mic_stream: Option<Rc<RefCell<Stream>>>,
    broadcast_stream: Option<Rc<RefCell<Stream>>>,
    monitor_stream: Option<Rc<RefCell<Stream>>>,
}

impl ActiveStreams {
    fn stop(&mut self, mainloop: &Rc<RefCell<Mainloop>>) {
        mainloop.borrow_mut().lock();
        for slot in [
            &mut self.mic_stream,
            &mut self.broadcast_stream,
            &mut self.monitor_stream,
        ] {
            if let Some(stream) = slot.take() {
                {
                    let mut s = stream.borrow_mut();
                    s.set_read_callback(None);
                    s.set_write_callback(None);
                    s.set_state_callback(None);
                    let _ = s.disconnect();
                }
            }
        }
        mainloop.borrow_mut().unlock();
    }
}

pub struct LinuxAudioHolder {
    session: Option<Rc<PulseSession>>,
    streams: ActiveStreams,
    hotplug_tx: Option<Sender<HotplugEvent>>,
    pub(crate) snapshot: DeviceSnapshot,
}

impl LinuxAudioHolder {
    pub fn new(hotplug_tx: Option<Sender<HotplugEvent>>) -> Self {
        Self {
            session: None,
            streams: ActiveStreams {
                mic_stream: None,
                broadcast_stream: None,
                monitor_stream: None,
            },
            hotplug_tx,
            snapshot: DeviceSnapshot::default(),
        }
    }

    fn ensure_session(&mut self) -> Result<Rc<PulseSession>, String> {
        let needs_reconnect = self.session.as_ref().is_none_or(|s| !s.is_ready());
        if needs_reconnect {
            if let Some(old) = self.session.take() {
                self.streams.stop(&old.mainloop);
            }
            self.session = Some(Rc::new(PulseSession::connect(self.hotplug_tx.clone())?));
        }
        Ok(Rc::clone(self.session.as_ref().unwrap()))
    }

    pub fn get_audio_devices(&mut self) -> Result<AudioDevicesResult, String> {
        let session = self.ensure_session()?;
        let (sources, sinks, def_src, def_sink) = session.query_topology()?;

        let input_devices = sources
            .into_iter()
            .filter(|s| !s.is_monitor || s.id == VIRTUAL_MIC_NAME)
            .map(|s| {
                let is_default = def_src.as_deref() == Some(s.id.as_str());
                AudioDevice {
                    id: s.id,
                    name: s.description,
                    is_default,
                    is_input: true,
                    is_virtual: s.is_virtual,
                }
            })
            .collect();

        let output_devices = sinks
            .into_iter()
            .map(|s| {
                let is_default = def_sink.as_deref() == Some(s.id.as_str());
                AudioDevice {
                    id: s.id,
                    name: s.description,
                    is_default,
                    is_input: false,
                    is_virtual: s.is_virtual,
                }
            })
            .collect();

        Ok(AudioDevicesResult {
            input_devices,
            output_devices,
            current_input: self.snapshot.active_input_id.clone(),
            current_broadcast: self.snapshot.active_broadcast_id.clone(),
            current_monitor: self.snapshot.active_monitor_id.clone(),
        })
    }

    pub fn resolve_targets(
        &mut self,
        input_id: Option<&str>,
        broadcast_id: Option<&str>,
        monitor_id: Option<&str>,
    ) -> Result<(Option<String>, Option<String>, Option<String>), String> {
        let session = self.ensure_session()?;
        let (sources, sinks, def_src, def_sink) = session.query_topology()?;

        let resolved_in = resolve_input(&sources, def_src.as_deref(), input_id).map(|e| e.id);
        let resolved_bcast = resolve_broadcast(&sinks, broadcast_id).map(|e| e.id);
        let resolved_mon =
            resolve_monitor(&sinks, def_sink.as_deref(), monitor_id, resolved_bcast.as_deref())
                .map(|e| e.id);

        Ok((resolved_in, resolved_bcast, resolved_mon))
    }

    pub fn restart_streams(
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

        let session = self.ensure_session()?;
        let (sources, sinks, def_src, def_sink) = session.query_topology()?;

        let rb = HeapRb::<[f32; 2]>::new(MIC_RING_FRAMES);
        let (mic_prod, mic_cons) = rb.split();

        let mut errors: Vec<String> = Vec::new();

        // 1. Physical microphone capture
        if let Some(ep) = resolve_input(&sources, def_src.as_deref(), input_id.as_deref()) {
            match start_mic_stream(&session, &ep.id, mic_prod) {
                Ok(stream) => {
                    self.snapshot.active_input_id = Some(ep.id);
                    self.streams.mic_stream = Some(stream);
                    self.snapshot.mic_stream_active = true;
                }
                Err(e) => errors.push(format!("Microphone '{}': {e}", ep.description)),
            }
        } else if let Some(req) = input_id.as_deref() {
            errors.push(format!("Microphone '{req}' was not found"));
        } else {
            errors.push("No physical microphone input device detected".to_string());
        }

        // 2. Broadcast virtual sink output
        if let Some(ep) = resolve_broadcast(&sinks, broadcast_id.as_deref()) {
            let processor = mixer.create_broadcast_processor();
            match start_broadcast_stream(
                &session,
                &ep.id,
                processor,
                mic_cons,
                self.snapshot.mic_stream_active,
            ) {
                Ok(stream) => {
                    self.snapshot.active_broadcast_id = Some(ep.id);
                    self.streams.broadcast_stream = Some(stream);
                    mixer
                        .controls
                        .broadcast_stream_active
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                Err(e) => errors.push(format!("Virtual mic '{}': {e}", ep.description)),
            }
        } else if let Some(req) = broadcast_id.as_deref() {
            errors.push(format!("Virtual mic output '{req}' was not found"));
        } else {
            errors.push(
                "Virtual mic output endpoint is not active yet (Memeboard Virtual Mic not attached)"
                    .to_string(),
            );
        }

        // 3. Monitor headphones output
        if let Some(ep) = resolve_monitor(
            &sinks,
            def_sink.as_deref(),
            monitor_id.as_deref(),
            self.snapshot.active_broadcast_id.as_deref(),
        ) {
            let processor = mixer.create_monitor_processor();
            match start_monitor_stream(&session, &ep.id, processor) {
                Ok(stream) => {
                    self.snapshot.active_monitor_id = Some(ep.id);
                    self.streams.monitor_stream = Some(stream);
                    mixer
                        .controls
                        .monitor_stream_active
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                Err(e) => errors.push(format!("Monitor output '{}': {e}", ep.description)),
            }
        } else if let Some(req) = monitor_id.as_deref() {
            errors.push(format!("Monitor output '{req}' was not found"));
        }

        self.snapshot.last_error = if errors.is_empty() {
            None
        } else {
            Some(errors.join(" | "))
        };
        if let Some(err) = &self.snapshot.last_error {
            eprintln!("[Memeboard Linux audio] Stream notices: {err}");
        }

        Ok(self.snapshot.clone())
    }

    pub fn stop_all(&mut self) {
        if let Some(session) = &self.session {
            self.streams.stop(&session.mainloop);
        }
        self.snapshot = DeviceSnapshot::default();
    }
}

impl Drop for LinuxAudioHolder {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn resolve_input(
    sources: &[PulseEndpoint],
    default_source: Option<&str>,
    requested: Option<&str>,
) -> Option<PulseEndpoint> {
    let is_loop = |ep: &PulseEndpoint| {
        ep.is_monitor
            || ep.id == VIRTUAL_MIC_NAME
            || ep.is_virtual
            || ep.description.to_lowercase().contains("memeboard virtual mic")
    };

    if let Some(req) = requested {
        if let Some(found) = sources
            .iter()
            .find(|s| !is_loop(s) && (s.id == req || s.description == req))
        {
            return Some(found.clone());
        }
    }

    if let Some(def_id) = default_source {
        if let Some(found) = sources.iter().find(|s| s.id == def_id && !is_loop(s)) {
            return Some(found.clone());
        }
    }

    if let Some(saved_name) = policy::saved_original_physical_mic_name() {
        if let Some(found) = sources.iter().find(|s| {
            !is_loop(s) && (s.id == saved_name || s.description == saved_name)
        }) {
            return Some(found.clone());
        }
    }

    sources.iter().find(|s| !is_loop(s)).cloned()
}

fn resolve_broadcast(sinks: &[PulseEndpoint], requested: Option<&str>) -> Option<PulseEndpoint> {
    if let Some(req) = requested {
        if let Some(found) = sinks
            .iter()
            .find(|s| s.id == req || s.description == req)
        {
            return Some(found.clone());
        }
    }

    if let Some(v) = sinks.iter().find(|s| s.id == VIRTUAL_SINK_NAME) {
        return Some(v.clone());
    }

    let mut best: Option<(u8, PulseEndpoint)> = None;
    for s in sinks {
        if let Some(score) = broadcast_virtual_priority(&s.description) {
            if best.as_ref().map_or(true, |(b, _)| score > *b) {
                best = Some((score, s.clone()));
            }
        }
    }
    best.map(|(_, ep)| ep)
}

fn resolve_monitor(
    sinks: &[PulseEndpoint],
    default_sink: Option<&str>,
    requested: Option<&str>,
    active_broadcast_id: Option<&str>,
) -> Option<PulseEndpoint> {
    let valid_mon = |s: &PulseEndpoint| {
        !s.is_virtual && Some(s.id.as_str()) != active_broadcast_id
    };

    if let Some(req) = requested {
        if let Some(found) = sinks.iter().find(|s| {
            Some(s.id.as_str()) != active_broadcast_id
                && (s.id == req || s.description == req)
        }) {
            return Some(found.clone());
        }
    }

    if let Some(def_id) = default_sink {
        if let Some(found) = sinks.iter().find(|s| s.id == def_id && valid_mon(s)) {
            return Some(found.clone());
        }
    }

    sinks.iter().find(|s| valid_mon(s)).cloned()
}

fn audio_spec() -> Spec {
    Spec {
        format: Format::FLOAT32NE,
        channels: CHANNELS as u8,
        rate: SAMPLE_RATE,
    }
}

fn stream_buffer_attr() -> BufferAttr {
    let chunk_bytes = (CHUNK_FRAMES * FRAME_BYTES) as u32;
    BufferAttr {
        maxlength: u32::MAX,
        tlength: chunk_bytes * 2,
        prebuf: chunk_bytes,
        minreq: chunk_bytes,
        fragsize: chunk_bytes,
    }
}

fn wait_stream_ready(
    session: &PulseSession,
    stream: &Rc<RefCell<Stream>>,
    label: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        session.mainloop.borrow_mut().lock();
        let state = stream.borrow().get_state();
        session.mainloop.borrow_mut().unlock();

        match state {
            StreamState::Ready => return Ok(()),
            StreamState::Failed | StreamState::Terminated => {
                return Err(format!("{label} stream entered failed state"));
            }
            _ => {
                if Instant::now() >= deadline {
                    return Err(format!("Timed out waiting for {label} stream"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn start_mic_stream(
    session: &PulseSession,
    source_name: &str,
    prod: HeapProd<[f32; 2]>,
) -> Result<Rc<RefCell<Stream>>, String> {
    let spec = audio_spec();
    let attr = stream_buffer_attr();

    session.mainloop.borrow_mut().lock();
    let stream = Rc::new(RefCell::new(
        Stream::new(&mut session.context.borrow_mut(), "Memeboard Mic Capture", &spec, None)
            .ok_or_else(|| "Failed to allocate PulseAudio capture stream".to_string())?,
    ));

    let mut mic_state = MicCaptureState {
        prod,
        in_channels: CHANNELS,
        resampler: None,
    };
    let mut f32_scratch: Vec<f32> = Vec::with_capacity(2048);
    let mut rt_guard: Option<AudioThread> = None;

    let stream_cb = Rc::clone(&stream);
    stream.borrow_mut().set_read_callback(Some(Box::new(move |_| {
        if rt_guard.is_none() {
            rt_guard = Some(enter_audio_thread());
        }
        let mut s = stream_cb.borrow_mut();
        loop {
            match s.peek() {
                Ok(PeekResult::Data(bytes)) => {
                    f32_scratch.clear();
                    for c in bytes.chunks_exact(4) {
                        f32_scratch.push(f32::from_ne_bytes([c[0], c[1], c[2], c[3]]));
                    }
                    mic_state.push_samples(&f32_scratch, |v| v);
                    if s.discard().is_err() {
                        break;
                    }
                }
                Ok(PeekResult::Hole(_)) => {
                    if s.discard().is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    })));

    let res = stream.borrow_mut().connect_record(
        Some(source_name),
        Some(&attr),
        StreamFlags::ADJUST_LATENCY | StreamFlags::AUTO_TIMING_UPDATE,
    );
    session.mainloop.borrow_mut().unlock();

    res.map_err(|e| format!("connect_record failed: {e}"))?;
    if let Err(err) = wait_stream_ready(session, &stream, "mic capture") {
        session.mainloop.borrow_mut().lock();
        stream.borrow_mut().set_read_callback(None);
        let _ = stream.borrow_mut().disconnect();
        session.mainloop.borrow_mut().unlock();
        return Err(err);
    }

    Ok(stream)
}

fn start_broadcast_stream(
    session: &PulseSession,
    sink_name: &str,
    processor: StreamProcessor,
    mic_cons: HeapCons<[f32; 2]>,
    mic_stream_active: bool,
) -> Result<Rc<RefCell<Stream>>, String> {
    let spec = audio_spec();
    let attr = stream_buffer_attr();

    session.mainloop.borrow_mut().lock();
    let stream = Rc::new(RefCell::new(
        Stream::new(&mut session.context.borrow_mut(), "Memeboard Virtual Mic Mix", &spec, None)
            .ok_or_else(|| "Failed to allocate PulseAudio broadcast stream".to_string())?,
    ));

    let mut engine = BroadcastOutputEngine {
        processor,
        mic: MicConsumerState::new(mic_cons),
        mic_stream_active,
        out_channels: CHANNELS,
        resampler: None,
        scratch_48k: vec![0.0; CHUNK_FRAMES * CHANNELS],
    };
    let mut out_f32 = vec![0.0f32; CHUNK_FRAMES * CHANNELS];
    let mut rt_guard: Option<AudioThread> = None;

    let stream_cb = Rc::clone(&stream);
    stream.borrow_mut().set_write_callback(Some(Box::new(move |nbytes| {
        if rt_guard.is_none() {
            rt_guard = Some(enter_audio_thread());
        }
        let mut frames_left = nbytes / FRAME_BYTES;
        let mut s = stream_cb.borrow_mut();
        while frames_left > 0 {
            let chunk = frames_left.min(CHUNK_FRAMES);
            let slice = &mut out_f32[..chunk * CHANNELS];
            engine.render_into(slice, |v| v);
            let bytes = unsafe {
                std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), chunk * FRAME_BYTES)
            };
            if s.write_copy(bytes, 0, SeekMode::Relative).is_err() {
                break;
            }
            frames_left -= chunk;
        }
    })));

    let res = stream.borrow_mut().connect_playback(
        Some(sink_name),
        Some(&attr),
        StreamFlags::ADJUST_LATENCY | StreamFlags::AUTO_TIMING_UPDATE,
        None,
        None,
    );
    session.mainloop.borrow_mut().unlock();

    res.map_err(|e| format!("connect_playback failed: {e}"))?;
    if let Err(err) = wait_stream_ready(session, &stream, "broadcast") {
        session.mainloop.borrow_mut().lock();
        stream.borrow_mut().set_write_callback(None);
        let _ = stream.borrow_mut().disconnect();
        session.mainloop.borrow_mut().unlock();
        return Err(err);
    }

    Ok(stream)
}

fn start_monitor_stream(
    session: &PulseSession,
    sink_name: &str,
    processor: StreamProcessor,
) -> Result<Rc<RefCell<Stream>>, String> {
    let spec = audio_spec();
    let attr = stream_buffer_attr();

    session.mainloop.borrow_mut().lock();
    let stream = Rc::new(RefCell::new(
        Stream::new(&mut session.context.borrow_mut(), "Memeboard Headphones Monitor", &spec, None)
            .ok_or_else(|| "Failed to allocate PulseAudio monitor stream".to_string())?,
    ));

    let mut engine = MonitorOutputEngine {
        processor,
        out_channels: CHANNELS,
        resampler: None,
        scratch_48k: vec![0.0; CHUNK_FRAMES * CHANNELS],
    };
    let mut out_f32 = vec![0.0f32; CHUNK_FRAMES * CHANNELS];
    let mut rt_guard: Option<AudioThread> = None;

    let stream_cb = Rc::clone(&stream);
    stream.borrow_mut().set_write_callback(Some(Box::new(move |nbytes| {
        if rt_guard.is_none() {
            rt_guard = Some(enter_audio_thread());
        }
        let mut frames_left = nbytes / FRAME_BYTES;
        let mut s = stream_cb.borrow_mut();
        while frames_left > 0 {
            let chunk = frames_left.min(CHUNK_FRAMES);
            let slice = &mut out_f32[..chunk * CHANNELS];
            engine.render_into(slice, |v| v);
            let bytes = unsafe {
                std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), chunk * FRAME_BYTES)
            };
            if s.write_copy(bytes, 0, SeekMode::Relative).is_err() {
                break;
            }
            frames_left -= chunk;
        }
    })));

    let res = stream.borrow_mut().connect_playback(
        Some(sink_name),
        Some(&attr),
        StreamFlags::ADJUST_LATENCY | StreamFlags::AUTO_TIMING_UPDATE,
        None,
        None,
    );
    session.mainloop.borrow_mut().unlock();

    res.map_err(|e| format!("connect_playback failed: {e}"))?;
    if let Err(err) = wait_stream_ready(session, &stream, "monitor") {
        session.mainloop.borrow_mut().lock();
        stream.borrow_mut().set_write_callback(None);
        let _ = stream.borrow_mut().disconnect();
        session.mainloop.borrow_mut().unlock();
        return Err(err);
    }

    Ok(stream)
}
