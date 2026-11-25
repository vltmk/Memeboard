//! USB/IP wire protocol (version 1.1.1).
//! Adapted from Tom56785/virtual-cables (MIT License).

use std::io::{self, Read};

pub const PROTOCOL_VERSION: u16 = 0x0111;

pub const OP_REQ_IMPORT: u16 = 0x8003;
pub const OP_REP_IMPORT: u16 = 0x0003;
pub const OP_REQ_DEVLIST: u16 = 0x8005;
pub const OP_REP_DEVLIST: u16 = 0x0005;

pub const CMD_SUBMIT: u32 = 0x0000_0001;
pub const CMD_UNLINK: u32 = 0x0000_0002;
pub const RET_SUBMIT: u32 = 0x0000_0003;
pub const RET_UNLINK: u32 = 0x0000_0004;

pub const DIRECTION_OUT: u32 = 0;
pub const DIRECTION_IN: u32 = 1;
pub const SPEED_FULL: u32 = 2;

pub const NO_ISO_PACKETS: u32 = 0xffff_ffff;

pub const STATUS_OK: i32 = 0;
pub const STATUS_INVALID: i32 = -22;
pub const STATUS_PIPE: i32 = -32;
pub const STATUS_CONN_RESET: i32 = -104;

pub const BASIC_HEADER_SIZE: usize = 20;
pub const SUBMIT_BODY_SIZE: usize = 28;
pub const UNLINK_BODY_SIZE: usize = 28;
pub const ISO_DESCRIPTOR_SIZE: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpHeader {
    pub version: u16,
    pub code: u16,
    pub status: u32,
}

pub fn read_op_header(r: &mut impl Read) -> io::Result<OpHeader> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    let h = OpHeader {
        version: u16::from_be_bytes([b[0], b[1]]),
        code: u16::from_be_bytes([b[2], b[3]]),
        status: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
    };
    if h.version != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported USB/IP version 0x{:04x}", h.version),
        ));
    }
    Ok(h)
}

pub fn append_op_header(dst: &mut Vec<u8>, code: u16, status: u32) {
    dst.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    dst.extend_from_slice(&code.to_be_bytes());
    dst.extend_from_slice(&status.to_be_bytes());
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BasicHeader {
    pub command: u32,
    pub sequence: u32,
    pub device_id: u32,
    pub direction: u32,
    pub endpoint: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubmitRequest {
    pub basic: BasicHeader,
    pub transfer_flags: u32,
    pub transfer_buffer_length: u32,
    pub start_frame: u32,
    pub number_of_packets: u32,
    pub interval: u32,
    pub setup: [u8; 8],
}

impl SubmitRequest {
    pub fn is_isochronous(&self) -> bool {
        self.number_of_packets != NO_ISO_PACKETS && self.number_of_packets != 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsoPacket {
    pub offset: u32,
    pub length: u32,
    pub actual_length: u32,
    pub status: i32,
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub struct ConnReader<R> {
    inner: R,
    buf: Box<[u8]>,
    pos: usize,
    end: usize,
}

impl<R: Read> ConnReader<R> {
    pub fn new(inner: R, capacity: usize) -> ConnReader<R> {
        ConnReader {
            inner,
            buf: vec![0; capacity].into_boxed_slice(),
            pos: 0,
            end: 0,
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    pub fn read_fixed(&mut self, n: usize) -> io::Result<&[u8]> {
        assert!(n <= self.buf.len());
        if self.end - self.pos < n {
            self.buf.copy_within(self.pos..self.end, 0);
            self.end -= self.pos;
            self.pos = 0;
            while self.end < n {
                match self.inner.read(&mut self.buf[self.end..]) {
                    Ok(0) if self.end == 0 => return Err(clean_eof()),
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "unexpected EOF",
                        ))
                    }
                    Ok(read) => self.end += read,
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                    Err(err) => return Err(err),
                }
            }
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.buf[start..start + n])
    }
}

impl<R: Read> Read for ConnReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos == self.end {
            if out.len() >= self.buf.len() {
                return self.inner.read(out);
            }
            self.pos = 0;
            self.end = self.inner.read(&mut self.buf)?;
        }
        let n = out.len().min(self.end - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[derive(Debug)]
struct CleanEof;

impl std::fmt::Display for CleanEof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EOF")
    }
}

impl std::error::Error for CleanEof {}

fn clean_eof() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, CleanEof)
}

pub fn is_clean_eof(err: &io::Error) -> bool {
    err.get_ref().is_some_and(|inner| inner.is::<CleanEof>())
}

pub fn read_basic_header<R: Read>(r: &mut ConnReader<R>) -> io::Result<BasicHeader> {
    let b = r.read_fixed(BASIC_HEADER_SIZE)?;
    Ok(BasicHeader {
        command: be32(b, 0),
        sequence: be32(b, 4),
        device_id: be32(b, 8),
        direction: be32(b, 12),
        endpoint: be32(b, 16),
    })
}

pub fn read_submit_body<R: Read>(
    r: &mut ConnReader<R>,
    basic: BasicHeader,
) -> io::Result<SubmitRequest> {
    let b = r.read_fixed(SUBMIT_BODY_SIZE).map_err(partial_eof)?;
    let mut setup = [0u8; 8];
    setup.copy_from_slice(&b[20..28]);
    Ok(SubmitRequest {
        basic,
        transfer_flags: be32(b, 0),
        transfer_buffer_length: be32(b, 4),
        start_frame: be32(b, 8),
        number_of_packets: be32(b, 12),
        interval: be32(b, 16),
        setup,
    })
}

pub fn partial_eof(err: io::Error) -> io::Error {
    if is_clean_eof(&err) {
        io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF")
    } else {
        err
    }
}

pub fn read_iso_packets<R: Read>(
    r: &mut ConnReader<R>,
    count: u32,
    dst: &mut Vec<IsoPacket>,
) -> io::Result<()> {
    dst.clear();
    if count == NO_ISO_PACKETS || count == 0 {
        return Ok(());
    }
    if count > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid isochronous packet count {count}"),
        ));
    }
    for _ in 0..count {
        let b = r.read_fixed(ISO_DESCRIPTOR_SIZE).map_err(partial_eof)?;
        dst.push(IsoPacket {
            offset: be32(b, 0),
            length: be32(b, 4),
            actual_length: be32(b, 8),
            status: be32(b, 12) as i32,
        });
    }
    Ok(())
}

pub fn append_ret_submit(
    dst: &mut Vec<u8>,
    req: &SubmitRequest,
    status: i32,
    mut actual_length: u32,
    mut data: &[u8],
    packets: &[IsoPacket],
    error_count: u32,
) {
    if status != STATUS_OK {
        actual_length = 0;
        data = &[];
    }
    let (number_of_packets, start_frame) = if req.is_isochronous() {
        (packets.len() as u32, req.start_frame)
    } else {
        (NO_ISO_PACKETS, 0)
    };

    dst.extend_from_slice(&RET_SUBMIT.to_be_bytes());
    dst.extend_from_slice(&req.basic.sequence.to_be_bytes());
    dst.extend_from_slice(&[0; 12]);
    dst.extend_from_slice(&(status as u32).to_be_bytes());
    dst.extend_from_slice(&actual_length.to_be_bytes());
    dst.extend_from_slice(&start_frame.to_be_bytes());
    dst.extend_from_slice(&number_of_packets.to_be_bytes());
    dst.extend_from_slice(&error_count.to_be_bytes());
    dst.extend_from_slice(&[0; 8]);
    dst.extend_from_slice(data);
    for p in packets {
        dst.extend_from_slice(&p.offset.to_be_bytes());
        dst.extend_from_slice(&p.length.to_be_bytes());
        dst.extend_from_slice(&p.actual_length.to_be_bytes());
        dst.extend_from_slice(&(p.status as u32).to_be_bytes());
    }
}

pub fn append_ret_unlink(dst: &mut Vec<u8>, request: &BasicHeader, status: i32) {
    dst.extend_from_slice(&RET_UNLINK.to_be_bytes());
    dst.extend_from_slice(&request.sequence.to_be_bytes());
    dst.extend_from_slice(&[0; 12]);
    dst.extend_from_slice(&(status as u32).to_be_bytes());
    dst.extend_from_slice(&[0; 24]);
}

pub fn fixed_string(dst: &mut [u8], value: &str) {
    dst.fill(0);
    let n = value.len().min(dst.len());
    dst[..n].copy_from_slice(&value.as_bytes()[..n]);
}
