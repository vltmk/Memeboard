# Third-Party Notices

Memeboard redistributes or builds upon the following open-source software:

## usbip-win2 (`USBip-0.9.8.1-x64.exe`)
- Repository: https://github.com/vadimgrn/usbip-win2
- License: BSD-2-Clause License
- Used to provide a WHQL/EV-signed kernel virtual USB host controller so Windows' built-in inbox `usbaudio2.sys` creates **Memeboard Virtual Mic** without manual third-party driver installation or Test Signing mode.

## virtual-cables (UAC2 USB/IP Virtual Audio Architecture)
- Repository: https://github.com/Tom56785/virtual-cables
- License: MIT License
- Memeboard's in-process USB/IP UAC2 virtual audio device and Windows service / `IPolicyConfig` integration is adapted from `Tom56785/virtual-cables`.

## Rust & TypeScript Audio / Desktop Crates
- **Tauri v2** (MIT / Apache-2.0)
- **CPAL** (Apache-2.0)
- **Symphonia** (MPL-2.0)
- **mousiki** (BSD-3-Clause)
- **rubato** (MIT)
- **ringbuf** (MIT / Apache-2.0)
- **Lucide Icons** (ISC)
