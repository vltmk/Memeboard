# Memeboard Testing & Verification Guide

This document explains how Memeboard is tested today and how new tests should be structured as features are added.

---

## 1. Test Suites & Commands

Run all three verification suites before tagging a release (matching [`.github/workflows/release.yml`](../.github/workflows/release.yml)):

### 1.1 Rust Unit & Integration Tests
```bash
cargo test --manifest-path src-tauri/Cargo.toml
```
Covers unit and module-level integration tests across:
- `decoder::tests`: Resampling edge cases (`0` frames, `44.1 kHz -> 48 kHz` sine accuracy), 24-bit PCM WAV decoding, and BS.1770 gated LUFS ignoring leading silence.
- `mixer::tests`: `soft_peak_limiter` linearity and saturation, `perceptual_volume_taper` curve (`1.0` unity, `0.5 -> 0.25`), uncolored voice passthrough, monitor-only preview isolation, `-24 dB` voice ducking + `250 ms` VAD hold, and `+7 Hz` Hilbert AEC decorrelation.
- `device_manager::tests`: `MicConsumerState` WASAPI 480-frame jitter survival and underrun fade/reprime, `StreamingResampler` `44.1 kHz -> 48 kHz`, and virtual endpoint detection (avoiding false positives like `Sonic Studio Virtual Mixer`).
- `hotkeys::tests`: `parse_hotkey` (modifiers, F-keys, numpad, punctuation, trailing `+`), `KeyboardState` repeat suppression, Win+L modifier recovery, and 40-byte Win64 `RAWINPUT` packet decoding.
- `state_manager::tests`: `sounds_dir` containment enforcement on `delete_sound_file`, legacy schema v0->v1 migration preserving user clips, and `.memeboard` pack export/import + single-file export round-trips.
- `virtual_mic`: `DelayLine` pass-through/gain, `uac2` USB descriptor validation, `policy` JSON restore round-trip and virtual name detection, and `windows_svc` USB/IP port parsing.

> **Windows GNU note:** `src-tauri/build.rs` compiles a Common Controls v6 static manifest archive (`libtest_manifest.a`) linked under `#[cfg(test)]` in `src/lib.rs` so test binaries launch cleanly on Windows without `STATUS_ENTRYPOINT_NOT_FOUND`.

### 1.2 Real-Time Audio DSP Suite (`test_audio`)
```bash
cargo run --bin test_audio --manifest-path src-tauri/Cargo.toml
```
Runs 11 deterministic end-to-end audio engine checks using `AudioMixer::init_sync_for_test()`:
1. Peak limiter normal-range pass-through (`|s| <= 0.85`)
2. Peak limiter smooth saturation `< 1.0` for extreme spikes
3. `PlaybackMode::Restart`, `PlaybackMode::Overlap`, and `stop_all()` panic clear
4. `PlaybackMode::Stop` toggle behavior
5. Strict monitor-only `trigger_preview` isolation (zero broadcast bleed)
6. Bit-clean speech passthrough above `0.85` without limiter coloration
7. `-24 dB` auto-ducking & `is_mic_active` VAD state
8. Independent monitor mute while broadcast stays active
9. Concurrent independent broadcast (`128`-frame) and monitor (`64`-frame) stream buffers
10. Trimmed BS.1770 LUFS normalization targeting
11. AEC-safe `+7 Hz` headphone monitor decorrelation (`norm_corr < 0.20`) while broadcast stays pure

### 1.3 Frontend Type-Check & Production Bundle
```bash
npm run build
```
Runs `tsc` (strict TypeScript compilation) followed by `vite build` to verify all TSX components, Tailwind v4 CSS rules, and bundled font assets (`Inter Variable`, `Ioskeley Mono`, `IRANYekanWeb`).

### 1.4 Full Platform Installer / Package Build
```bash
npm run tauri build
```
- **Windows**: Verifies `scripts/prepare-driver.mjs` (SHA-256 verification of `USBip-0.9.8.1-x64.exe`), builds the release binary with LTO, and packages `src-tauri/target/release/bundle/nsis/Memeboard_<version>_x64-setup.exe`.
- **Linux (`ubuntu-22.04` CI)**: Skips Windows driver download, builds the release binary with LTO, and packages `.AppImage`, `.deb`, and `.rpm` bundles (`tauri.linux.conf.json`).

---

## 2. Guidelines for Writing New Tests

1. **Test audio synchronously via `AudioMixer::init_sync_for_test()`**:
   Never open real OS audio devices inside `cargo test`; CI runners (`windows-latest`) may have no physical mic/headphones attached. Use `mixer.process_frames(&mic, &mut bcast, &mut mon)` with synthetic buffers (`48_000` Hz stereo interleaved).
2. **Keep file-system tests isolated under `std::env::temp_dir()`**:
   Generate unique directory names using `Uuid::new_v4()` and always remove the temp directory at the end of the test. Never touch `%LOCALAPPDATA%\Memeboard` in tests.
3. **Assert containment & boundary invariants**:
   Any command or helper accepting paths (`delete_sound_file`, `export_sound_file`, `import_pack`) must have tests verifying that traversal attempts (`..`, absolute paths, or paths outside `sounds_dir`) are rejected.
4. **Verify DSP math with explicit tolerances**:
   - Check that speech samples remain bit-clean (`(in_s - out_s).abs() < 1e-6`) when no clipping occurs.
   - Check that monitor-only previews produce zero signal (`< 1e-6`) on `broadcast_out`.
   - When testing attack/release envelopes or Hilbert shifters, advance enough frames (e.g., `4096` frames) past filter warmup before asserting steady-state correlation or attenuation.
5. **Frontend & GUI manual checklist** (for visual/OS-specific features that cannot run headless):
   - Docked main drawer snaps flush to bottom-right work area (`DOCKED` badge) and updates to `FLOAT` when dragged away.
   - Overlay toggles via `Ctrl+Shift+S` with no white border and without stealing focus from fullscreen games.
   - Right-click context menu on sound pills (`Download Audio…`, `Category`, `Edit Sound…`, `Delete`) and category pills (`Edit Category & Clips…`, `Export Category Pack…`).
   - Mixed English, monospace hotkey badges (`Ioskeley Mono`), and Persian/Arabic (`IRANYekanWeb`, `dir="auto"`) rendering.
