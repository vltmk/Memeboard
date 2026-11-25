//! A virtual audio device's delay line addressed by USB bus frame.
//! Adapted from Tom56785/virtual-cables (MIT License).

use std::sync::Mutex;

/// Bytes per PCM frame: stereo, 24-bit.
pub const FRAME_BYTES: usize = 6;
/// Bytes per 1 ms packet at 48 kHz: 48 PCM frames.
pub const PACKET_BYTES: usize = 48 * FRAME_BYTES;
const QUEUE_SLOTS: u64 = 1024;
const EMPTY: u64 = u64::MAX;
const CATCH_UP_FRAMES: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gain {
    Unity,
    Scale(f32),
}

pub struct DelayLine {
    delay: u64,
    slots: u64,
    state: Mutex<LineState>,
}

struct LineState {
    audio: Vec<u8>,
    frames: Vec<u64>,
    playback: Option<Segment>,
    playback_first: Option<u64>,
    last_read: Option<u64>,
    gain: Gain,
    missing: u64,
    realigned: u64,
    caught_up: u64,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    next: u64,
    offset: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineStatus {
    pub missing: u64,
    pub realigned: u64,
    pub caught_up: u64,
}

impl DelayLine {
    pub fn new(delay: u64) -> DelayLine {
        let slots = delay + QUEUE_SLOTS;
        DelayLine {
            delay,
            slots,
            state: Mutex::new(LineState {
                audio: vec![0; slots as usize * PACKET_BYTES],
                frames: vec![EMPTY; slots as usize],
                playback: None,
                playback_first: None,
                last_read: None,
                gain: Gain::Unity,
                missing: 0,
                realigned: 0,
                caught_up: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LineState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn delay(&self) -> u64 {
        self.delay
    }

    pub fn status(&self) -> LineStatus {
        let s = self.lock();
        LineStatus {
            missing: s.missing,
            realigned: s.realigned,
            caught_up: s.caught_up,
        }
    }

    pub fn set_gain(&self, gain: Gain) {
        self.lock().gain = gain;
    }

    pub fn write<'a>(&self, start: u64, packets: impl IntoIterator<Item = &'a [u8]>) {
        let mut guard = self.lock();
        let s = &mut *guard;
        let mut segment = match s.playback {
            Some(segment) if segment.next == start => segment,
            _ => Segment {
                next: start,
                offset: 0,
            },
        };
        let mut scheduled = start;
        for packet in packets {
            let own = scheduled;
            scheduled += 1;
            let mut frame = own + segment.offset;
            if let Some(read) = s.last_read {
                let earliest = (read + 1 + self.delay / 2).max(own);
                if frame <= read {
                    segment.offset = earliest - own;
                    frame = earliest;
                    s.realigned += 1;
                } else if segment.offset > 0 && frame >= earliest + CATCH_UP_FRAMES {
                    segment.offset = earliest - own;
                    frame = earliest;
                    s.caught_up += 1;
                }
                if frame >= read + self.slots {
                    continue;
                }
            }
            s.playback_first.get_or_insert(frame);
            s.store(self.slots, frame, packet);
        }
        segment.next = scheduled;
        s.playback = Some(segment);
    }

    pub fn read(
        &self,
        start: u64,
        out: &mut [u8],
        lengths: impl IntoIterator<Item = usize>,
        playback_active: bool,
    ) {
        let mut guard = self.lock();
        let s = &mut *guard;
        let mut pos = 0;
        for (capture_frame, length) in (start..).zip(lengths) {
            let length = length.min(out.len() - pos);
            let dst = &mut out[pos..pos + length];
            pos += length;
            let Some(frame) = capture_frame.checked_sub(self.delay) else {
                dst.fill(0);
                continue;
            };
            s.last_read = Some(s.last_read.map_or(frame, |read| read.max(frame)));
            let slot = (frame % self.slots) as usize;
            if s.frames[slot] == frame {
                let src = &s.audio[slot * PACKET_BYTES..(slot + 1) * PACKET_BYTES];
                let n = length.min(PACKET_BYTES) / FRAME_BYTES * FRAME_BYTES;
                apply_gain(s.gain, &src[..n], &mut dst[..n]);
                dst[n..].fill(0);
            } else {
                dst.fill(0);
                if playback_active && s.playback_first.is_some_and(|first| frame >= first) {
                    s.missing += 1;
                }
            }
        }
        out[pos..].fill(0);
    }

    pub fn end_playback(&self) {
        let mut s = self.lock();
        s.playback = None;
        s.playback_first = None;
    }

    pub fn end_capture(&self) {
        self.lock().last_read = None;
    }
}

impl LineState {
    fn store(&mut self, slots: u64, frame: u64, packet: &[u8]) {
        if packet.is_empty() {
            return;
        }
        let slot = (frame % slots) as usize;
        let dst = &mut self.audio[slot * PACKET_BYTES..(slot + 1) * PACKET_BYTES];
        let n = packet.len().min(PACKET_BYTES) / FRAME_BYTES * FRAME_BYTES;
        dst[..n].copy_from_slice(&packet[..n]);
        dst[n..].fill(0);
        self.frames[slot] = frame;
    }
}

fn apply_gain(gain: Gain, src: &[u8], dst: &mut [u8]) {
    let factor = match gain {
        Gain::Unity => {
            dst.copy_from_slice(src);
            return;
        }
        Gain::Scale(factor) => factor,
    };
    let (src_chunks, _) = src.as_chunks::<3>();
    let (dst_chunks, _) = dst.as_chunks_mut::<3>();
    for (from, to) in src_chunks.iter().zip(dst_chunks) {
        let sample = decode_int24(from[0], from[1], from[2]);
        encode_int24(to, (sample as f32 * factor).round() as i32);
    }
}

fn decode_int24(low: u8, middle: u8, high: u8) -> i32 {
    let mut value = low as i32 | (middle as i32) << 8 | (high as i32) << 16;
    if value & 0x80_0000 != 0 {
        value |= !0xff_ffff;
    }
    value
}

fn encode_int24(dst: &mut [u8], value: i32) {
    let value = value.clamp(-0x80_0000, 0x7f_ffff);
    dst[0] = value as u8;
    dst[1] = (value >> 8) as u8;
    dst[2] = (value >> 16) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(frame: u64) -> Vec<u8> {
        vec![(frame % 251) as u8 + 1; PACKET_BYTES]
    }

    fn read(
        line: &DelayLine,
        start: u64,
        count: usize,
        playback_active: bool,
    ) -> Vec<Vec<u8>> {
        let mut out = vec![0xEE; count * PACKET_BYTES];
        line.read(
            start,
            &mut out,
            std::iter::repeat_n(PACKET_BYTES, count),
            playback_active,
        );
        out.chunks(PACKET_BYTES).map(<[u8]>::to_vec).collect()
    }

    fn write(line: &DelayLine, start: u64, count: u64) {
        let packets: Vec<Vec<u8>> = (start..start + count).map(packet).collect();
        line.write(start, packets.iter().map(Vec::as_slice));
    }

    #[test]
    fn passes_audio_through_unchanged_after_delay() {
        let line = DelayLine::new(25);
        for urb in 0..15 {
            write(&line, 1000 + urb * 10, 10);
        }
        let got = read(&line, 1035, 10, true);
        for (i, pkt) in got.iter().enumerate() {
            assert_eq!(pkt, &packet(1010 + i as u64));
        }
        assert_eq!(line.status(), LineStatus::default());
    }

    #[test]
    fn unity_and_scaled_gain() {
        let line = DelayLine::new(1);
        let mut samples = Vec::new();
        for value in [0x40_0000, -0x40_0000, 1, -1, 0x7f_ffff] {
            let mut b = [0; 3];
            encode_int24(&mut b, value);
            samples.extend_from_slice(&b);
        }
        samples.resize(PACKET_BYTES, 0);
        line.write(1000, [samples.as_slice()]);
        assert_eq!(read(&line, 1001, 1, false)[0], samples);
        line.set_gain(Gain::Scale(0.5));
        let half = read(&line, 1001, 1, false).remove(0);
        assert_eq!(decode_int24(half[0], half[1], half[2]), 0x20_0000);
    }
}
