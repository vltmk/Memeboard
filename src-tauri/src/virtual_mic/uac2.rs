//! Full-speed USB Audio Class 2.0 virtual audio device descriptors and control handling.
//! Windows automatically binds its inbox `usbaudio2.sys` driver to this device.
//! Adapted from Tom56785/virtual-cables (MIT License).

use std::collections::HashMap;
use std::sync::Mutex;

use super::delay_line::{DelayLine, Gain, PACKET_BYTES};

pub const DESCRIPTOR_DEVICE: u8 = 0x01;
pub const DESCRIPTOR_CONFIGURATION: u8 = 0x02;
pub const DESCRIPTOR_STRING: u8 = 0x03;
pub const DESCRIPTOR_INTERFACE: u8 = 0x04;
pub const DESCRIPTOR_ENDPOINT: u8 = 0x05;
pub const DESCRIPTOR_INTERFACE_ASSOCIATION: u8 = 0x0B;
pub const DESCRIPTOR_CS_INTERFACE: u8 = 0x24;
pub const DESCRIPTOR_CS_ENDPOINT: u8 = 0x25;

pub const PRODUCT_ID_BASE: u32 = 0xCB00;
const IP_VERSION_02_00: u8 = 0x20;

const CLOCK_SOURCE: u8 = 9;
const PLAYBACK_INPUT_TERMINAL: u8 = 1;
const PLAYBACK_FEATURE_UNIT: u8 = 2;
const PLAYBACK_OUTPUT_TERMINAL: u8 = 3;
const RECORDING_INPUT_TERMINAL: u8 = 4;
const RECORDING_FEATURE_UNIT: u8 = 5;
const RECORDING_OUTPUT_TERMINAL: u8 = 6;

pub const SAMPLE_RATE: u32 = 48_000;
pub const DEFAULT_LATENCY_MS: i64 = 25;
pub const MIN_LATENCY_MS: i64 = 1;
pub const MAX_LATENCY_MS: i64 = 1000;

pub const REQUEST_GET_STATUS: u8 = 0x00;
pub const REQUEST_CLEAR_FEATURE: u8 = 0x01;
pub const REQUEST_SET_FEATURE: u8 = 0x03;
pub const REQUEST_SET_ADDRESS: u8 = 0x05;
pub const REQUEST_GET_DESCRIPTOR: u8 = 0x06;
pub const REQUEST_GET_CONFIGURATION: u8 = 0x08;
pub const REQUEST_SET_CONFIGURATION: u8 = 0x09;
pub const REQUEST_GET_INTERFACE: u8 = 0x0A;
pub const REQUEST_SET_INTERFACE: u8 = 0x0B;
pub const REQUEST_SYNCH_FRAME: u8 = 0x0C;

pub const AUDIO_CUR: u8 = 0x01;
pub const AUDIO_RANGE: u8 = 0x02;

const CS_SAM_FREQ_CONTROL: u8 = 0x01;
const CS_CLOCK_VALID_CONTROL: u8 = 0x02;
const FU_MUTE_CONTROL: u8 = 0x01;
const FU_VOLUME_CONTROL: u8 = 0x02;
const STALL: i32 = -32;

const FEATURE_UNITS: [u8; 2] = [PLAYBACK_FEATURE_UNIT, RECORDING_FEATURE_UNIT];
const VOLUME_MIN: i16 = -60 * 256;
const VOLUME_MAX: i16 = 0;
const VOLUME_RES: i16 = 256;
const VOLUME_SILENT: i16 = i16::MIN;

pub struct Descriptors {
    pub cable_number: u32,
    pub product: String,
    pub serial: String,
    pub device: Vec<u8>,
    pub config: Vec<u8>,
    pub strings: HashMap<u8, Vec<u8>>,
}

impl Descriptors {
    pub fn named(cable_number: u32, serial: &str, name: &str) -> Result<Descriptors, String> {
        if !(1..=255).contains(&cable_number) {
            return Err("cable number must be 1..255".into());
        }
        let product = name.to_string();
        let serial = serial.to_string();
        let mut strings = HashMap::new();
        strings.insert(0, vec![4, DESCRIPTOR_STRING, 0x09, 0x04]);
        strings.insert(1, string_descriptor("Memeboard"));
        strings.insert(2, string_descriptor(&product));
        strings.insert(3, string_descriptor(&serial));
        let d = Descriptors {
            cable_number,
            device: device_descriptor(cable_number),
            config: configuration_descriptor(),
            product,
            serial,
            strings,
        };
        d.validate()?;
        Ok(d)
    }

    pub fn get_descriptor(&self, descriptor_type: u8, index: u8) -> Option<Vec<u8>> {
        match descriptor_type {
            DESCRIPTOR_DEVICE if index == 0 => Some(self.device.clone()),
            DESCRIPTOR_CONFIGURATION if index == 0 => Some(self.config.clone()),
            DESCRIPTOR_STRING => self.strings.get(&index).cloned(),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.device.len() != 18 || self.device[0] != 18 || self.device[1] != DESCRIPTOR_DEVICE {
            return Err("invalid device descriptor".into());
        }
        if self.config.len() < 9 || self.config[1] != DESCRIPTOR_CONFIGURATION {
            return Err("invalid configuration descriptor".into());
        }
        let total = u16::from_le_bytes([self.config[2], self.config[3]]) as usize;
        if total != self.config.len() {
            return Err(format!(
                "configuration wTotalLength={total}, actual={}",
                self.config.len()
            ));
        }
        let mut pos = 0;
        while pos < self.config.len() {
            let length = self.config[pos] as usize;
            if length < 2 || pos + length > self.config.len() {
                return Err(format!("invalid descriptor at offset {pos}"));
            }
            pos += length;
        }
        Ok(())
    }
}

fn device_descriptor(cable_number: u32) -> Vec<u8> {
    let pid = (PRODUCT_ID_BASE + cable_number) as u16;
    let mut b = vec![0u8; 18];
    b[0] = 18;
    b[1] = DESCRIPTOR_DEVICE;
    b[2..4].copy_from_slice(&0x0200u16.to_le_bytes());
    b[4] = 0xEF;
    b[5] = 0x02;
    b[6] = 0x01;
    b[7] = 64;
    b[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    b[10..12].copy_from_slice(&pid.to_le_bytes());
    b[12..14].copy_from_slice(&0x0001u16.to_le_bytes());
    b[14] = 1;
    b[15] = 2;
    b[16] = 3;
    b[17] = 1;
    b
}

fn configuration_descriptor() -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    let c = DESCRIPTOR_CS_INTERFACE;
    let le16 = |v: u16| v.to_le_bytes();
    let channels: [u8; 4] = 0x0000_0003u32.to_le_bytes();

    b.extend_from_slice(&[9, DESCRIPTOR_CONFIGURATION, 0, 0, 3, 1, 0, 0x80, 50]);
    b.extend_from_slice(&[
        8,
        DESCRIPTOR_INTERFACE_ASSOCIATION,
        0,
        3,
        0x01,
        0x00,
        IP_VERSION_02_00,
        0,
    ]);
    b.extend_from_slice(&[
        9,
        DESCRIPTOR_INTERFACE,
        0,
        0,
        0,
        0x01,
        0x01,
        IP_VERSION_02_00,
        0,
    ]);
    let header = b.len();
    b.extend_from_slice(&[9, c, 0x01, 0x00, 0x02, 0x08, 0, 0, 0x00]);
    b.extend_from_slice(&[8, c, 0x0A, CLOCK_SOURCE, 0x05, 0x05, 0, 0]);

    // Playback: USB streaming input terminal -> feature unit -> line output terminal
    b.extend_from_slice(&[17, c, 0x02, PLAYBACK_INPUT_TERMINAL]);
    b.extend_from_slice(&le16(0x0101));
    b.extend_from_slice(&[0, CLOCK_SOURCE, 2]);
    b.extend_from_slice(&channels);
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&feature_unit(PLAYBACK_FEATURE_UNIT, PLAYBACK_INPUT_TERMINAL));
    b.extend_from_slice(&[12, c, 0x03, PLAYBACK_OUTPUT_TERMINAL]);
    b.extend_from_slice(&le16(0x0301)); // Speaker output terminal (proven with usbaudio2.sys; playback guard protects user's speakers)
    b.extend_from_slice(&[0, PLAYBACK_FEATURE_UNIT, CLOCK_SOURCE, 0, 0, 0]);

    // Recording: microphone input terminal -> feature unit -> USB streaming output terminal
    b.extend_from_slice(&[17, c, 0x02, RECORDING_INPUT_TERMINAL]);
    b.extend_from_slice(&le16(0x0201));
    b.extend_from_slice(&[0, CLOCK_SOURCE, 2]);
    b.extend_from_slice(&channels);
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&feature_unit(RECORDING_FEATURE_UNIT, RECORDING_INPUT_TERMINAL));
    b.extend_from_slice(&[12, c, 0x03, RECORDING_OUTPUT_TERMINAL]);
    b.extend_from_slice(&le16(0x0101));
    b.extend_from_slice(&[0, RECORDING_FEATURE_UNIT, CLOCK_SOURCE, 0, 0, 0]);

    let ac_length = (b.len() - header) as u16;
    b[header + 6..header + 8].copy_from_slice(&le16(ac_length));

    for (interface, terminal, endpoint) in [
        (1, PLAYBACK_INPUT_TERMINAL, 0x01),
        (2, RECORDING_OUTPUT_TERMINAL, 0x82),
    ] {
        b.extend_from_slice(&[
            9,
            DESCRIPTOR_INTERFACE,
            interface,
            0,
            0,
            0x01,
            0x02,
            IP_VERSION_02_00,
            0,
        ]);
        b.extend_from_slice(&[
            9,
            DESCRIPTOR_INTERFACE,
            interface,
            1,
            1,
            0x01,
            0x02,
            IP_VERSION_02_00,
            0,
        ]);
        b.extend_from_slice(&[16, c, 0x01, terminal, 0x00, 0x01]);
        b.extend_from_slice(&0x0000_0001u32.to_le_bytes());
        b.extend_from_slice(&[2]);
        b.extend_from_slice(&channels);
        b.extend_from_slice(&[0]);
        b.extend_from_slice(&[6, c, 0x02, 0x01, 3, 24]);
        b.extend_from_slice(&[7, DESCRIPTOR_ENDPOINT, endpoint, 0x0D]);
        b.extend_from_slice(&le16(PACKET_BYTES as u16));
        b.extend_from_slice(&[1]);
        b.extend_from_slice(&[8, DESCRIPTOR_CS_ENDPOINT, 0x01, 0x00, 0x00, 0x00, 0, 0]);
    }

    let total = b.len() as u16;
    b[2..4].copy_from_slice(&total.to_le_bytes());
    b
}

fn feature_unit(id: u8, source: u8) -> Vec<u8> {
    let mut b = vec![18, DESCRIPTOR_CS_INTERFACE, 0x06, id, source];
    b.extend_from_slice(&0x0000_000Fu32.to_le_bytes());
    b.extend_from_slice(&[0; 8]);
    b.push(0);
    b
}

fn string_descriptor(s: &str) -> Vec<u8> {
    let mut encoded: Vec<u16> = s.encode_utf16().collect();
    let mut length = 2 + encoded.len() * 2;
    if length > 255 {
        encoded.truncate(126);
        length = 254;
    }
    let mut b = vec![0u8; length];
    b[0] = length as u8;
    b[1] = DESCRIPTOR_STRING;
    for (i, v) in encoded.iter().enumerate() {
        b[2 + i * 2..4 + i * 2].copy_from_slice(&v.to_le_bytes());
    }
    b
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

pub fn parse_setup(b: &[u8]) -> Result<SetupPacket, String> {
    if b.len() < 8 {
        return Err("setup packet too short".into());
    }
    Ok(SetupPacket {
        request_type: b[0],
        request: b[1],
        value: u16::from_le_bytes([b[2], b[3]]),
        index: u16::from_le_bytes([b[4], b[5]]),
        length: u16::from_le_bytes([b[6], b[7]]),
    })
}

pub fn bus_id(number: u32) -> String {
    format!("1-{number}")
}

pub struct Device {
    pub number: u32,
    pub bus_id: String,
    pub descriptors: Descriptors,
    pub audio: DelayLine,
    state: Mutex<DeviceState>,
}

struct DeviceState {
    configuration: u8,
    alt_setting: HashMap<u8, u8>,
    features: [FeatureUnit; 2],
}

#[derive(Clone, Copy, Default)]
struct FeatureUnit {
    mute: bool,
    volume: i16,
}

impl Device {
    pub fn named(number: u32, serial: &str, name: &str, latency_ms: i64) -> Result<Device, String> {
        let descriptors = Descriptors::named(number, serial, name)?;
        let latency_ms = latency_ms.clamp(MIN_LATENCY_MS, MAX_LATENCY_MS);
        Ok(Device {
            number,
            bus_id: bus_id(number),
            descriptors,
            audio: DelayLine::new(latency_ms as u64),
            state: Mutex::new(DeviceState {
                configuration: 0,
                alt_setting: HashMap::from([(0, 0), (1, 0), (2, 0)]),
                features: [FeatureUnit::default(); 2],
            }),
        })
    }

    pub fn product(&self) -> &str {
        &self.descriptors.product
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DeviceState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn handle_control(&self, setup: SetupPacket, out: &[u8]) -> (Vec<u8>, i32) {
        let mut d = self.lock();
        let standard = setup.request_type & 0x60 == 0x00;
        if standard {
            return match setup.request {
                REQUEST_GET_DESCRIPTOR => {
                    let typ = (setup.value >> 8) as u8;
                    let idx = setup.value as u8;
                    match self.descriptors.get_descriptor(typ, idx) {
                        Some(v) => (truncate(v, setup.length), 0),
                        None => (Vec::new(), STALL),
                    }
                }
                REQUEST_SET_ADDRESS => (Vec::new(), 0),
                REQUEST_SET_CONFIGURATION => {
                    let configuration = setup.value as u8;
                    if configuration > 1 {
                        return (Vec::new(), STALL);
                    }
                    d.configuration = configuration;
                    for iface in 0..3 {
                        d.alt_setting.insert(iface, 0);
                    }
                    self.audio.end_playback();
                    self.audio.end_capture();
                    (Vec::new(), 0)
                }
                REQUEST_GET_CONFIGURATION => {
                    (truncate(vec![d.configuration], setup.length), 0)
                }
                REQUEST_SET_INTERFACE => {
                    let iface = setup.index as u8;
                    let alt = setup.value as u8;
                    if d.configuration != 1 || iface > 2 || alt > 1 || (iface == 0 && alt != 0) {
                        return (Vec::new(), STALL);
                    }
                    d.alt_setting.insert(iface, alt);
                    match (iface, alt) {
                        (1, 0) => self.audio.end_playback(),
                        (2, 0) => self.audio.end_capture(),
                        _ => {}
                    }
                    (Vec::new(), 0)
                }
                REQUEST_GET_INTERFACE => {
                    let iface = setup.index as u8;
                    match d.alt_setting.get(&iface) {
                        Some(&alt) => (truncate(vec![alt], setup.length), 0),
                        None => (Vec::new(), STALL),
                    }
                }
                REQUEST_GET_STATUS | REQUEST_SYNCH_FRAME => {
                    (truncate(vec![0, 0], setup.length), 0)
                }
                REQUEST_CLEAR_FEATURE | REQUEST_SET_FEATURE => (Vec::new(), 0),
                _ => (Vec::new(), STALL),
            };
        }

        let recipient = setup.request_type & 0x1f;
        let get = setup.request_type & 0x80 != 0;
        let entity_id = (setup.index >> 8) as u8;
        let interface_number = setup.index as u8;
        let control = (setup.value >> 8) as u8;
        let channel = setup.value as u8;
        if recipient != 0x01 || interface_number != 0 || channel != 0 {
            return (Vec::new(), STALL);
        }
        let reply = |value: Vec<u8>| (truncate(value, setup.length), 0);

        if entity_id == CLOCK_SOURCE {
            return match (control, setup.request, get) {
                (CS_SAM_FREQ_CONTROL, AUDIO_CUR, true) => {
                    reply(SAMPLE_RATE.to_le_bytes().to_vec())
                }
                (CS_SAM_FREQ_CONTROL, AUDIO_RANGE, true) => {
                    let mut range = 1u16.to_le_bytes().to_vec();
                    for value in [SAMPLE_RATE, SAMPLE_RATE, 0] {
                        range.extend_from_slice(&value.to_le_bytes());
                    }
                    reply(range)
                }
                (CS_SAM_FREQ_CONTROL, AUDIO_CUR, false)
                    if out.get(..4) == Some(&SAMPLE_RATE.to_le_bytes()) =>
                {
                    (Vec::new(), 0)
                }
                (CS_CLOCK_VALID_CONTROL, AUDIO_CUR, true) => reply(vec![1]),
                _ => (Vec::new(), STALL),
            };
        }

        let Some(unit) = FEATURE_UNITS.iter().position(|&id| id == entity_id) else {
            return (Vec::new(), STALL);
        };
        let feature = d.features[unit];
        match (control, setup.request, get) {
            (FU_MUTE_CONTROL, AUDIO_CUR, true) => reply(vec![feature.mute as u8]),
            (FU_MUTE_CONTROL, AUDIO_CUR, false) => {
                if let Some(&first) = out.first() {
                    d.features[unit].mute = first != 0;
                    self.audio.set_gain(feature_gain(&d.features));
                }
                (Vec::new(), 0)
            }
            (FU_VOLUME_CONTROL, AUDIO_CUR, true) => reply(feature.volume.to_le_bytes().to_vec()),
            (FU_VOLUME_CONTROL, AUDIO_CUR, false) => {
                if out.len() >= 2 {
                    let volume = i16::from_le_bytes([out[0], out[1]]);
                    d.features[unit].volume = if volume == VOLUME_SILENT {
                        volume
                    } else {
                        volume.clamp(VOLUME_MIN, VOLUME_MAX)
                    };
                    self.audio.set_gain(feature_gain(&d.features));
                }
                (Vec::new(), 0)
            }
            (FU_VOLUME_CONTROL, AUDIO_RANGE, true) => {
                let mut range = 1u16.to_le_bytes().to_vec();
                for value in [VOLUME_MIN, VOLUME_MAX, VOLUME_RES] {
                    range.extend_from_slice(&value.to_le_bytes());
                }
                reply(range)
            }
            _ => (Vec::new(), STALL),
        }
    }

    pub fn playback_active(&self) -> bool {
        let d = self.lock();
        d.configuration == 1 && d.alt_setting.get(&1) == Some(&1)
    }

    pub fn capture_active(&self) -> bool {
        let d = self.lock();
        d.configuration == 1 && d.alt_setting.get(&2) == Some(&1)
    }

    pub fn write_playback<'a>(&self, start: u64, packets: impl IntoIterator<Item = &'a [u8]>) {
        if self.playback_active() {
            self.audio.write(start, packets);
        }
    }

    pub fn read_capture(
        &self,
        start: u64,
        out: &mut [u8],
        lengths: impl IntoIterator<Item = usize>,
    ) {
        let (capturing, playing) = {
            let d = self.lock();
            let active = |iface| d.configuration == 1 && d.alt_setting.get(&iface) == Some(&1);
            (active(2), active(1))
        };
        if !capturing {
            out.fill(0);
            return;
        }
        self.audio.read(start, out, lengths, playing);
    }
}

fn feature_gain(features: &[FeatureUnit; 2]) -> Gain {
    if features
        .iter()
        .any(|f| f.mute || f.volume == VOLUME_SILENT)
    {
        return Gain::Scale(0.0);
    }
    let volume: i32 = features.iter().map(|f| f.volume as i32).sum();
    if volume == 0 {
        Gain::Unity
    } else {
        Gain::Scale(10f32.powf(volume as f32 / 256.0 / 20.0))
    }
}

fn truncate(mut b: Vec<u8>, n: u16) -> Vec<u8> {
    b.truncate(n as usize);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uac2_descriptors_valid_and_named() {
        let dev = Device::named(1, "MEMEBOARD-001", "Memeboard Virtual Mic", 25).unwrap();
        dev.descriptors.validate().unwrap();
        assert_eq!(dev.product(), "Memeboard Virtual Mic");
        assert_eq!(dev.bus_id, "1-1");
        assert_eq!(dev.audio.delay(), 25);
    }
}
