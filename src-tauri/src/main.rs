// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Guard against known WebKitGTK DMABUF blank-window crashes on Linux NVIDIA setups
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
            && std::path::Path::new("/dev/nvidia0").exists()
        {
            unsafe {
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            }
        }
    }

    if let Some(exit_code) = memeboard_lib::virtual_mic::maybe_run_helper_mode() {
        std::process::exit(exit_code);
    }
    memeboard_lib::run()
}
