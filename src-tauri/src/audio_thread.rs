//! Cross-platform real-time audio thread elevation.

#[cfg(windows)]
pub use crate::virtual_mic::windows_svc::{AudioThread, enter_audio_thread};

#[cfg(not(windows))]
mod imp {
    use audio_thread_priority::{
        RtPriorityHandle, demote_current_thread_from_real_time,
        promote_current_thread_to_real_time,
    };

    pub struct AudioThread {
        handle: Option<RtPriorityHandle>,
    }

    impl Drop for AudioThread {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = demote_current_thread_from_real_time(handle);
            }
        }
    }

    pub fn enter_audio_thread() -> AudioThread {
        let handle = promote_current_thread_to_real_time(480, 48_000).ok();
        AudioThread { handle }
    }
}

#[cfg(not(windows))]
pub use imp::{AudioThread, enter_audio_thread};
