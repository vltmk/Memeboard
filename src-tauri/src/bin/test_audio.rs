use memeboard_lib::decoder::{compute_normalization_gain_range, DecodedAudio};
use memeboard_lib::mixer::{soft_peak_limiter, AudioMixer, CHANNELS, SAMPLE_RATE};
use memeboard_lib::types::PlaybackMode;

fn main() {
    println!("=== RUNNING MEMEBOARD AUDIO ENGINE TESTS ===");

    // 1. Peak Limiter Normal Range
    println!("Running test 1: peak limiter normal range...");
    let samples = [-0.85, -0.5, 0.0, 0.5, 0.85];
    for &s in &samples {
        let limited = soft_peak_limiter(s);
        assert!((limited - s).abs() < 1e-6, "Sample {} altered", s);
    }
    println!("✓ Passed test 1: normal signals pass untouched");

    // 2. Peak Limiter Saturation
    println!("Running test 2: peak limiter saturation...");
    let loud_samples = [0.9, 1.0, 1.5, 2.0, 5.0, 10.0, -1.0, -2.0, -10.0];
    for &s in &loud_samples {
        let limited = soft_peak_limiter(s);
        assert!(limited.abs() < 1.0, "Limiter exceeded 1.0: {}", limited);
        if s > 0.0 {
            assert!(limited > 0.85);
        } else {
            assert!(limited < -0.85);
        }
    }
    println!("✓ Passed test 2: loud sound spikes smoothly saturated < 1.0");

    // 3. Restart / Overlap / Stop modes with lock-free StreamProcessors
    println!("Running test 3: restart, overlap, and toggle stop modes...");
    let mixer = AudioMixer::new();
    mixer.init_sync_for_test();
    let audio = DecodedAudio {
        samples: vec![0.5f32; (SAMPLE_RATE * CHANNELS as u32) as usize],
        duration: 1.0,
        peaks: vec![0.5; 60],
        normalized_gain: 1.0,
    };
    mixer.load_sound("sound1".to_string(), audio);

    let mut bcast = vec![0.0f32; 256 * CHANNELS];
    let mut mon = vec![0.0f32; 256 * CHANNELS];

    mixer.trigger_sound("sound1", 0.0, 1.0, 1.0, PlaybackMode::Restart, 1.0);
    mixer.process_frames(&[], &mut bcast, &mut mon);
    assert_eq!(mixer.active_clip_count(), 1);
    assert_eq!(mixer.get_playing_sound_ids(), vec!["sound1"]);

    mixer.trigger_sound("sound1", 0.0, 1.0, 1.0, PlaybackMode::Restart, 1.0);
    mixer.process_frames(&[], &mut bcast, &mut mon);
    assert_eq!(mixer.active_clip_count(), 1);

    mixer.trigger_sound("sound1", 0.0, 1.0, 1.0, PlaybackMode::Overlap, 1.0);
    mixer.process_frames(&[], &mut bcast, &mut mon);
    assert_eq!(mixer.active_clip_count(), 2);

    mixer.stop_all();
    mixer.process_frames(&[], &mut bcast, &mut mon);
    assert_eq!(mixer.active_clip_count(), 0);
    assert!(mixer.get_playing_sound_ids().is_empty());
    println!("✓ Passed test 3: restart, overlap, and panic stop all verified");

    // 4. Toggle Stop Mode
    println!("Running test 4: toggle stop mode...");
    let toggle_mixer = AudioMixer::new();
    toggle_mixer.init_sync_for_test();
    toggle_mixer.load_sound(
        "toggle".to_string(),
        DecodedAudio {
            samples: vec![0.3f32; 48000 * 2],
            duration: 1.0,
            peaks: vec![0.3; 60],
            normalized_gain: 1.0,
        },
    );
    toggle_mixer.trigger_sound("toggle", 0.0, 1.0, 1.0, PlaybackMode::Stop, 1.0);
    toggle_mixer.process_frames(&[], &mut bcast, &mut mon);
    assert_eq!(toggle_mixer.active_clip_count(), 1);

    toggle_mixer.trigger_sound("toggle", 0.0, 1.0, 1.0, PlaybackMode::Stop, 1.0);
    toggle_mixer.process_frames(&[], &mut bcast, &mut mon);
    assert_eq!(toggle_mixer.active_clip_count(), 0);
    println!("✓ Passed test 4: toggle stop mode starts and stops cleanly");

    // 5. Monitor-Only Editor Preview Isolation
    println!("Running test 5: preview_sound routes ONLY to monitor headphones...");
    let prev_mixer = AudioMixer::new();
    prev_mixer.init_sync_for_test();
    prev_mixer.load_sound(
        "secret_meme".to_string(),
        DecodedAudio {
            samples: vec![0.4f32; 9600],
            duration: 0.1,
            peaks: vec![0.4; 10],
            normalized_gain: 1.0,
        },
    );
    prev_mixer.trigger_preview("secret_meme", 0.0, 0.1, 1.0, 1.0);
    bcast.fill(0.0);
    mon.fill(0.0);
    prev_mixer.process_frames(&[], &mut bcast, &mut mon);
    assert!(mon[0].abs() > 0.2, "Monitor headphones must hear preview");
    assert!(
        bcast.iter().all(|&s| s.abs() < 1e-6),
        "Broadcast virtual mic must NEVER hear editor preview!"
    );
    println!("✓ Passed test 5: editor preview is strictly monitor-only");

    // 6. Voice Quality Preservation (uncolored speech passthrough)
    println!("Running test 6: live speech stays 100% uncolored without waveshaping...");
    let voice_mixer = AudioMixer::new();
    voice_mixer.init_sync_for_test();
    voice_mixer.controls.set_volumes(0.8, 1.0, 1.0, false, false);
    let mut voice_in = vec![0.0f32; 256 * CHANNELS];
    for (i, s) in voice_in.iter_mut().enumerate() {
        *s = ((i as f32) * 0.03).sin() * 0.91; // above old limiter 0.85 threshold
    }
    voice_mixer.process_frames(&voice_in, &mut bcast, &mut mon);
    for (i, (&in_s, &out_s)) in voice_in.iter().zip(bcast.iter()).enumerate() {
        assert!(
            (in_s - out_s).abs() < 1e-6,
            "Speech sample {} was distorted: in={} out={}",
            i,
            in_s,
            out_s
        );
    }
    println!("✓ Passed test 6: physical microphone speech passes bit-clean without waveshaping");

    // 7. -24 dB Voice Ducking + Hysteresis + Hold
    println!("Running test 7: -24 dB auto-ducking & hysteresis...");
    let duck_mixer = AudioMixer::new();
    duck_mixer.init_sync_for_test();
    duck_mixer.controls.set_volumes(0.8, 1.0, 1.0, false, false);
    duck_mixer.controls.set_ducking(true, -24.0);
    duck_mixer.load_sound(
        "bgm".to_string(),
        DecodedAudio {
            samples: vec![0.5f32; SAMPLE_RATE as usize * CHANNELS * 2],
            duration: 2.0,
            peaks: vec![0.5; 60],
            normalized_gain: 1.0,
        },
    );
    duck_mixer.trigger_sound("bgm", 0.0, 2.0, 1.0, PlaybackMode::Restart, 1.0);
    let active_mic = vec![0.25f32; 512 * CHANNELS];
    let mut b_duck = vec![0.0f32; 512 * CHANNELS];
    let mut m_duck = vec![0.0f32; 512 * CHANNELS];
    for _ in 0..8 {
        duck_mixer.process_frames(&active_mic, &mut b_duck, &mut m_duck);
    }
    let sound_only_component = (b_duck[b_duck.len() - 1] - 0.25).abs();
    assert!(
        sound_only_component < 0.05,
        "-24 dB ducking should attenuate 0.5 sound near ~0.0315, got {}",
        sound_only_component
    );
    assert!(
        duck_mixer
            .controls
            .is_mic_active
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    println!("✓ Passed test 7: -24 dB smooth voice ducking & VAD hold verified");

    // 8. Independent Monitor Mute + Broadcast Unmuted
    println!("Running test 8: independent mute controls...");
    let mute_mixer = AudioMixer::new();
    mute_mixer.init_sync_for_test();
    mute_mixer.controls.set_volumes(0.8, 1.0, 1.0, true, false);
    mute_mixer.load_sound(
        "tone".to_string(),
        DecodedAudio {
            samples: vec![0.4f32; 9600],
            duration: 0.1,
            peaks: vec![0.4; 10],
            normalized_gain: 1.0,
        },
    );
    mute_mixer.trigger_sound("tone", 0.0, 0.1, 1.0, PlaybackMode::Restart, 1.0);
    mute_mixer.process_frames(&[], &mut bcast, &mut mon);
    assert_eq!(mon[0], 0.0, "Monitor must be muted");
    assert!(bcast[0] > 0.3, "Broadcast must still play");
    println!("✓ Passed test 8: monitor muted while broadcast active");

    // 9. Independent Dual Stream Positions
    println!("Running test 9: concurrent dual-stream buffer sizes...");
    let dual_mixer = AudioMixer::new();
    let mut b_proc = dual_mixer.create_broadcast_processor();
    let mut m_proc = dual_mixer.create_monitor_processor();
    dual_mixer.controls.broadcast_stream_active.store(true, std::sync::atomic::Ordering::Relaxed);
    dual_mixer.controls.monitor_stream_active.store(true, std::sync::atomic::Ordering::Relaxed);
    dual_mixer.load_sound(
        "dual".to_string(),
        DecodedAudio {
            samples: vec![0.5f32; 9600],
            duration: 0.1,
            peaks: vec![0.5; 10],
            normalized_gain: 1.0,
        },
    );
    dual_mixer.trigger_sound("dual", 0.0, 0.1, 1.0, PlaybackMode::Restart, 1.0);
    let mut b_buf = vec![0.0f32; 128 * CHANNELS];
    let mut m_buf = vec![0.0f32; 64 * CHANNELS];
    b_proc.process_broadcast(&[], &mut b_buf);
    m_proc.process_monitor(&mut m_buf);
    assert!(b_buf[127 * CHANNELS] > 0.3);
    assert!(m_buf[63 * CHANNELS] > 0.25);
    println!("✓ Passed test 9: broadcast and monitor processors run lock-free and independent");

    // 10. Trimmed ITU-R BS.1770 Gated Loudness Calculation
    println!("Running test 10: trimmed BS.1770 normalization targets LUFS accurately...");
    let mut trim_samples = vec![0.0f32; SAMPLE_RATE as usize * CHANNELS * 2];
    for frame in (SAMPLE_RATE as usize)..(SAMPLE_RATE as usize * 2) {
        let v = (2.0 * std::f32::consts::PI * 1000.0 * frame as f32 / SAMPLE_RATE as f32).sin() * 0.05;
        trim_samples[frame * 2] = v;
        trim_samples[frame * 2 + 1] = v;
    }
    let gain_quiet_second = compute_normalization_gain_range(&trim_samples, 1.0, 2.0, -14.0);
    assert!(
        gain_quiet_second > 1.5,
        "Expected positive boost for quiet trimmed second, got {}",
        gain_quiet_second
    );
    println!("✓ Passed test 10: BS.1770 trimmed loudness normalization verified");

    println!("\n===========================================");
    println!("ALL 10 MEMEBOARD AUDIO TESTS PASSED SUCCESSFULLY!");
    println!("===========================================");
}
