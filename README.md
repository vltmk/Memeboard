# Memeboard

A fast, lightweight Discord-style microphone soundboard for Windows built with **Tauri v2**, **Rust**, and **React / TypeScript**.

Play sound clips directly through your voice chat microphone while talking naturally — with zero separate VB-Cable downloads, bit-clean microphone passthrough, non-consuming global hotkeys, and a built-in virtual microphone device.

---

## Key Features

- **Built-in Virtual Microphone (Zero Extra App Installs):**
  Memeboard bundles WHQL-signed [`vadimgrn/usbip-win2`](https://github.com/vadimgrn/usbip-win2) with Windows' inbox USB Audio 2.0 class driver (`usbaudio2.sys`) to create **Memeboard Virtual Mic** automatically inside the installer.
- **Physical Microphone Untouched + Auto Default Mic Switching:**
  Your real microphone hardware and Windows properties stay 100% unmodified. While Memeboard runs, it can automatically set **Memeboard Virtual Mic** as the Windows Default Recording & Communications device, then safely restore your original microphone on exit (with crash recovery on next launch).
- **Studio-Clean Real-Time Audio Engine (48 kHz Stereo):**
  - Lock-free SPSC queues and pre-allocated mixer slots (zero locks/allocations in WASAPI audio callbacks)
  - Bit-transparent voice passthrough (`tanh` soft-peak limiter only shapes soundboard clips, never your live speech)
  - Adaptive clock-drift compensation and `rubato` `SincFixedIn` resampling for mismatched device sample rates
  - Smooth auto voice ducking down to `-24 dB` with hysteresis + 250 ms hold
  - **ITU-R BS.1770** K-weighted gated loudness normalization calculated over your trimmed clip bounds
  - Supports **MP3, WAV (16/24/32-bit PCM & 32/64-bit Float), OGG/Vorbis, FLAC, AAC/M4A, and Opus**
- **Sound Editor & Waveform Trimmer:**
  - Drag start/end trim handles (down to 0.05s) and click anywhere to seek
  - **Monitor-only** headphones preview (`preview_sound` never leaks into Discord/voice chat)
  - Custom categories, icons/emojis, per-clip volume gain, and `Restart` / `Stop` / `Overlap` modes
- **Non-Consuming Global Hotkeys & Mini Overlay:**
  - Uses Windows Raw Input (`RIDEV_INPUTSINK`) pass-through hooks — hotkeys trigger in fullscreen games without ever blocking keys like `Esc` or `F1–F5`
  - Default Panic Key: `Ctrl + Shift + X`
  - Draggable, non-focus-stealing Mini Floating Overlay (`Ctrl + Shift + S`)
  - Lives quietly in the System Tray when closed (`Quit Memeboard` stops streams and restores your default mic)

---

## Quick Start (Windows)

1. Download and run **`Memeboard_0.1.0_x64-setup.exe`** from [Releases](../../releases).
2. The installer automatically sets up the built-in `usbip-win2` driver and `MemeboardVirtualAudio` background service (no separate virtual cable installer needed).
3. Launch **Memeboard** — drag and drop audio files onto the board or click **Add Sound**.
4. Discord, OBS, or games using Windows Default Microphone will immediately hear your normal voice + triggered sound clips.

> You can also select **Memeboard Virtual Mic** manually as your Input Device in Discord/OBS Settings if you disable Auto-Switch Default Mic in **Audio Settings**.

---

## Building From Source

### Prerequisites
- **Node.js** 20+
- **Rust** stable toolchain (`x86_64-pc-windows-msvc` or `x86_64-pc-windows-gnu`)
- Windows 10 / 11

### Development & Production Installer Build

```bash
# 1. Install frontend dependencies
npm install

# 2. Download & SHA256-verify the bundled usbip-win2 driver installer
npm run prepare:driver

# 3. Build the desktop app & NSIS Windows installer
npm run tauri build
```

Run tests:

```bash
# Rust unit tests
cargo test --manifest-path src-tauri/Cargo.toml

# Audio DSP & mixer suite
cargo run --bin test_audio --manifest-path src-tauri/Cargo.toml

# Frontend TypeScript check + Vite build
npm run build
```

---

## License

Licensed under the [MIT License](LICENSE). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for bundled driver and library notices.
