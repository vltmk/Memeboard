Memeboard Built-in Virtual Microphone Driver Directory
======================================================

When built for distribution (`npm run tauri build` / `npm run prepare:driver`),
this directory stages the WHQL-signed `vadimgrn/usbip-win2` installer
(`USBip-0.9.8.1-x64.exe`, BSD-2-Clause License) for silent installation during
Memeboard setup.

Memeboard pairs `usbip-win2` with Windows' built-in USB Audio Class 2.0 driver
(`usbaudio2.sys`) to create a zero-extra-install, bit-exact virtual microphone.
