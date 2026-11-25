//! Host USB bus clock aligned with Windows `QueryInterruptTime` / `usbip-win2`.
//! Adapted from Tom56785/virtual-cables (MIT License).

use std::time::{Duration, Instant};

use crate::virtual_mic::event::Event;

pub const TICKS_PER_FRAME: u64 = 10_000;

#[derive(Clone, Copy, Debug)]
pub struct BusTime {
    pub wall: Instant,
    pub coarse: u64,
    pub precise: u64,
}

pub fn current_bus_time() -> BusTime {
    let (coarse, precise) = host_bus_ticks();
    BusTime {
        wall: Instant::now(),
        coarse,
        precise,
    }
}

impl BusTime {
    pub fn frame(&self) -> u64 {
        self.coarse / TICKS_PER_FRAME
    }

    pub fn frame_start(&self, frame: u64) -> Instant {
        let delta = (frame * TICKS_PER_FRAME) as i64 - self.precise as i64;
        let offset = Duration::from_nanos(delta.unsigned_abs() * 100);
        if delta >= 0 {
            self.wall + offset
        } else {
            self.wall.checked_sub(offset).unwrap_or(self.wall)
        }
    }
}

pub fn wait_bus_frame(event: &Event, frame: u64, cancelled: &dyn Fn() -> bool) -> Result<(), Cancelled> {
    loop {
        if host_bus_ticks().0 / TICKS_PER_FRAME >= frame {
            return Ok(());
        }
        if cancelled() {
            return Err(Cancelled);
        }
        event.wait(Some(Instant::now() + Duration::from_micros(500)));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

#[cfg(windows)]
mod host {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    type QueryInterruptTimeFn = unsafe extern "system" fn(*mut u64);

    struct Functions {
        coarse: QueryInterruptTimeFn,
        precise: QueryInterruptTimeFn,
    }

    fn functions() -> Option<&'static Functions> {
        static FUNCTIONS: OnceLock<Option<Functions>> = OnceLock::new();
        FUNCTIONS
            .get_or_init(|| {
                for dll in ["kernel32.dll", "api-ms-win-core-realtime-l1-1-1.dll"] {
                    let name: Vec<u16> = dll.encode_utf16().chain([0]).collect();
                    unsafe {
                        let module = LoadLibraryW(name.as_ptr());
                        if module.is_null() {
                            continue;
                        }
                        let coarse = GetProcAddress(module, c"QueryInterruptTime".as_ptr().cast());
                        let precise =
                            GetProcAddress(module, c"QueryInterruptTimePrecise".as_ptr().cast());
                        if let (Some(coarse), Some(precise)) = (coarse, precise) {
                            return Some(Functions {
                                coarse: std::mem::transmute::<
                                    unsafe extern "system" fn() -> isize,
                                    QueryInterruptTimeFn,
                                >(coarse),
                                precise: std::mem::transmute::<
                                    unsafe extern "system" fn() -> isize,
                                    QueryInterruptTimeFn,
                                >(precise),
                            });
                        }
                    }
                }
                None
            })
            .as_ref()
    }

    pub fn host_bus_ticks() -> (u64, u64) {
        let Some(f) = functions() else {
            return super::fallback_ticks();
        };
        let (mut coarse, mut precise) = (0u64, 0u64);
        unsafe {
            (f.precise)(&mut precise);
            (f.coarse)(&mut coarse);
        }
        (coarse.min(precise), precise)
    }
}

#[cfg(not(windows))]
mod host {
    pub fn host_bus_ticks() -> (u64, u64) {
        super::fallback_ticks()
    }
}

fn fallback_ticks() -> (u64, u64) {
    use std::sync::OnceLock;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let ticks = (EPOCH.get_or_init(Instant::now).elapsed().as_nanos() / 100) as u64;
    (ticks, ticks)
}

pub fn host_bus_ticks() -> (u64, u64) {
    host::host_bus_ticks()
}
