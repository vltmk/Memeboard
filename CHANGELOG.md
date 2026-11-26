# Changelog

All notable changes to Memeboard are documented in this file.

## [0.2.0] - 2026-11-26

### Added
- **Bulk Packs & Templates (`.memeboard` / `.zip`)**:
  - Export full soundboards or individual categories into portable `.memeboard` / `.zip` packs bundling audio assets and `memeboard.json` metadata templates (`name`, `emoji`, `category`, `volume`, `hotkey`, `trimStart`, `trimEnd`, `playbackMode`).
  - Import `.memeboard` packs, plain `.zip` audio archives, or bulk-import multiple audio files with a shared template via the new **Packs & Bulk Templates** dialog (`PackTransferModal`).
- **Audio File Download**:
  - Added a right-click context menu on sound pills (`Download Audio…`, `Play in Mic / Stop`, `Preview Locally`, `Edit Sound…`, quick `Category` assignment, `Delete`) and a **Download** button inside `SoundEditorModal`.
- **Category Creation & Content Management**:
  - Added persistent `categories` storage in `state.json`, a `+ Category` button and `Edit` control in the category bar, a right-click menu on category pills, and `CategoryModal` for renaming, deleting, exporting, and selecting which clips belong to a category.
- **Custom Typography**:
  - Bundled `Inter Variable` as the main UI font, `Ioskeley Mono` (`IoskeleyMono-Regular.woff2`) as the monospace/accent font, and `IRANYekanWeb` (`iranyekanweblight.woff2`) scoped to Arabic/Persian Unicode ranges with `dir="auto"` text support.
- **Documentation**:
  - Added [Architecture Guide](docs/ARCHITECTURE.md), [Testing Guide](docs/TESTING.md), and `CHANGELOG.md`.

### Changed
- **Monochromatic App Icon**: Replaced the default Tauri icon with a custom monochromatic 5-bar acoustic waveform icon across `src-tauri/icons/*`, the tray, header, and overlay.
- **Docked Bottom-Right Drawer UI**: Converted the main window to a frameless bottom-right drawer anchored flush to the Windows work area (`0, 0`) with custom drag/minimize/tray controls, dock-state tracking (`DOCKED` / `FLOAT`), and a slide-in entrance transition.
- **Animated Live / Voice Live Indicator**: Added smooth CSS breathing halo (`Live`), sonar ripple, and 3-bar equalizer animations (`Voice Live`) in the header status pill.
- **Perceptual Mic SFX Volume Curve**: Applied a quadratic perceptual taper (`v^2` for `0.0..=1.0`) to broadcast and monitor volume controls and adjusted the default `loudnessTargetLufs` to `-16.0` LUFS for cleaner Discord voice balancing.

### Fixed
- **Overlay White Border**: Disabled DWM shadow (`shadow: false`) and Windows 11 DWM rounding/border color (`DWMWCP_DONOTROUND`, `DWMWA_COLOR_NONE`) on the transparent overlay so no thick white border renders around it.

---

## [0.1.0] - Initial Release
- Built-in `usbip-win2` UAC2 Virtual Microphone with Windows Service (`MemeboardVirtualAudio`) and automatic default microphone switching/restoration.
- Lock-free 48 kHz stereo WASAPI mixer with voice ducking, BS.1770 loudness normalization, waveform trim editor, and `+7 Hz` AEC-safe headphone monitoring.
- Windows Raw Input (`RIDEV_INPUTSINK`) non-consuming global hotkeys and mini floating overlay.
