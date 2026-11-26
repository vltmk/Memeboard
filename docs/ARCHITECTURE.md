# Memeboard Architecture

Memeboard is a Windows desktop soundboard built on **Tauri v2** (`src-tauri/`) with a **React 19 + TypeScript + Tailwind CSS v4** frontend (`src/`).

---

## 1. High-Level System Overview

```text
+-------------------------------------------------------------------+
| React Frontend (src/)                                             |
|  - Main Drawer (App.tsx, Header, SoundGrid, Category/Pack Modals) |
|  - Mini Overlay (?overlay=true -> OverlayView.tsx)                |
+-----------------------------+-------------------------------------+
                              | Tauri IPC commands & events
+-----------------------------v-------------------------------------+
| Tauri Core & State (src-tauri/src/lib.rs, commands.rs)            |
|  - StateManager (%LOCALAPPDATA%\Memeboard\state.json & sounds/)   |
|  - Pack Exporter / Importer (.memeboard / .zip)                   |
|  - Windows Raw Input Global Hotkeys (hotkeys.rs)                  |
|  - Frameless Docking & Overlay Management (Win32 / DWM)           |
+-----------------------------+-------------------------------------+
                              | Lock-free SPSC rings + Atomics
+-----------------------------v-------------------------------------+
| Real-Time Audio Engine (device_manager.rs, mixer.rs, decoder.rs)  |
|  - Mic Capture (WASAPI) ---> Broadcast StreamProcessor            |
|                                  |--> Virtual Mic (WASAPI Render) |
|                              Monitor StreamProcessor              |
|                                  |--> Headphones (WASAPI Render)  |
+-----------------------------+-------------------------------------+
                              | USB Audio Class 2.0 (UAC2)
+-----------------------------v-------------------------------------+
| Built-in Virtual Mic (src-tauri/src/virtual_mic/)                 |
|  - Local USB/IP Server + UAC2 Descriptors + DelayLine             |
|  - Windows Service (MemeboardVirtualAudio) & usbip2_ude driver    |
|  - IPolicyConfig Default Mic Switcher & Crash Recovery            |
+-------------------------------------------------------------------+
```

---

## 2. Audio Engine & DSP Pipeline

### 2.1 Decoding & Loudness Normalization (`src-tauri/src/decoder.rs`)
- Decodes **MP3, WAV, OGG/Vorbis, FLAC, AAC/M4A, Opus (`mousiki`), and WebM** via `symphonia` and `mousiki`.
- Resamples non-48 kHz sources to **48,000 Hz stereo interleaved (`f32`)** using `rubato::SincFixedIn` (BlackmanHarris2 window).
- Generates an 80-point normalized waveform peak array (`peaks`) for the trim editor.
- Computes **ITU-R BS.1770-4** K-weighted gated loudness over `[trim_start, trim_end]` (100 ms blocks, `-70 LUFS` absolute gate, `-10 LU` relative gate) targeting `loudness_target_lufs` (default `-16.0 LUFS`, clamped gain `0.15..=3.5`).

### 2.2 Lock-Free Dual-Stream Mixer (`src-tauri/src/mixer.rs`)
Audio callbacks never allocate or block on mutexes:
- `AudioMixer` owns a `SharedMixerControls` struct of atomics (`AtomicU32` bit-packed `f32` gains, `AtomicBool` mutes/flags, and `[AtomicU64; 16]` playing bitsets supporting up to `MAX_SOUND_SLOTS = 1024`).
- Control commands (`LoadSound`, `UnloadSound`, `TriggerSound`, `StopSound`, `StopAll`) are sent over independent `ringbuf` SPSC queues (`bcast_tx`, `mon_tx`) to two independent `StreamProcessor` instances:
  1. **Broadcast Processor (`is_broadcast = true`)**:
     - Reads physical mic samples from a `4096`-frame lock-free capture ring with jitter prefill (`MIC_CUSHION_FRAMES = 480`) and clock-drift compensation in `device_manager.rs`.
     - Runs Voice Activity Detection (VAD) with hysteresis (`VOICE_OPEN_THRESH = 0.018`, `VOICE_CLOSE_THRESH = 0.009`, `250 ms` hold).
     - Applies smooth exponential attack (`15 ms`) / release (`300 ms`) voice ducking (`voice_ducking_db`, `-24..0 dB`) to soundboard clips when you speak.
     - Applies `perceptual_volume_gain` (`v * v` for `v <= 1.0`, linear above `1.0`) so `50%` slider = `-12 dB` attenuation while `100%` stays at unity (`1.0`).
     - Soft-limits **only** the SFX bus (`soft_peak_limiter`, threshold `0.85`), then blends mic voice with `blend_preserving_voice(mic, sound)` so live speech stays bit-clean below `±0.99`.
  2. **Monitor Processor (`is_broadcast = false`)**:
     - Plays both normal clips and `preview_only` editor previews (previews never enter the broadcast stream).
     - Optionally applies a `+7 Hz` single-sideband Hilbert phase shift (`StereoHilbertShifter`) to active broadcast clips on headphones (`aec_safe_monitor`) so Discord/TeamSpeak/WebRTC linear Acoustic Echo Cancellation cannot lock onto and cancel clips bleeding from open-back headphones or speakers.

### 2.3 Device Routing (`src-tauri/src/device_manager.rs`)
- Runs a dedicated single-threaded WASAPI actor (`memeboard-wasapi-actor`) so `!Send` `cpal::Stream` handles never cross threads.
- Strictly avoids routing physical mic capture to virtual endpoints or motherboard loopback mixers (e.g., `Sonic Studio Virtual Mixer`).
- Elevates audio callback threads via MMCSS (`Pro Audio`) in `windows_svc.rs`.

---

## 3. Built-in Virtual Microphone (`src-tauri/src/virtual_mic/`)

- **UAC2 Device (`uac2.rs`, `delay_line.rs`)**: Emulates a full-speed USB Audio Class 2.0 device at `48 kHz` stereo with playback (render) and recording (capture) endpoints connected by a `DelayLine`. Windows binds its inbox `usbaudio2.sys` driver automatically.
- **USB/IP Transport (`usbip/`)**: Local TCP USB/IP server talking to the WHQL-signed `usbip-win2` (`usbip2_ude`) kernel driver.
- **Service vs In-Process (`windows_svc.rs`, `mod.rs`)**:
  - Installed builds register `MemeboardVirtualAudio` (`memeboard.exe --service`), allowing standard non-elevated users to start/attach the virtual device without UAC prompts.
  - Portable/dev builds fall back to `InProcessVirtualCable` when the service isn't installed.
- **Default Mic Switching (`policy.rs`)**: Uses COM `IPolicyConfig` to save the user's physical default console & communications capture IDs to `%LOCALAPPDATA%\Memeboard\mic_restore.json`, switches defaults to **Memeboard Virtual Mic**, guards speaker defaults against PnP changes, and restores the physical mic cleanly on quit (or recovers on next startup after a crash).

---

## 4. Global Hotkeys & Single-Instance Activation (`src-tauri/src/hotkeys.rs`)

- Acquires a Windows named mutex (`Local\MemeboardGuiSingleInstance_v1`). A second launch posts `WM_SHOW_MAIN_WINDOW` to the existing instance's message-only window (`MemeboardRawInputHotkeyWindow`) and exits.
- Registers `RAWINPUTDEVICE` (`RIDEV_INPUTSINK`) for `RIM_TYPEKEYBOARD` on a dedicated pump thread:
  - Non-consuming pass-through (never hooks/blocks keys in games like `Esc` or `F1–F24`).
  - Suppresses key auto-repeat via `pressed_vkeys` tracking and recovers stuck modifiers after Win+L / UAC via `GetAsyncKeyState`.
  - Paused automatically during hotkey capture in the UI (`set_hotkey_recording`).

---

## 5. Persistence, Security & Pack Format (`src-tauri/src/state_manager.rs`)

### 5.1 Storage Layout
All persistent user data resides in `%LOCALAPPDATA%\Memeboard\`:
- `state.json` — `SoundboardState` (`sounds`, `settings`, `categories`), written atomically via `state.json.tmp`.
- `sounds/<uuid>.<ext>` — Managed local copies of imported audio files.
- `mic_restore.json` — Crash-safe microphone policy backup.

### 5.2 Path Containment
Both `delete_sound_file` and `export_sound_file` / `export_pack` canonicalize paths and verify `.starts_with(canon_sounds_dir)` before reading or deleting managed audio files.

### 5.3 `.memeboard` / `.zip` Pack Schema
Pack archives are standard Deflate-compressed ZIP files containing:
- `memeboard.json`:
  ```json
  {
    "formatVersion": 1,
    "app": "Memeboard",
    "exportedAt": 1795664427,
    "categories": ["General", "Voice"],
    "sounds": [
      {
        "name": "Clip Name",
        "emoji": "🔊",
        "category": "Voice",
        "volume": 1.0,
        "hotkey": "Control+Shift+B",
        "trimStart": 0.0,
        "trimEnd": 1.45,
        "playbackMode": "restart",
        "audioFile": "audio/001-Clip Name.mp3"
      }
    ]
  }
  ```
- `audio/<index>-<sanitized_name>.<ext>`: Raw audio assets.
- `import_pack` validates archive paths against traversal (`..`, absolute prefixes), imports all entries, preserves template metadata, and recomputes BS.1770 normalization against the user's current `loudnessTargetLufs`. If a `.zip` lacks `memeboard.json`, all supported audio files inside are still imported cleanly.

---

## 6. Windowing & Frontend UI (`src/`, `commands.rs`, `tauri.conf.json`)

- **Main Window (`main`)**: Frameless (`decorations: false`, `shadow: false`, `transparent: false`), sized `396×640`, docked flush (`0, 0`) to `SPI_GETWORKAREA` bottom-right. Uses Windows 11 `DwmSetWindowAttribute` (`DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUNDSMALL`, `DWMWA_BORDER_COLOR = #27272A`) and tracks dock state on `WindowEvent::Moved`.
- **Mini Overlay (`overlay`)**: Transparent, frameless, shadowless (`decorations: false`, `transparent: true`, `shadow: false`), `alwaysOnTop: true`, `focusable: false`, `skipTaskbar: true`, shown via `SW_SHOWNOACTIVATE` + `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` with `DWMWCP_DONOTROUND` and `DWMWA_COLOR_NONE` to prevent Windows 11 border artifacts.
- **Typography (`src/App.css`, `src/main.tsx`)**:
  - Primary UI font: `Inter Variable` (`@fontsource-variable/inter`)
  - Accent/monospace font: `Ioskeley Mono` (`IoskeleyMono-Regular.woff2`)
  - Persian/Arabic script font: `IRANYekanWeb` (`iranyekanweblight.woff2`) scoped via CSS `unicode-range` (`U+0600-06FF`, `U+0750-077F`, `U+08A0-08FF`, `U+FB50-FDFF`, `U+FE70-FEFF`, `U+200C-200D`), paired with `dir="auto"` on sound names and text inputs.
