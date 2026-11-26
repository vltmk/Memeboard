# Memeboard

A fast, lightweight, monochromatic microphone soundboard for Windows built with **Tauri v2**, **Rust**, and **React / TypeScript**.

Play sound clips directly through your voice chat microphone while talking naturally — with zero separate VB-Cable downloads, bit-clean microphone passthrough, non-consuming global hotkeys, and a built-in virtual microphone device.

---

## Key Features

- **Built-in Virtual Microphone (Zero Extra App Installs):**
  Memeboard bundles WHQL-signed [`vadimgrn/usbip-win2`](https://github.com/vadimgrn/usbip-win2) with Windows' inbox USB Audio 2.0 class driver (`usbaudio2.sys`) to expose **Memeboard Virtual Mic** automatically inside the installer.
- **Physical Microphone Untouched + Auto Default Mic Switching:**
  Your real microphone hardware and Windows properties remain unmodified. While Memeboard runs, it can automatically set **Memeboard Virtual Mic** as the Windows Default Recording & Communications device, then safely restore your original microphone on exit (with crash recovery on next launch).
- **Studio-Clean Real-Time Audio Engine (48 kHz Stereo):**
  - Lock-free SPSC queues and pre-allocated mixer slots (zero locks/allocations in WASAPI audio callbacks)
  - Bit-transparent voice passthrough (`tanh` soft-peak limiter only shapes soundboard clips, never your live speech)
  - Perceptual quadratic audio taper on **Mic SFX** and **Local** volume sliders (`50%` = `-12 dB`, `100%` = unity) so lowering SFX volume in Discord is smooth and natural
  - Adaptive clock-drift compensation and `rubato` `SincFixedIn` resampling for mismatched device sample rates
  - Smooth auto voice ducking down to `-24 dB` with hysteresis + 250 ms hold
  - **AEC-Safe Headphone Monitor:** Optional `+7 Hz` Hilbert frequency decorrelation on headphones only so Discord/TeamSpeak/WebRTC echo cancellation never cancels your soundboard clips
  - **ITU-R BS.1770** K-weighted gated loudness normalization (`-16 LUFS` default) calculated over your trimmed clip bounds
  - Supports **MP3, WAV (16/24/32-bit PCM & 32/64-bit Float), OGG/Vorbis, FLAC, AAC/M4A, Opus, and WebM**
- **Bulk Packs, Templates & Individual Downloads:**
  - Export full soundboards or single categories as portable **`.memeboard` / `.zip`** packs containing audio files + `memeboard.json` templates (names, emojis, categories, trim bounds, volume, hotkeys, playback modes)
  - Import `.memeboard` packs, plain `.zip` audio archives, or bulk-import audio files with a shared template
  - Right-click any sound pill (or open its editor) to **Download Audio…** directly to disk
- **Category Management & Sound Editor:**
  - Create, rename, and delete category pills, assign/unassign clips from a checklist modal, or move clips via the right-click context menu
  - Drag start/end trim handles (down to `0.05s`) with click-to-seek and **monitor-only** preview (`preview_sound` never leaks into Discord)
  - `Restart`, `Stop`, and `Overlap` playback modes
- **Docked Drawer, Mini Overlay & Non-Consuming Global Hotkeys:**
  - Frameless bottom-right docked drawer UI with snap-to-corner docking, animated **Live / Voice Live** status indicator, and Inter + Ioskeley Mono + IRANYekanWeb (Persian/Arabic) typography
  - Windows Raw Input (`RIDEV_INPUTSINK`) pass-through global hotkeys work inside fullscreen games without blocking `Esc` or `F1–F24`
  - Default Panic Key: `Ctrl + Shift + X` • Mini Overlay: `Ctrl + Shift + S`
  - Runs quietly in the System Tray when closed (`Quit Memeboard` stops streams and restores your default mic)

---

## Documentation

- **[Architecture Guide](docs/ARCHITECTURE.md)** — Deep dive into the audio engine, virtual mic USB/IP stack, Raw Input hotkeys, pack schema, and windowing model.
- **[Testing Guide](docs/TESTING.md)** — How unit tests, DSP verification, frontend checks, and installer builds are structured and run.
- **[Changelog](CHANGELOG.md)** — Release notes and version history.

---

## Quick Start (Windows)

1. Download and run **`Memeboard_0.2.0_x64-setup.exe`** from [Releases](../../releases).
2. The installer automatically sets up the built-in `usbip-win2` driver and `MemeboardVirtualAudio` background service (no separate virtual cable installer needed).
3. Launch **Memeboard** — drag and drop audio files (or a `.memeboard` pack) onto the board, or click **Add**.
4. Discord, TeamSpeak, OBS, or games using Windows Default Microphone will immediately hear your normal voice + triggered sound clips.

> You can also select **Memeboard Virtual Mic** manually as your Input Device in Discord/OBS Settings if you disable **Auto-Switch Default Mic** in **Audio Settings**.

---

## Building & Testing From Source

### Prerequisites
- **Node.js** 20+
- **Rust** stable toolchain (`x86_64-pc-windows-gnu` or `x86_64-pc-windows-msvc`)
- Windows 10 / 11

### Development & Production Installer Build

```bash
# 1. Install frontend dependencies
npm install

# 2. Download & SHA256-verify the bundled usbip-win2 driver installer
npm run prepare:driver

# 3. Run in development mode
npm run tauri dev

# 4. Build the desktop app & NSIS Windows installer
npm run tauri build
```

### Verification Commands

```bash
# Rust unit tests (mixer, decoder, pack import/export, hotkeys, virtual mic)
cargo test --manifest-path src-tauri/Cargo.toml

# Audio DSP & real-time mixer verification binary
cargo run --bin test_audio --manifest-path src-tauri/Cargo.toml

# Frontend TypeScript check + Vite production bundle
npm run build
```

---

## Releasing via GitHub Actions

Pushing a tag matching `v*` (for example `v0.2.0`) triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which runs all Rust unit tests, executes `test_audio`, builds the NSIS installer via `npm run tauri build`, and publishes the `.exe` installer to GitHub Releases.

---

## License

Licensed under the [MIT License](LICENSE). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for bundled driver and library notices.
