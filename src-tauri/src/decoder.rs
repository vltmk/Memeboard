use mousiki::{Channels as OpusChannels, Decoder as OpusDecoder};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{CODEC_TYPE_NULL, CODEC_TYPE_OPUS, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const TARGET_SAMPLE_RATE: u32 = 48_000;
pub const TARGET_CHANNELS: usize = 2; // Stereo interleaved: [L, R, L, R, ...]

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Interleaved stereo samples at TARGET_SAMPLE_RATE (48kHz)
    pub samples: Vec<f32>,
    pub duration: f32,
    pub peaks: Vec<f32>,
    pub normalized_gain: f32,
}

pub fn decode_audio_file<P: AsRef<Path>>(path: P) -> Result<DecodedAudio, String> {
    decode_audio_file_with_lufs(path, -14.0)
}

pub fn decode_audio_file_with_lufs<P: AsRef<Path>>(
    path: P,
    target_lufs: f32,
) -> Result<DecodedAudio, String> {
    let file = File::open(&path).map_err(|e| format!("Failed to open file: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.as_ref().extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }

    let meta_opts: MetadataOptions = Default::default();
    let fmt_opts: FormatOptions = Default::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .map_err(|e| format!("Audio format probe failed: {}", e))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "No supported audio track found in file".to_string())?;

    let track_id = track.id;
    let is_opus = track.codec_params.codec == CODEC_TYPE_OPUS;
    let track_sample_rate = if is_opus {
        48_000
    } else {
        track.codec_params.sample_rate.unwrap_or(44_100)
    };
    let track_channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .or_else(|| parse_opus_head_channels(track.codec_params.extra_data.as_deref()))
        .unwrap_or(2)
        .max(1);

    let mut raw_samples: Vec<f32> = Vec::new();

    if is_opus {
        let opus_ch = if track_channels == 1 {
            OpusChannels::Mono
        } else {
            OpusChannels::Stereo
        };
        let mut opus_dec = OpusDecoder::new(48_000, opus_ch)
            .map_err(|e| format!("Failed to initialize Opus decoder: {e:?}"))?;
        let mut pre_skip = parse_opus_preskip(
            track.codec_params.extra_data.as_deref(),
            track.codec_params.delay,
        );
        // Max 120ms Opus frame at 48kHz = 5760 frames per channel
        let mut pcm_scratch = vec![0.0f32; 5760 * opus_ch.count()];
        let out_ch = opus_ch.count();

        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(SymphoniaError::ResetRequired) => continue,
                Err(_) => break,
            };

            if packet.track_id() != track_id {
                continue;
            }
            let data: &[u8] = &packet.data;
            if data.starts_with(b"OpusHead") {
                if data.len() >= 12 && pre_skip == 0 {
                    pre_skip = u16::from_le_bytes([data[10], data[11]]) as usize;
                }
                continue;
            }
            if data.starts_with(b"OpusTags") || data.is_empty() {
                continue;
            }

            if let Ok(frames) = opus_dec.decode_float(data, &mut pcm_scratch, false) {
                let total_floats = frames * out_ch;
                if pre_skip > 0 {
                    let skip_frames = pre_skip.min(frames);
                    pre_skip -= skip_frames;
                    let start_idx = skip_frames * out_ch;
                    raw_samples.extend_from_slice(&pcm_scratch[start_idx..total_floats]);
                } else {
                    raw_samples.extend_from_slice(&pcm_scratch[..total_floats]);
                }
            }
        }

        // Convert using opus decoder output channels (1 or 2)
        if raw_samples.is_empty() {
            return Err("No Opus audio samples could be decoded".to_string());
        }
        let stereo = convert_to_stereo(&raw_samples, out_ch);
        let duration = (stereo.len() / TARGET_CHANNELS) as f32 / TARGET_SAMPLE_RATE as f32;
        let peaks = compute_peaks(&stereo, 80);
        let normalized_gain = compute_normalization_gain_range(&stereo, 0.0, duration, target_lufs);
        return Ok(DecodedAudio {
            samples: stereo,
            duration,
            peaks,
            normalized_gain,
        });
    }

    let dec_opts: DecoderOptions = Default::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .map_err(|e| format!("Failed to create audio decoder: {}", e))?;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                extract_samples_to_f32(&decoded, track_channels, &mut raw_samples);
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }

    if raw_samples.is_empty() {
        return Err("No audio samples could be decoded".to_string());
    }

    let stereo_samples = convert_to_stereo(&raw_samples, track_channels);

    let resampled = if track_sample_rate != TARGET_SAMPLE_RATE {
        resample_high_quality(&stereo_samples, track_sample_rate, TARGET_SAMPLE_RATE)
    } else {
        stereo_samples
    };

    if resampled.is_empty() {
        return Err("Decoded audio contained zero output frames".to_string());
    }

    let total_frames = resampled.len() / TARGET_CHANNELS;
    let duration = total_frames as f32 / TARGET_SAMPLE_RATE as f32;
    let peaks = compute_peaks(&resampled, 80);
    let normalized_gain = compute_normalization_gain_range(&resampled, 0.0, duration, target_lufs);

    Ok(DecodedAudio {
        samples: resampled,
        duration,
        peaks,
        normalized_gain,
    })
}

fn parse_opus_head_channels(extra: Option<&[u8]>) -> Option<usize> {
    let data = extra?;
    if data.starts_with(b"OpusHead") && data.len() >= 10 {
        Some((data[9] as usize).max(1))
    } else {
        None
    }
}

fn parse_opus_preskip(extra: Option<&[u8]>, codec_delay: Option<u32>) -> usize {
    if let Some(d) = codec_delay {
        return d as usize;
    }
    if let Some(data) = extra {
        if data.starts_with(b"OpusHead") && data.len() >= 12 {
            return u16::from_le_bytes([data[10], data[11]]) as usize;
        }
    }
    0
}

pub fn extract_samples_to_f32(buffer: &AudioBufferRef, channels: usize, out: &mut Vec<f32>) {
    let frames = buffer.frames();
    if frames == 0 || channels == 0 {
        return;
    }
    match buffer {
        AudioBufferRef::F32(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail { planes.planes()[ch][frame_idx] } else { 0.0 });
                }
            }
        }
        AudioBufferRef::F64(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        planes.planes()[ch][frame_idx] as f32
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::S32(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        planes.planes()[ch][frame_idx] as f32 / 2_147_483_648.0
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::U32(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        (planes.planes()[ch][frame_idx] as f64 - 2_147_483_648.0) as f32
                            / 2_147_483_648.0
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::S24(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        planes.planes()[ch][frame_idx].0 as f32 / 8_388_608.0
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::U24(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        (planes.planes()[ch][frame_idx].0 as f32 - 8_388_608.0) / 8_388_608.0
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::S16(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        planes.planes()[ch][frame_idx] as f32 / 32_768.0
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::U16(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        (planes.planes()[ch][frame_idx] as f32 - 32_768.0) / 32_768.0
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::S8(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        planes.planes()[ch][frame_idx] as f32 / 128.0
                    } else {
                        0.0
                    });
                }
            }
        }
        AudioBufferRef::U8(buf) => {
            let planes = buf.planes();
            let avail = planes.planes().len();
            for frame_idx in 0..frames {
                for ch in 0..channels {
                    out.push(if ch < avail {
                        (planes.planes()[ch][frame_idx] as f32 - 128.0) / 128.0
                    } else {
                        0.0
                    });
                }
            }
        }
    }
}

pub fn convert_to_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 || samples.is_empty() {
        return Vec::new();
    }
    if channels == 2 {
        return samples.to_vec();
    }
    if channels == 1 {
        let mut out = Vec::with_capacity(samples.len() * 2);
        for &s in samples {
            out.push(s);
            out.push(s);
        }
        return out;
    }

    let frames = samples.len() / channels;
    let mut out = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        let base = f * channels;
        let fl = samples[base];
        let fr = samples[base + 1];
        // Downmix center (ch 2) and surround (ch 4, 5) cleanly if present (standard 5.1/7.1 layout)
        let center = if channels >= 3 {
            samples[base + 2] * 0.7071
        } else {
            0.0
        };
        let (sl, sr) = if channels >= 6 {
            (samples[base + 4] * 0.5, samples[base + 5] * 0.5)
        } else {
            (0.0, 0.0)
        };
        let l = (fl + center + sl).clamp(-1.0, 1.0);
        let r = (fr + center + sr).clamp(-1.0, 1.0);
        out.push(l);
        out.push(r);
    }
    out
}

pub fn resample_high_quality(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return input.to_vec();
    }

    let in_frames = input.len() / TARGET_CHANNELS;
    if in_frames == 0 {
        return Vec::new();
    }

    let ratio = to_rate as f64 / from_rate as f64;
    let expected_out_frames = ((in_frames as f64) * ratio).round().max(1.0) as usize;

    if in_frames < 64 {
        return resample_linear_safe(input, from_rate, to_rate, TARGET_CHANNELS);
    }

    let chunk_size = 1024usize.min(in_frames);
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Cubic,
        oversampling_factor: 160,
        window: WindowFunction::BlackmanHarris2,
    };

    let Ok(mut resampler) = SincFixedIn::<f32>::new(ratio, 1.1, params, chunk_size, TARGET_CHANNELS)
    else {
        return resample_linear_safe(input, from_rate, to_rate, TARGET_CHANNELS);
    };

    let mut left_in = Vec::with_capacity(in_frames);
    let mut right_in = Vec::with_capacity(in_frames);
    for f in 0..in_frames {
        left_in.push(input[f * 2]);
        right_in.push(input[f * 2 + 1]);
    }

    let delay = resampler.output_delay();
    let max_out_per_chunk = resampler.output_frames_max();
    let mut out_buf = vec![vec![0.0f32; max_out_per_chunk]; TARGET_CHANNELS];
    let mut out_l = Vec::with_capacity(expected_out_frames + delay + 64);
    let mut out_r = Vec::with_capacity(expected_out_frames + delay + 64);

    let mut pos = 0;
    while pos + chunk_size <= in_frames {
        let slices = [&left_in[pos..pos + chunk_size], &right_in[pos..pos + chunk_size]];
        if let Ok((_, written)) = resampler.process_into_buffer(&slices, &mut out_buf, None) {
            out_l.extend_from_slice(&out_buf[0][..written]);
            out_r.extend_from_slice(&out_buf[1][..written]);
        }
        pos += chunk_size;
    }

    if pos < in_frames {
        let slices = [&left_in[pos..], &right_in[pos..]];
        if let Ok((_, written)) = resampler.process_partial_into_buffer(Some(&slices), &mut out_buf, None) {
            out_l.extend_from_slice(&out_buf[0][..written]);
            out_r.extend_from_slice(&out_buf[1][..written]);
        }
    }

    // Flush resampler delay tail
    if let Ok((_, written)) =
        resampler.process_partial_into_buffer(None::<&[&[f32]]>, &mut out_buf, None)
    {
        out_l.extend_from_slice(&out_buf[0][..written]);
        out_r.extend_from_slice(&out_buf[1][..written]);
    }

    let start = delay.min(out_l.len());
    let avail = out_l.len() - start;
    let take = expected_out_frames.min(avail);

    let mut interleaved = Vec::with_capacity(take * TARGET_CHANNELS);
    for i in 0..take {
        interleaved.push(out_l[start + i]);
        interleaved.push(out_r[start + i]);
    }
    interleaved
}

pub fn resample_linear_safe(
    input: &[f32],
    from_rate: u32,
    to_rate: u32,
    channels: usize,
) -> Vec<f32> {
    if channels == 0 || input.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return input.to_vec();
    }

    let in_frames = input.len() / channels;
    if in_frames == 0 {
        return Vec::new();
    }

    let ratio = to_rate as f64 / from_rate as f64;
    let out_frames = (in_frames as f64 * ratio).round() as usize;
    if out_frames == 0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(out_frames * channels);
    let last_frame = in_frames - 1;

    for out_frame in 0..out_frames {
        let src_pos = out_frame as f64 / ratio;
        let idx0 = (src_pos.floor() as usize).min(last_frame);
        let frac = (src_pos - idx0 as f64) as f32;
        let idx1 = (idx0 + 1).min(last_frame);

        for ch in 0..channels {
            let s0 = input[idx0 * channels + ch];
            let s1 = input[idx1 * channels + ch];
            out.push(s0 + frac * (s1 - s0));
        }
    }

    out
}

pub fn compute_peaks(samples: &[f32], num_points: usize) -> Vec<f32> {
    if samples.is_empty() || num_points == 0 {
        return vec![0.1; num_points];
    }

    let frames = samples.len() / TARGET_CHANNELS;
    if frames == 0 {
        return vec![0.1; num_points];
    }
    let mut peaks = Vec::with_capacity(num_points);

    for p in 0..num_points {
        let start_frame = (p * frames) / num_points;
        let end_frame = (((p + 1) * frames) / num_points).max(start_frame + 1).min(frames);
        let mut max_val = 0.0f32;

        for f in start_frame..end_frame {
            let s_left = samples[f * 2].abs();
            let s_right = samples[f * 2 + 1].abs();
            let s_max = s_left.max(s_right);
            if s_max > max_val {
                max_val = s_max;
            }
        }
        peaks.push(max_val.clamp(0.04, 1.0));
    }

    let global_max = peaks.iter().cloned().fold(0.0f32, f32::max);
    if global_max > 0.05 {
        let scale = 0.95 / global_max;
        for val in &mut peaks {
            *val = (*val * scale).clamp(0.05, 1.0);
        }
    }

    peaks
}

/// ITU-R BS.1770 K-weighted biquad filter state at 48 kHz
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// ITU-R BS.1770-4 Stage 1 (high shelf) & Stage 2 (high-pass) coefficients at 48,000 Hz
fn bs1770_filters_48k() -> (Biquad, Biquad) {
    let stage1 = Biquad {
        b0: 1.53512485958697,
        b1: -2.69169618940638,
        b2: 1.19839281085285,
        a1: -1.69065929318241,
        a2: 0.73248077421585,
        z1: 0.0,
        z2: 0.0,
    };
    let stage2 = Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: -1.99004745483398,
        a2: 0.99007225036621,
        z1: 0.0,
        z2: 0.0,
    };
    (stage1, stage2)
}

/// Computes K-weighted gated loudness normalization multiplier over the trimmed `[trim_start, trim_end]` slice.
pub fn compute_normalization_gain_range(
    samples: &[f32],
    trim_start: f32,
    trim_end: f32,
    target_lufs: f32,
) -> f32 {
    if samples.len() < TARGET_CHANNELS {
        return 1.0;
    }

    let total_frames = samples.len() / TARGET_CHANNELS;
    let start_frame = ((trim_start.max(0.0) * TARGET_SAMPLE_RATE as f32) as usize)
        .min(total_frames.saturating_sub(1));
    let raw_end = (trim_end * TARGET_SAMPLE_RATE as f32) as usize;
    let end_frame = if raw_end > start_frame {
        raw_end.min(total_frames)
    } else {
        total_frames
    };

    let frames = end_frame.saturating_sub(start_frame);
    if frames == 0 {
        return 1.0;
    }

    let (mut l_s1, mut l_s2) = bs1770_filters_48k();
    let (mut r_s1, mut r_s2) = bs1770_filters_48k();

    // 100ms blocks for robust gating on short meme clips
    let block_frames = (TARGET_SAMPLE_RATE as usize / 10).min(frames).max(1);
    let mut block_powers = Vec::new();
    let mut block_sum = 0.0f64;
    let mut count_in_block = 0usize;

    for f in start_frame..end_frame {
        let l = l_s2.process(l_s1.process(samples[f * 2] as f64));
        let r = r_s2.process(r_s1.process(samples[f * 2 + 1] as f64));
        block_sum += l * l + r * r;
        count_in_block += 1;

        if count_in_block == block_frames {
            block_powers.push(block_sum / count_in_block as f64);
            block_sum = 0.0;
            count_in_block = 0;
        }
    }
    if count_in_block > 0 {
        block_powers.push(block_sum / count_in_block as f64);
    }

    // BS.1770 absolute gate at -70 LUFS (offset -0.691 dB)
    let abs_gate_pow = 10.0f64.powf((-70.0 + 0.691) / 10.0);
    let gated_abs: Vec<f64> = block_powers
        .iter()
        .copied()
        .filter(|&p| p > abs_gate_pow)
        .collect();

    if gated_abs.is_empty() {
        return 1.0;
    }

    let mean_abs = gated_abs.iter().sum::<f64>() / gated_abs.len() as f64;
    let rel_gate_lufs = -0.691 + 10.0 * mean_abs.log10() - 10.0;
    let rel_gate_pow = 10.0f64.powf((rel_gate_lufs + 0.691) / 10.0);

    let gated_rel: Vec<f64> = gated_abs
        .into_iter()
        .filter(|&p| p > rel_gate_pow)
        .collect();
    let final_pow = if gated_rel.is_empty() {
        mean_abs
    } else {
        gated_rel.iter().sum::<f64>() / gated_rel.len() as f64
    };

    if final_pow < 1e-9 {
        return 1.0;
    }

    let measured_lufs = (-0.691 + 10.0 * final_pow.log10()) as f32;
    let clamped_target = target_lufs.clamp(-30.0, -6.0);
    let diff_db = clamped_target - measured_lufs;
    let gain = 10.0f32.powf(diff_db / 20.0);

    gain.clamp(0.15, 3.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resample_empty_does_not_underflow() {
        assert!(resample_linear_safe(&[], 44_100, 48_000, 2).is_empty());
        assert!(resample_high_quality(&[], 44_100, 48_000).is_empty());
    }

    #[test]
    fn resamples_44100_sine_to_48000_cleanly() {
        let dur = 0.5;
        let in_rate = 44_100u32;
        let frames = (in_rate as f32 * dur) as usize;
        let mut input = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / in_rate as f32).sin() * 0.5;
            input.push(s);
            input.push(s);
        }
        let out = resample_high_quality(&input, in_rate, 48_000);
        let out_frames = out.len() / 2;
        assert!((out_frames as i32 - 24_000).abs() <= 4, "got {out_frames}");
    }

    #[test]
    fn decodes_24bit_pcm_wav_file() {
        let dir = std::env::temp_dir().join(format!("memeboard-dec-24-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test24.wav");
        {
            let mut f = File::create(&path).unwrap();
            let frames = 4800u32;
            let data_len = frames * 3; // mono 24-bit
            f.write_all(b"RIFF").unwrap();
            f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
            f.write_all(b"WAVEfmt ").unwrap();
            f.write_all(&16u32.to_le_bytes()).unwrap();
            f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
            f.write_all(&1u16.to_le_bytes()).unwrap(); // Mono
            f.write_all(&48000u32.to_le_bytes()).unwrap();
            f.write_all(&(48000u32 * 3).to_le_bytes()).unwrap();
            f.write_all(&3u16.to_le_bytes()).unwrap();
            f.write_all(&24u16.to_le_bytes()).unwrap();
            f.write_all(b"data").unwrap();
            f.write_all(&data_len.to_le_bytes()).unwrap();
            for i in 0..frames {
                let val = ((2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin()
                    * 4_000_000.0) as i32;
                let bytes = val.to_le_bytes();
                f.write_all(&bytes[0..3]).unwrap();
            }
        }
        let decoded = decode_audio_file(&path).unwrap();
        assert_eq!(decoded.samples.len(), 4800 * 2);
        let max_amp = decoded.samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max_amp > 0.4 && max_amp < 0.6, "max_amp={max_amp}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gated_lufs_ignores_leading_silence() {
        let mut samples = vec![0.0f32; 48_000 * 2 * 4]; // 4s silence
        // Append 1s of -20 dB sine
        for f in 0..48_000 {
            let s = (2.0 * std::f32::consts::PI * 1000.0 * f as f32 / 48_000.0).sin() * 0.1;
            samples.push(s);
            samples.push(s);
        }
        let gain_full = compute_normalization_gain_range(&samples, 0.0, 5.0, -14.0);
        let gain_trimmed = compute_normalization_gain_range(&samples, 4.0, 5.0, -14.0);
        assert!(
            (gain_full - gain_trimmed).abs() < 0.3,
            "Gated LUFS should match active segment: full={gain_full}, trimmed={gain_trimmed}"
        );
    }
}
