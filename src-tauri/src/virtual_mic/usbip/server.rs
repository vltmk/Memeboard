//! Local USB/IP server exporting our UAC2 virtual audio device.
//! Adapted from Tom56785/virtual-cables (MIT License).

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::busclock::{BusTime, Cancelled, current_bus_time, wait_bus_frame};
use super::protocol::*;
use crate::virtual_mic::event::Event;
use crate::virtual_mic::uac2::{self, Device, SetupPacket};

const MAX_TRANSFER_LENGTH: u32 = 16 * 1024 * 1024;
const MAX_POOLED_BUFFER: usize = 64 * 1024;
const MAX_POOLED_URBS: usize = 256;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn describe(err: &io::Error) -> String {
    if is_clean_eof(err) {
        "EOF".into()
    } else if err.kind() == io::ErrorKind::UnexpectedEof {
        "unexpected EOF".into()
    } else {
        err.to_string()
    }
}

pub struct Server {
    pub address: String,
    devices: Mutex<Vec<Arc<Device>>>,
    stopping: AtomicBool,
    local_addr: Mutex<Option<SocketAddr>>,
    connections: Mutex<HashMap<u64, Connection>>,
    next_connection: AtomicU64,
}

struct Connection {
    stream: TcpStream,
    bus_id: Option<String>,
}

impl Server {
    pub fn new(address: impl Into<String>, devices: Vec<Arc<Device>>) -> Server {
        Server {
            address: address.into(),
            devices: Mutex::new(devices),
            stopping: AtomicBool::new(false),
            local_addr: Mutex::new(None),
            connections: Mutex::new(HashMap::new()),
            next_connection: AtomicU64::new(1),
        }
    }

    pub fn serve(self: &Arc<Self>, ready: impl FnOnce(Result<(), String>)) -> Result<(), String> {
        let listener = match TcpListener::bind(&self.address) {
            Ok(listener) => listener,
            Err(err) => {
                let message = format!("listen tcp {}: {err}", self.address);
                ready(Err(message.clone()));
                return Err(message);
            }
        };
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        *lock(&self.local_addr) = Some(addr);
        ready(Ok(()));

        for conn in listener.incoming() {
            if self.stopping.load(Ordering::SeqCst) {
                return Ok(());
            }
            let conn = match conn {
                Ok(conn) => conn,
                Err(err) => return Err(err.to_string()),
            };
            let _ = conn.set_nodelay(true);
            let keepalive = socket2::TcpKeepalive::new()
                .with_time(Duration::from_secs(15))
                .with_interval(Duration::from_secs(15));
            let _ = socket2::SockRef::from(&conn).set_tcp_keepalive(&keepalive);
            let server = Arc::clone(self);
            std::thread::Builder::new()
                .name("memeboard-usbip-conn".into())
                .spawn(move || server.handle_connection(conn))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(mut addr) = *lock(&self.local_addr) {
            if addr.ip().is_unspecified() {
                addr.set_ip(match addr.ip() {
                    IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                    IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
                });
            }
            let _ = TcpStream::connect_timeout(&addr, Duration::from_secs(1));
        }
        for conn in lock(&self.connections).values() {
            let _ = conn.stream.shutdown(Shutdown::Both);
        }
    }

    fn handle_connection(self: Arc<Self>, conn: TcpStream) {
        let id = self.next_connection.fetch_add(1, Ordering::SeqCst);
        if let Ok(stream) = conn.try_clone() {
            lock(&self.connections).insert(id, Connection { stream, bus_id: None });
        }
        if self.stopping.load(Ordering::SeqCst) {
            let _ = conn.shutdown(Shutdown::Both);
        }
        self.serve_connection(id, &conn);
        lock(&self.connections).remove(&id);
    }

    fn serve_connection(self: &Arc<Self>, id: u64, conn: &TcpStream) {
        let (Ok(read_half), Ok(mut write_half)) = (conn.try_clone(), conn.try_clone()) else {
            return;
        };
        let _ = read_half.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = write_half.set_write_timeout(Some(Duration::from_secs(30)));
        let mut reader = ConnReader::new(read_half, 64 * 1024);
        let Ok(header) = read_op_header(&mut reader) else {
            return;
        };

        match header.code {
            OP_REQ_DEVLIST => {
                let _ = write_half.write_all(&self.device_list());
            }
            OP_REQ_IMPORT => {
                let mut bus_raw = [0u8; 32];
                if reader.read_exact(&mut bus_raw).is_err() {
                    return;
                }
                let bus_id = String::from_utf8_lossy(&bus_raw)
                    .trim_end_matches('\0')
                    .to_string();
                let Some(device) = self.import_device(id, &bus_id) else {
                    let mut reply = Vec::new();
                    append_op_header(&mut reply, OP_REP_IMPORT, 1);
                    let _ = write_half.write_all(&reply);
                    return;
                };
                let mut reply = Vec::new();
                append_op_header(&mut reply, OP_REP_IMPORT, 0);
                append_usb_device(&mut reply, &device);
                if write_half.write_all(&reply).is_err() {
                    return;
                }
                let _ = reader.get_ref().set_read_timeout(None);
                let _ = write_half.set_write_timeout(None);
                self.handle_urbs(reader, Box::new(write_half), device);
            }
            _ => {}
        }
    }

    pub(crate) fn device_list(&self) -> Vec<u8> {
        let mut b = Vec::new();
        append_op_header(&mut b, OP_REP_DEVLIST, 0);
        let devices = lock(&self.devices);
        b.extend_from_slice(&(devices.len() as u32).to_be_bytes());
        for d in devices.iter() {
            append_usb_device(&mut b, d);
            b.extend_from_slice(&[0x01, 0x01, 0x20, 0]);
            b.extend_from_slice(&[0x01, 0x02, 0x20, 0]);
            b.extend_from_slice(&[0x01, 0x02, 0x20, 0]);
        }
        b
    }

    fn import_device(&self, id: u64, bus_id: &str) -> Option<Arc<Device>> {
        let devices = lock(&self.devices);
        let device = devices.iter().find(|d| d.bus_id == bus_id).cloned()?;
        if let Some(conn) = lock(&self.connections).get_mut(&id) {
            conn.bus_id = Some(bus_id.to_string());
        }
        Some(device)
    }

    fn handle_urbs<R: Read>(
        self: &Arc<Self>,
        mut reader: ConnReader<R>,
        writer: Box<dyn Write + Send>,
        device: Arc<Device>,
    ) {
        let _audio_thread = crate::virtual_mic::windows_svc::enter_audio_thread();
        let state = Arc::new(ConnState::new(writer));
        let control_event = Event::new();
        let mut workers: HashMap<u32, EndpointWorker> = HashMap::new();

        loop {
            let basic = match read_basic_header(&mut reader) {
                Ok(basic) => basic,
                Err(err) => {
                    if !is_clean_eof(&err) && !self.stopping.load(Ordering::SeqCst) {
                        eprintln!(
                            "Memeboard USB/IP {} disconnected: {}",
                            device.bus_id,
                            describe(&err)
                        );
                    }
                    break;
                }
            };

            match basic.command {
                CMD_SUBMIT => {
                    let mut submit = match read_submit(&mut reader, basic, &state.pool) {
                        Ok(submit) => submit,
                        Err(_) => break,
                    };

                    if basic.endpoint == 0 || !submit.req.is_isochronous() {
                        let result = self.process_submit(
                            &state,
                            &device,
                            &mut submit,
                            None,
                            &control_event,
                        );
                        submit.release(&state.pool);
                        if result.is_err() {
                            break;
                        }
                        continue;
                    }

                    let worker = workers
                        .entry(basic.endpoint)
                        .or_insert_with(|| EndpointWorker::start(self, &state, &device));
                    let now = current_bus_time();
                    let slot = state.reserve_iso_completion(
                        basic.endpoint,
                        submit.req.number_of_packets,
                        now,
                    );
                    submit.req.start_frame = slot.start_frame;
                    submit.start = slot.start;
                    submit.complete_at = slot.deadline;
                    submit.complete_frame = slot.end_frame;

                    if basic.endpoint == 1 && basic.direction == DIRECTION_OUT {
                        device.write_playback(slot.start, iso_payloads(&submit.out, &submit.packets));
                    }

                    submit.tracked = true;
                    let cancel =
                        Arc::new(CancelToken::new(Some(Arc::clone(&worker.shared.event))));
                    state.add_pending(basic.sequence, basic.endpoint, Arc::clone(&cancel));
                    worker.push(Job { submit, cancel });
                }
                CMD_UNLINK => {
                    let target = match reader.read_fixed(UNLINK_BODY_SIZE) {
                        Ok(body) => u32::from_be_bytes([body[0], body[1], body[2], body[3]]),
                        Err(_) => break,
                    };
                    let (_, result) = state.unlink(&basic, target);
                    if result.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }

        state.cancel_all();
        for (_, worker) in workers {
            worker.stop();
        }
    }

    fn process_submit(
        &self,
        state: &ConnState,
        device: &Device,
        submit: &mut ParsedSubmit,
        cancel: Option<&CancelToken>,
        event: &Event,
    ) -> io::Result<()> {
        let req = submit.req;

        if req.basic.endpoint == 0 {
            let setup = match uac2::parse_setup(&req.setup) {
                Ok(setup) => setup,
                Err(_) => return state.write_submit(submit, STATUS_INVALID, 0, &[], &[], 0),
            };
            let (mut data, status) = device.handle_control(setup, &submit.out);
            if status == STATUS_OK {
                reset_timelines_after_control(state, &setup);
            }
            data.truncate(req.transfer_buffer_length as usize);
            let mut actual = data.len() as u32;
            if req.basic.direction == DIRECTION_OUT && status == STATUS_OK {
                actual = submit.out.len() as u32;
                data.clear();
            }
            return state.write_submit(submit, status, actual, &data, &[], 0);
        }

        let cancelled = || cancel.is_some_and(CancelToken::is_cancelled);
        let fail = |submit: &mut ParsedSubmit, status: i32| {
            mark_iso_error(&mut submit.packets, status);
            let error_count = submit.packets.len() as u32;
            state.write_submit(submit, status, 0, &[], &submit.packets, error_count)
        };
        if req.is_isochronous() && cancelled() {
            return fail(submit, STATUS_CONN_RESET);
        }

        if req.is_isochronous()
            && wait_iso_completion(event, submit.complete_at, submit.complete_frame, &cancelled)
                .is_err()
        {
            return fail(submit, STATUS_CONN_RESET);
        }

        match (req.basic.endpoint, req.basic.direction) {
            (1, DIRECTION_OUT) => {
                let actual = submit.out.len();
                mark_iso_packets(&mut submit.packets, actual);
                state.write_submit(submit, STATUS_OK, actual as u32, &[], &submit.packets, 0)
            }
            (2, DIRECTION_IN) => {
                let length = response_payload_length(&req, &submit.packets);
                submit.input.resize(length, 0);
                if submit.packets.is_empty() {
                    device.read_capture(submit.start, &mut submit.input, [length]);
                } else {
                    let lengths = submit.packets.iter().map(|p| p.length as usize);
                    device.read_capture(submit.start, &mut submit.input, lengths);
                }
                mark_iso_packets(&mut submit.packets, length);
                state.write_submit(
                    submit,
                    STATUS_OK,
                    length as u32,
                    &submit.input,
                    &submit.packets,
                    0,
                )
            }
            _ => fail(submit, STATUS_PIPE),
        }
    }
}

fn append_usb_device(b: &mut Vec<u8>, d: &Device) {
    let mut path = [0u8; 256];
    let mut bus_id = [0u8; 32];
    fixed_string(&mut path, &format!("/sys/devices/platform/memeboard/{}", d.bus_id));
    fixed_string(&mut bus_id, &d.bus_id);
    b.extend_from_slice(&path);
    b.extend_from_slice(&bus_id);
    let dev = &d.descriptors.device;
    let le16 = |at: usize| u16::from_le_bytes([dev[at], dev[at + 1]]);
    b.extend_from_slice(&1u32.to_be_bytes());
    b.extend_from_slice(&d.number.to_be_bytes());
    b.extend_from_slice(&SPEED_FULL.to_be_bytes());
    b.extend_from_slice(&le16(8).to_be_bytes());
    b.extend_from_slice(&le16(10).to_be_bytes());
    b.extend_from_slice(&le16(12).to_be_bytes());
    b.extend_from_slice(&[dev[4], dev[5], dev[6]]);
    b.push(1);
    b.push(dev[17]);
    b.push(3);
}

pub(crate) struct CancelToken {
    cancelled: AtomicBool,
    wake: Option<Arc<Event>>,
}

impl CancelToken {
    pub(crate) fn new(wake: Option<Arc<Event>>) -> CancelToken {
        CancelToken {
            cancelled: AtomicBool::new(false),
            wake,
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(wake) = &self.wake {
            wake.signal();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

struct PendingUrb {
    endpoint: u32,
    cancel: Arc<CancelToken>,
}

pub(crate) struct ConnState {
    writer: Mutex<WriterState>,
    sched: Mutex<Schedule>,
    pub(crate) pool: UrbPool,
}

struct WriterState {
    out: Box<dyn Write + Send>,
    frame: Vec<u8>,
}

#[derive(Default)]
struct Schedule {
    pending: HashMap<u32, PendingUrb>,
    iso_pending: HashMap<u32, i64>,
    iso_next: HashMap<u32, u64>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct IsoSlot {
    pub start_frame: u32,
    pub start: u64,
    pub end_frame: u64,
    pub deadline: Option<Instant>,
}

impl Schedule {
    fn reset_iso_timeline(&mut self, endpoint: u32) {
        if self.iso_pending.get(&endpoint).copied().unwrap_or(0) == 0 {
            self.iso_next.remove(&endpoint);
        }
    }

    fn claim_pending(&mut self, sequence: u32, unlinked: bool) -> bool {
        let Some(entry) = self.pending.remove(&sequence) else {
            return false;
        };
        *self.iso_pending.entry(entry.endpoint).or_default() -= 1;
        if unlinked {
            entry.cancel.cancel();
            self.reset_iso_timeline(entry.endpoint);
        }
        true
    }
}

impl ConnState {
    pub(crate) fn new(out: Box<dyn Write + Send>) -> ConnState {
        ConnState {
            writer: Mutex::new(WriterState {
                out,
                frame: Vec::new(),
            }),
            sched: Mutex::new(Schedule::default()),
            pool: UrbPool::default(),
        }
    }

    pub(crate) fn reset_idle_iso_timelines(&self) {
        let mut s = lock(&self.sched);
        let endpoints: Vec<u32> = s.iso_next.keys().copied().collect();
        for endpoint in endpoints {
            s.reset_iso_timeline(endpoint);
        }
    }

    pub(crate) fn clear_endpoint_halt(&self, endpoint: u32) {
        lock(&self.sched).reset_iso_timeline(endpoint);
    }

    pub(crate) fn reserve_iso_completion(
        &self,
        endpoint: u32,
        packets: u32,
        now: BusTime,
    ) -> IsoSlot {
        if packets == 0 {
            return IsoSlot {
                start_frame: now.frame() as u32,
                start: now.frame(),
                end_frame: 0,
                deadline: Some(now.wall),
            };
        }
        let packets = packets.min(255);
        let mut s = lock(&self.sched);
        let current = now.frame();
        let start = match s.iso_next.get(&endpoint) {
            Some(&next) if next > current => next,
            _ => current + 1,
        };
        let end = start + packets as u64;
        s.iso_next.insert(endpoint, end);
        IsoSlot {
            start_frame: start as u32,
            start,
            end_frame: end,
            deadline: Some(now.frame_start(end)),
        }
    }

    pub(crate) fn write_submit(
        &self,
        submit: &ParsedSubmit,
        status: i32,
        actual: u32,
        data: &[u8],
        packets: &[IsoPacket],
        error_count: u32,
    ) -> io::Result<()> {
        let mut w = lock(&self.writer);
        if submit.tracked && !lock(&self.sched).claim_pending(submit.req.basic.sequence, false) {
            return Ok(());
        }
        let w = &mut *w;
        w.frame.clear();
        append_ret_submit(
            &mut w.frame,
            &submit.req,
            status,
            actual,
            data,
            packets,
            error_count,
        );
        w.write_frame()
    }

    pub(crate) fn unlink(&self, req: &BasicHeader, target: u32) -> (bool, io::Result<()>) {
        let mut w = lock(&self.writer);
        let found = lock(&self.sched).claim_pending(target, true);
        let status = if found { STATUS_CONN_RESET } else { STATUS_OK };
        let w = &mut *w;
        w.frame.clear();
        append_ret_unlink(&mut w.frame, req, status);
        (found, w.write_frame())
    }

    pub(crate) fn add_pending(&self, sequence: u32, endpoint: u32, cancel: Arc<CancelToken>) {
        let mut s = lock(&self.sched);
        if let Some(previous) = s.pending.remove(&sequence) {
            previous.cancel.cancel();
            *s.iso_pending.entry(previous.endpoint).or_default() -= 1;
        }
        s.pending.insert(sequence, PendingUrb { endpoint, cancel });
        *s.iso_pending.entry(endpoint).or_default() += 1;
    }

    pub(crate) fn cancel_all(&self) {
        for entry in lock(&self.sched).pending.values() {
            entry.cancel.cancel();
        }
    }
}

impl WriterState {
    fn write_frame(&mut self) -> io::Result<()> {
        let result = self.out.write_all(&self.frame);
        if self.frame.capacity() > MAX_POOLED_BUFFER {
            self.frame = Vec::new();
        }
        result
    }
}

#[derive(Default)]
struct UrbBuffers {
    out: Vec<u8>,
    input: Vec<u8>,
    packets: Vec<IsoPacket>,
}

#[derive(Default)]
pub(crate) struct UrbPool(Mutex<Vec<UrbBuffers>>);

impl UrbPool {
    fn take(&self) -> UrbBuffers {
        lock(&self.0).pop().unwrap_or_default()
    }

    fn give(&self, mut buffers: UrbBuffers) {
        if buffers.out.capacity() > MAX_POOLED_BUFFER {
            buffers.out = Vec::new();
        }
        if buffers.input.capacity() > MAX_POOLED_BUFFER {
            buffers.input = Vec::new();
        }
        let mut pool = lock(&self.0);
        if pool.len() < MAX_POOLED_URBS {
            pool.push(buffers);
        }
    }
}

pub(crate) struct ParsedSubmit {
    pub req: SubmitRequest,
    pub out: Vec<u8>,
    pub packets: Vec<IsoPacket>,
    pub input: Vec<u8>,
    pub start: u64,
    pub complete_at: Option<Instant>,
    pub complete_frame: u64,
    pub tracked: bool,
}

impl ParsedSubmit {
    fn with_buffers(req: SubmitRequest, buffers: UrbBuffers) -> ParsedSubmit {
        ParsedSubmit {
            req,
            out: buffers.out,
            packets: buffers.packets,
            input: buffers.input,
            complete_at: None,
            start: 0,
            complete_frame: 0,
            tracked: false,
        }
    }

    pub(crate) fn release(self, pool: &UrbPool) {
        pool.give(UrbBuffers {
            out: self.out,
            input: self.input,
            packets: self.packets,
        });
    }
}

pub(crate) fn read_submit<R: Read>(
    reader: &mut ConnReader<R>,
    basic: BasicHeader,
    pool: &UrbPool,
) -> io::Result<ParsedSubmit> {
    let req = read_submit_body(reader, basic)?;
    if req.transfer_buffer_length > MAX_TRANSFER_LENGTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid transfer length {}", req.transfer_buffer_length),
        ));
    }
    let mut submit = ParsedSubmit::with_buffers(req, pool.take());
    submit.out.clear();
    if basic.direction == DIRECTION_OUT && req.transfer_buffer_length > 0 {
        submit.out.resize(req.transfer_buffer_length as usize, 0);
        if let Err(err) = reader.read_exact(&mut submit.out) {
            submit.release(pool);
            return Err(partial_eof(err));
        }
    }
    if let Err(err) = read_iso_packets(reader, req.number_of_packets, &mut submit.packets) {
        submit.release(pool);
        return Err(err);
    }
    Ok(submit)
}

struct Job {
    submit: ParsedSubmit,
    cancel: Arc<CancelToken>,
}

struct WorkerShared {
    queue: Mutex<VecDeque<Job>>,
    event: Arc<Event>,
    stopping: AtomicBool,
}

struct EndpointWorker {
    shared: Arc<WorkerShared>,
    thread: Option<JoinHandle<()>>,
}

impl EndpointWorker {
    fn start(
        server: &Arc<Server>,
        state: &Arc<ConnState>,
        device: &Arc<Device>,
    ) -> EndpointWorker {
        let shared = Arc::new(WorkerShared {
            queue: Mutex::new(VecDeque::new()),
            event: Arc::new(Event::new()),
            stopping: AtomicBool::new(false),
        });
        let (server, state, device, worker) = (
            Arc::clone(server),
            Arc::clone(state),
            Arc::clone(device),
            Arc::clone(&shared),
        );
        let thread = std::thread::Builder::new()
            .name("memeboard-usbip-ep".into())
            .spawn(move || worker.run(&server, &state, &device))
            .expect("start endpoint worker thread");
        EndpointWorker {
            shared,
            thread: Some(thread),
        }
    }

    fn push(&self, job: Job) {
        lock(&self.shared.queue).push_back(job);
        self.shared.event.signal();
    }

    fn stop(mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared.event.signal();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl WorkerShared {
    fn run(&self, server: &Server, state: &ConnState, device: &Device) {
        let _audio_thread = crate::virtual_mic::windows_svc::enter_audio_thread();
        loop {
            let job = lock(&self.queue).pop_front();
            let Some(mut job) = job else {
                if self.stopping.load(Ordering::SeqCst) {
                    return;
                }
                self.event.wait(None);
                continue;
            };
            let _ = server.process_submit(
                state,
                device,
                &mut job.submit,
                Some(&job.cancel),
                &self.event,
            );
            job.submit.release(&state.pool);
        }
    }
}

pub(crate) fn is_endpoint_halt_clear(setup: &SetupPacket) -> bool {
    setup.request_type == 0x02 && setup.request == uac2::REQUEST_CLEAR_FEATURE && setup.value == 0
}

pub(crate) fn reset_timelines_after_control(state: &ConnState, setup: &SetupPacket) {
    if is_endpoint_halt_clear(setup) {
        state.clear_endpoint_halt((setup.index & 0x0f) as u32);
    } else if (setup.request_type == 0x01 && setup.request == uac2::REQUEST_SET_INTERFACE)
        || (setup.request_type == 0x00 && setup.request == uac2::REQUEST_SET_CONFIGURATION)
    {
        state.reset_idle_iso_timelines();
    }
}

fn wait_iso_completion(
    event: &Event,
    complete_at: Option<Instant>,
    end_frame: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Cancelled> {
    let Some(complete_at) = complete_at else {
        return Ok(());
    };
    while Instant::now() < complete_at {
        if cancelled() {
            return Err(Cancelled);
        }
        event.wait(Some(complete_at));
    }
    wait_bus_frame(event, end_frame, cancelled)
}

fn response_payload_length(req: &SubmitRequest, packets: &[IsoPacket]) -> usize {
    if packets.is_empty() {
        return req.transfer_buffer_length as usize;
    }
    let mut total: u64 = packets.iter().map(|p| p.length as u64).sum();
    if req.transfer_buffer_length > 0 && total > req.transfer_buffer_length as u64 {
        total = req.transfer_buffer_length as u64;
    }
    total.min(MAX_TRANSFER_LENGTH as u64) as usize
}

fn iso_payloads<'a>(out: &'a [u8], packets: &'a [IsoPacket]) -> impl Iterator<Item = &'a [u8]> + 'a {
    packets.iter().map(move |p| {
        let start = p.offset as usize;
        start
            .checked_add(p.length as usize)
            .and_then(|end| out.get(start..end))
            .unwrap_or(&[])
    })
}

pub(crate) fn mark_iso_packets(packets: &mut [IsoPacket], actual_total: usize) {
    let mut remaining = actual_total;
    for packet in packets {
        packet.status = STATUS_OK;
        let wanted = (packet.length as usize).min(remaining);
        packet.actual_length = wanted as u32;
        remaining -= wanted;
    }
}

fn mark_iso_error(packets: &mut [IsoPacket], status: i32) {
    for packet in packets {
        packet.actual_length = 0;
        packet.status = status;
    }
}
