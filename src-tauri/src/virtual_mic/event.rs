//! An auto-reset event with precise, interruptible timed waits.

pub use imp::Event;

#[cfg(windows)]
mod imp {
    use std::ptr::null;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CancelWaitableTimer, CreateEventW,
        CreateWaitableTimerExW, INFINITE, SetEvent, SetWaitableTimer, TIMER_ALL_ACCESS,
        WaitForMultipleObjects, WaitForSingleObject,
    };

    pub struct Event {
        event: HANDLE,
        timer: HANDLE,
    }

    unsafe impl Send for Event {}
    unsafe impl Sync for Event {}

    impl Event {
        pub fn new() -> Event {
            unsafe {
                let event = CreateEventW(null(), 0, 0, null());
                assert!(!event.is_null(), "CreateEventW failed");
                let mut timer = CreateWaitableTimerExW(
                    null(),
                    null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                );
                if timer.is_null() {
                    timer = CreateWaitableTimerExW(null(), null(), 0, TIMER_ALL_ACCESS);
                }
                assert!(!timer.is_null(), "CreateWaitableTimerExW failed");
                Event { event, timer }
            }
        }

        pub fn signal(&self) {
            unsafe {
                let _ = SetEvent(self.event);
            }
        }

        pub fn wait(&self, deadline: Option<Instant>) -> bool {
            let Some(deadline) = deadline else {
                unsafe {
                    return WaitForSingleObject(self.event, INFINITE) == WAIT_OBJECT_0;
                }
            };

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                unsafe {
                    return WaitForSingleObject(self.event, 0) == WAIT_OBJECT_0;
                }
            }

            let ticks = (remaining.as_nanos() / 100).max(1);
            let due = -(i64::try_from(ticks).unwrap_or(i64::MAX));
            unsafe {
                if SetWaitableTimer(self.timer, &due, 0, None, null(), 0) == 0 {
                    let ms = remaining.as_millis().max(1).min(u32::MAX as u128 - 1) as u32;
                    return WaitForSingleObject(self.event, ms) == WAIT_OBJECT_0;
                }
                let handles = [self.event, self.timer];
                let result = WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE);
                if result == WAIT_OBJECT_0 {
                    CancelWaitableTimer(self.timer);
                    return true;
                }
                false
            }
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.timer);
                CloseHandle(self.event);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::{Condvar, Mutex};
    use std::time::Instant;

    pub struct Event {
        signalled: Mutex<bool>,
        changed: Condvar,
    }

    impl Event {
        pub fn new() -> Event {
            Event {
                signalled: Mutex::new(false),
                changed: Condvar::new(),
            }
        }

        pub fn signal(&self) {
            *self.signalled.lock().unwrap_or_else(|p| p.into_inner()) = true;
            self.changed.notify_one();
        }

        pub fn wait(&self, deadline: Option<Instant>) -> bool {
            let mut signalled = self.signalled.lock().unwrap_or_else(|p| p.into_inner());
            loop {
                if *signalled {
                    *signalled = false;
                    return true;
                }
                match deadline {
                    None => {
                        signalled = self
                            .changed
                            .wait(signalled)
                            .unwrap_or_else(|p| p.into_inner());
                    }
                    Some(deadline) => {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return false;
                        }
                        signalled = self
                            .changed
                            .wait_timeout(signalled, remaining)
                            .unwrap_or_else(|p| p.into_inner())
                            .0;
                    }
                }
            }
        }
    }
}

impl Default for Event {
    fn default() -> Event {
        Event::new()
    }
}
