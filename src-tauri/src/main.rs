// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(exit_code) = memeboard_lib::virtual_mic::maybe_run_helper_mode() {
        std::process::exit(exit_code);
    }
    memeboard_lib::run()
}
