//! Windows service, elevated attach broker, MMCSS Pro Audio scheduling,
//! and usbip-win2 driver detection/installation.

use std::env;
use std::ffi::{CStr, c_void};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CALL_NOT_IMPLEMENTED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_NOT_ACTIVE,
    HANDLE, INVALID_HANDLE_VALUE, LocalFree, NO_ERROR,
};
use windows_sys::Win32::Media::timeBeginPeriod;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_QUERY,
    TokenElevation,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW};
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
    ProcessPowerThrottling, SetPriorityClass, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::uac2::{DEFAULT_LATENCY_MS, Device, bus_id};
use super::usbip::server::Server;

pub const SERVICE_NAME: &str = "MemeboardVirtualAudio";
pub const PIPE_NAME: &str = r"\\.\pipe\MemeboardVirtualAudio";
pub const DEFAULT_LISTEN: &str = "127.0.0.1:3245";
const PIPE_SECURITY: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn last_error() -> String {
    io::Error::last_os_error().to_string()
}

fn system_proc<F: Copy>(dll: &str, name: &CStr) -> Option<F> {
    let dll = wide(dll);
    unsafe {
        let module = LoadLibraryW(dll.as_ptr());
        if module.is_null() {
            return None;
        }
        let proc = GetProcAddress(module, name.as_ptr().cast())?;
        Some(std::mem::transmute_copy(&proc))
    }
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

type SetProcessInformationFn = unsafe extern "system" fn(*mut c_void, i32, *const c_void, u32) -> i32;

pub fn raise_process_priority() {
    unsafe {
        let process = GetCurrentProcess();
        let _ = SetPriorityClass(process, ABOVE_NORMAL_PRIORITY_CLASS);

        if let Some(set_info) =
            system_proc::<SetProcessInformationFn>("kernel32.dll", c"SetProcessInformation")
        {
            let mut state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                    | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
                StateMask: 0,
            };
            let ok = set_info(
                process,
                ProcessPowerThrottling,
                (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            );
            if ok == 0 {
                state.ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
                let _ = set_info(
                    process,
                    ProcessPowerThrottling,
                    (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
                    size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                );
            }
        }
        let _ = timeBeginPeriod(1);
    }
}

pub struct AudioThread(HANDLE);

pub fn enter_audio_thread() -> AudioThread {
    let task = wide("Pro Audio");
    let mut index = 0u32;
    let handle = unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
    AudioThread(handle)
}

impl Drop for AudioThread {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { AvRevertMmThreadCharacteristics(self.0) };
        }
    }
}

pub fn find_usbip_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("driver").join("usbip").join("usbip.exe"));
            candidates.push(dir.join("driver").join("usbip.exe"));
            candidates.push(dir.join("usbip.exe"));
        }
    }
    if let Some(pf) = env::var_os("ProgramFiles").map(PathBuf::from) {
        candidates.push(pf.join("Memeboard").join("driver").join("usbip").join("usbip.exe"));
        candidates.push(pf.join("USBip").join("usbip.exe"));
        candidates.push(pf.join("usbip-win2").join("usbip.exe"));
    }
    if let Some(x86) = env::var_os("ProgramFiles(x86)").filter(|v| !v.is_empty()) {
        let x86 = PathBuf::from(x86);
        candidates.push(x86.join("USBip").join("usbip.exe"));
        candidates.push(x86.join("usbip-win2").join("usbip.exe"));
    }
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            candidates.push(dir.join("usbip.exe"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn is_usbip_kernel_driver_installed() -> bool {
    let subkey = wide(r"SYSTEM\CurrentControlSet\Services\usbip2_ude");
    unsafe {
        let mut hkey = null_mut();
        let status = RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey);
        if status == 0 {
            RegCloseKey(hkey);
            true
        } else {
            false
        }
    }
}

pub fn is_driver_ready() -> bool {
    is_usbip_kernel_driver_installed() && find_usbip_executable().is_some()
}

pub fn find_bundled_driver_installer() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("driver").join("USBip-0.9.8.1-x64.exe"));
            candidates.push(dir.join("resources").join("driver").join("USBip-0.9.8.1-x64.exe"));
            // Handle dev target/debug layout -> project/src-tauri/resources/driver
            for anc in dir.ancestors().take(4) {
                candidates.push(anc.join("src-tauri").join("resources").join("driver").join("USBip-0.9.8.1-x64.exe"));
                candidates.push(anc.join("resources").join("driver").join("USBip-0.9.8.1-x64.exe"));
            }
        }
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("src-tauri").join("resources").join("driver").join("USBip-0.9.8.1-x64.exe"));
        candidates.push(cwd.join("resources").join("driver").join("USBip-0.9.8.1-x64.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn install_bundled_driver_elevated() -> Result<(), String> {
    let installer = find_bundled_driver_installer().ok_or_else(|| {
        "Bundled driver installer USBip-0.9.8.1-x64.exe was not found. Reinstall Memeboard or run `npm run prepare:driver`.".to_string()
    })?;

    let target_dir = env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join("Memeboard")
        .join("driver")
        .join("usbip");

    let params = format!(
        "/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /COMPONENTS=\"main,client\" /TASKS=\"vcredist\" /DIR=\"{}\"",
        target_dir.display()
    );
    let verb = wide("runas");
    let file = wide(&installer.to_string_lossy());
    let args = wide(&params);

    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = args.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(format!("Driver installation cancelled or failed: {}", last_error()));
        }
        if !info.hProcess.is_null() {
            WaitForSingleObject(info.hProcess, 180_000);
            let mut exit_code = 0u32;
            let _ = GetExitCodeProcess(info.hProcess, &mut exit_code);
            CloseHandle(info.hProcess);
            if exit_code != 0 && exit_code != 3010 {
                return Err(format!("Driver installer exited with code {exit_code}"));
            }
        }
    }

    if !is_driver_ready() {
        return Err("Driver installation finished, but usbip2_ude or usbip.exe was not detected.".into());
    }
    Ok(())
}

// --- Service Control Manager client helpers ---

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceState {
    NotInstalled,
    Stopped,
    Pending,
    Running,
}

struct ScHandle(SC_HANDLE);
impl Drop for ScHandle {
    fn drop(&mut self) {
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn open_sc_manager(access: u32) -> Result<ScHandle, String> {
    let mgr = unsafe { OpenSCManagerW(null(), null(), access) };
    if mgr.is_null() {
        return Err(format!("OpenSCManagerW failed: {}", last_error()));
    }
    Ok(ScHandle(mgr))
}

fn open_service_handle(mgr: &ScHandle, access: u32) -> io::Result<ScHandle> {
    let name = wide(SERVICE_NAME);
    let svc = unsafe { OpenServiceW(mgr.0, name.as_ptr(), access) };
    if svc.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(ScHandle(svc))
    }
}

pub fn query_service_state() -> ServiceState {
    let Ok(mgr) = open_sc_manager(SC_MANAGER_CONNECT) else {
        return ServiceState::NotInstalled;
    };
    let Ok(svc) = open_service_handle(&mgr, SERVICE_QUERY_STATUS) else {
        return ServiceState::NotInstalled;
    };
    let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    let ok = unsafe { QueryServiceStatus(svc.0, &mut status) };
    if ok == 0 {
        return ServiceState::Stopped;
    }
    match status.dwCurrentState {
        SERVICE_RUNNING => ServiceState::Running,
        SERVICE_STOPPED => ServiceState::Stopped,
        _ => ServiceState::Pending,
    }
}

pub fn start_installed_service() -> Result<(), String> {
    let mgr = open_sc_manager(SC_MANAGER_CONNECT)?;
    let svc = open_service_handle(&mgr, SERVICE_START | SERVICE_QUERY_STATUS).map_err(|e| {
        if e.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            "Service not installed".to_string()
        } else {
            e.to_string()
        }
    })?;

    unsafe {
        if StartServiceW(svc.0, 0, null()) == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(ERROR_SERVICE_ALREADY_RUNNING as i32) {
                return Err(format!("StartServiceW failed: {err}"));
            }
        }
    }

    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { QueryServiceStatus(svc.0, &mut status) } == 0 {
            return Err(format!("QueryServiceStatus failed: {}", last_error()));
        }
        if status.dwCurrentState == SERVICE_RUNNING {
            return Ok(());
        }
        if status.dwCurrentState == SERVICE_STOPPED {
            return Err("MemeboardVirtualAudio service stopped unexpectedly".into());
        }
        if Instant::now() >= deadline {
            return Err("Timed out waiting for MemeboardVirtualAudio service to start".into());
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

pub fn stop_installed_service() -> Result<(), String> {
    let Ok(mgr) = open_sc_manager(SC_MANAGER_CONNECT) else {
        return Ok(());
    };
    let Ok(svc) = open_service_handle(
        &mgr,
        windows_sys::Win32::System::Services::SERVICE_STOP | SERVICE_QUERY_STATUS,
    ) else {
        return Ok(());
    };
    let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    unsafe {
        if ControlService(svc.0, SERVICE_CONTROL_STOP, &mut status) == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_SERVICE_NOT_ACTIVE as i32) {
                return Ok(());
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if unsafe { QueryServiceStatus(svc.0, &mut status) } == 0 {
            break;
        }
        if status.dwCurrentState == SERVICE_STOPPED || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    Ok(())
}

// --- usbip.exe attachment helpers ---

fn combined_output(command: &mut Command) -> (String, Result<(), String>) {
    command.creation_flags(CREATE_NO_WINDOW);
    let Ok((mut reader, writer)) = std::io::pipe() else {
        return (String::new(), Err("pipe failed".into()));
    };
    let Ok(stderr) = writer.try_clone() else {
        return (String::new(), Err("pipe clone failed".into()));
    };
    command.stdin(Stdio::null()).stdout(writer).stderr(stderr);
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return (String::new(), Err(e.to_string())),
    };
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut output = Vec::new();
    let _ = reader.read_to_end(&mut output);
    let status = child.wait();
    let text = String::from_utf8_lossy(&output).trim().to_string();
    let res = match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("exit {:?}", s.code())),
        Err(e) => Err(e.to_string()),
    };
    (text, res)
}

fn run_usbip(exe: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    let (text, res) = combined_output(&mut cmd);
    match res {
        Ok(()) => Ok(text),
        Err(err) => Err(format!("usbip {}: {err} ({text})", args.join(" "))),
    }
}

fn port_number(line: &str) -> Option<&str> {
    let rest = line
        .get(..4)
        .filter(|p| p.eq_ignore_ascii_case("port"))
        .map(|_| &line[4..])?;
    let digits = rest.trim_start();
    if digits.len() == rest.len() {
        return None;
    }
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 || !digits[end..].starts_with(':') {
        return None;
    }
    Some(&digits[..end])
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerTarget {
    pub host: String,
    pub port: u16,
}

impl ServerTarget {
    pub fn from_listen(listen: &str) -> Result<ServerTarget, String> {
        let invalid = || format!("invalid USB/IP listen address {listen:?}");
        let (host, port) = listen.trim().rsplit_once(':').ok_or_else(invalid)?;
        let port: u16 = port.parse().map_err(|_| invalid())?;
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        if !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
        {
            return Err(invalid());
        }
        let host = match host {
            "" | "0.0.0.0" => "127.0.0.1",
            "::" => "::1",
            h => h,
        };
        Ok(ServerTarget {
            host: host.to_string(),
            port,
        })
    }

    fn url_prefix(&self) -> String {
        if self.host.contains(':') {
            format!("usbip://[{}]:{}/", self.host, self.port)
        } else {
            format!("usbip://{}:{}/", self.host, self.port)
        }
    }

    fn command<'a>(&'a self, port: &'a str, subcommand: &[&'a str]) -> Vec<&'a str> {
        let mut args = vec!["--tcp-port", port, subcommand[0], "-r", self.host.as_str()];
        args.extend_from_slice(&subcommand[1..]);
        args
    }
}

fn own_ports(output: &str, target: &ServerTarget) -> Vec<(String, String)> {
    let prefix = target.url_prefix();
    let mut current_port: Option<String> = None;
    let mut ports = Vec::new();
    for line in output.split('\n') {
        let trimmed = line.trim();
        if let Some(port) = port_number(trimmed) {
            current_port = Some(port.to_string());
            continue;
        }
        if let Some(start) = trimmed.find(&prefix) {
            if let Some(port) = current_port.take() {
                let bus = trimmed[start + prefix.len()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                ports.push((port, bus.to_string()));
            }
        }
    }
    ports
}

pub fn run_attach_helper(attach: bool, full: bool, target: &ServerTarget) -> Result<(), String> {
    let exe = find_usbip_executable()
        .ok_or_else(|| "usbip.exe not found; install driver first".to_string())?;
    let port_str = target.port.to_string();
    let attached = run_usbip(&exe, &["port"])
        .map(|out| own_ports(&out, target))
        .unwrap_or_default();

    let wanted = if attach {
        vec![bus_id(1)]
    } else {
        Vec::new()
    };

    let mut kept = Vec::new();
    for (p, bus) in &attached {
        if !full && wanted.contains(bus) && !kept.contains(&bus.as_str()) {
            kept.push(bus.as_str());
            continue;
        }
        let _ = run_usbip(&exe, &["detach", "-p", p]);
        if !wanted.contains(bus) {
            let _ = run_usbip(&exe, &target.command(&port_str, &["attach", "-b", bus, "--stop"]));
        }
    }

    for bus in &wanted {
        if kept.contains(&bus.as_str()) {
            continue;
        }
        let low_latency = ["attach", "-b", bus.as_str(), "--receive-mode", "low-latency"];
        if run_usbip(&exe, &target.command(&port_str, &low_latency)).is_err() {
            run_usbip(&exe, &target.command(&port_str, &["attach", "-b", bus.as_str()]))?;
        }
    }

    Ok(())
}

// --- Windows Service Mode (`--service`) ---

static SERVICE_STATUS_HANDLE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static SERVICE_CHECKPOINT: AtomicU32 = AtomicU32::new(0);
static SERVICE_STOP: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn report_service_status(state: u32, wait_hint: Duration) {
    let accepted = if state == SERVICE_RUNNING {
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
    } else {
        0
    };
    let checkpoint = match state {
        SERVICE_RUNNING | SERVICE_STOPPED => 0,
        _ => SERVICE_CHECKPOINT.fetch_add(1, Ordering::SeqCst) + 1,
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        dwWin32ExitCode: NO_ERROR,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: checkpoint,
        dwWaitHint: wait_hint.as_millis() as u32,
    };
    unsafe {
        let _ = SetServiceStatus(SERVICE_STATUS_HANDLE.load(Ordering::SeqCst), &status);
    }
}

unsafe extern "system" fn service_control_handler(
    control: u32,
    _: u32,
    _: *mut c_void,
    _: *mut c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            let (stop, stopped) = &SERVICE_STOP;
            *stop.lock().unwrap_or_else(|p| p.into_inner()) = true;
            stopped.notify_all();
            NO_ERROR
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
unsafe impl Send for SecurityDescriptor {}
impl SecurityDescriptor {
    fn new(sddl: &str) -> Result<Self, String> {
        let sddl = wide(sddl);
        let mut desc = null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut desc,
                null_mut(),
            )
        };
        if ok == 0 {
            Err(format!("SDDL parse failed: {}", last_error()))
        } else {
            Ok(Self(desc))
        }
    }
}
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn create_pipe_instance(security: &SecurityDescriptor, first: bool) -> Result<File, String> {
    let attrs = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let open_mode = PIPE_ACCESS_DUPLEX
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    let pipe_mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    let wide_name = wide(PIPE_NAME);
    unsafe {
        let handle = CreateNamedPipeW(
            wide_name.as_ptr(),
            open_mode,
            pipe_mode,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            &attrs,
        );
        if handle == INVALID_HANDLE_VALUE {
            Err(format!("CreateNamedPipeW failed: {}", last_error()))
        } else {
            Ok(File::from_raw_handle(handle))
        }
    }
}

struct PipeServer {
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PipeServer {
    fn start(status_text: Arc<Mutex<String>>) -> Result<Self, String> {
        let security = SecurityDescriptor::new(PIPE_SECURITY)?;
        let first = create_pipe_instance(&security, true)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stopping);
        let thread = std::thread::Builder::new()
            .name("memeboard-svc-pipe".into())
            .spawn(move || {
                let mut pipe = first;
                loop {
                    let connected = unsafe {
                        ConnectNamedPipe(pipe.as_raw_handle(), null_mut()) != 0
                            || io::Error::last_os_error().raw_os_error()
                                == Some(ERROR_PIPE_CONNECTED as i32)
                    };
                    if flag.load(Ordering::SeqCst) {
                        return;
                    }
                    if connected {
                        let st = Arc::clone(&status_text);
                        std::thread::spawn(move || {
                            let mut line = String::new();
                            let _ = BufReader::new((&pipe).take(1024)).read_line(&mut line);
                            let cmd = line.trim();
                            let reply = match cmd {
                                "STATUS" => format!("OK {}\n", st.lock().unwrap_or_else(|p| p.into_inner())),
                                "STOP" => {
                                    let (stop, stopped) = &SERVICE_STOP;
                                    *stop.lock().unwrap_or_else(|p| p.into_inner()) = true;
                                    stopped.notify_all();
                                    "OK\n".to_string()
                                }
                                _ => "ERR\n".to_string(),
                            };
                            let _ = (&pipe).write_all(reply.as_bytes());
                            unsafe {
                                FlushFileBuffers(pipe.as_raw_handle());
                                DisconnectNamedPipe(pipe.as_raw_handle());
                            }
                        });
                    }
                    match create_pipe_instance(&security, false) {
                        Ok(next) => pipe = next,
                        Err(_) => return,
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stopping,
            thread: Some(thread),
        })
    }

    fn stop(mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(PIPE_NAME);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn ping_service_pipe(cmd: &str) -> Result<String, String> {
    let mut attempts = 0;
    let file = loop {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(PIPE_NAME)
        {
            Ok(f) => break f,
            Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && attempts < 25 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => return Err(err.to_string()),
        }
    };
    (&file)
        .write_all(format!("{cmd}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(file.take(4096))
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    Ok(reply.trim().to_string())
}

unsafe extern "system" fn service_main(_: u32, _: *mut windows_sys::core::PWSTR) {
    let name = wide(SERVICE_NAME);
    let handle = unsafe {
        RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(service_control_handler), null())
    };
    if handle.is_null() {
        return;
    }
    SERVICE_STATUS_HANDLE.store(handle, Ordering::SeqCst);
    report_service_status(SERVICE_START_PENDING, Duration::from_secs(10));
    raise_process_priority();

    let status_text = Arc::new(Mutex::new("starting".to_string()));
    let pipe = PipeServer::start(Arc::clone(&status_text)).ok();

    let device = match Device::named(1, "MEMEBOARD-001", "Memeboard Virtual Mic", DEFAULT_LATENCY_MS) {
        Ok(d) => Arc::new(d),
        Err(err) => {
            *status_text.lock().unwrap_or_else(|p| p.into_inner()) = format!("error: {err}");
            if let Some(p) = pipe {
                p.stop();
            }
            report_service_status(SERVICE_STOPPED, Duration::ZERO);
            return;
        }
    };

    let server = Arc::new(Server::new(DEFAULT_LISTEN, vec![device]));
    let server_run = Arc::clone(&server);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let server_thread = std::thread::Builder::new()
        .name("memeboard-usbip-srv".into())
        .spawn(move || {
            let mut tx = Some(ready_tx);
            let _ = server_run.serve(|res| {
                if let Some(tx) = tx.take() {
                    let _ = tx.send(res);
                }
            });
        })
        .ok();

    let server_ok = ready_rx.recv().ok().and_then(|r| r.ok()).is_some();
    if server_ok {
        if let Ok(target) = ServerTarget::from_listen(DEFAULT_LISTEN) {
            match run_attach_helper(true, true, &target) {
                Ok(()) => {
                    *status_text.lock().unwrap_or_else(|p| p.into_inner()) = "attached".to_string();
                }
                Err(err) => {
                    *status_text.lock().unwrap_or_else(|p| p.into_inner()) =
                        format!("attach_failed: {err}");
                }
            }
        }
    } else {
        *status_text.lock().unwrap_or_else(|p| p.into_inner()) = "server_bind_failed".to_string();
    }

    report_service_status(SERVICE_RUNNING, Duration::ZERO);

    let (stop, stopped) = &SERVICE_STOP;
    let mut stopping = stop.lock().unwrap_or_else(|p| p.into_inner());
    while !*stopping {
        stopping = stopped.wait(stopping).unwrap_or_else(|p| p.into_inner());
    }
    drop(stopping);

    report_service_status(SERVICE_STOP_PENDING, Duration::from_secs(15));
    if let Ok(target) = ServerTarget::from_listen(DEFAULT_LISTEN) {
        let _ = run_attach_helper(false, false, &target);
    }
    server.shutdown();
    if let Some(t) = server_thread {
        let _ = t.join();
    }
    if let Some(p) = pipe {
        p.stop();
    }
    report_service_status(SERVICE_STOPPED, Duration::ZERO);
}

pub fn run_windows_service() -> Result<(), String> {
    let mut name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: null_mut(),
            lpServiceProc: None,
        },
    ];
    let ok = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) };
    if ok == 0 {
        Err(format!("StartServiceCtrlDispatcherW failed: {}", last_error()))
    } else {
        Ok(())
    }
}

// --- Portable / Dev Elevated Attach Broker Fallback ---

pub struct InProcessVirtualCable {
    server: Arc<Server>,
    server_thread: Option<JoinHandle<()>>,
    broker_conn: Option<TcpStream>,
}

impl InProcessVirtualCable {
    pub fn start() -> Result<Self, String> {
        let target = ServerTarget::from_listen(DEFAULT_LISTEN)?;
        let device = Arc::new(Device::named(
            1,
            "MEMEBOARD-001",
            "Memeboard Virtual Mic",
            DEFAULT_LATENCY_MS,
        )?);
        let server = Arc::new(Server::new(DEFAULT_LISTEN, vec![device]));
        let server_run = Arc::clone(&server);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let server_thread = std::thread::Builder::new()
            .name("memeboard-usbip-local".into())
            .spawn(move || {
                let mut tx = Some(ready_tx);
                let _ = server_run.serve(|res| {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(res);
                    }
                });
            })
            .map_err(|e| e.to_string())?;

        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                server.shutdown();
                let _ = server_thread.join();
                return Err(e);
            }
            Err(_) => {
                server.shutdown();
                let _ = server_thread.join();
                return Err("USB/IP server failed to start".into());
            }
        }

        if is_elevated() {
            run_attach_helper(true, true, &target)?;
            return Ok(Self {
                server,
                server_thread: Some(server_thread),
                broker_conn: None,
            });
        }

        // Launch one-time elevated attach broker (--attach-broker)
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let token = new_broker_token()?;
        let exe = env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe.parent().unwrap_or_else(|| Path::new("."));
        let params = format!(
            "--attach-broker --broker-address {addr} --broker-token {token} --listen {DEFAULT_LISTEN}"
        );
        let res = unsafe {
            ShellExecuteW(
                null_mut(),
                wide("runas").as_ptr(),
                wide(&exe.to_string_lossy()).as_ptr(),
                wide(&params).as_ptr(),
                wide(&dir.to_string_lossy()).as_ptr(),
                SW_HIDE,
            ) as isize
        };
        if res <= 32 {
            server.shutdown();
            let _ = server_thread.join();
            return Err("Administrator approval for virtual mic attachment was declined.".into());
        }

        let _ = listener.set_nonblocking(true);
        let deadline = Instant::now() + Duration::from_secs(30);
        let conn = loop {
            match listener.accept() {
                Ok((c, _)) => break c,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        server.shutdown();
                        let _ = server_thread.join();
                        return Err("Timed out waiting for attach broker".into());
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                Err(e) => {
                    server.shutdown();
                    let _ = server_thread.join();
                    return Err(e.to_string());
                }
            }
        };
        let _ = conn.set_nonblocking(false);
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let got_tok = read_broker_line(&conn)?;
        if got_tok.trim() != token {
            server.shutdown();
            let _ = server_thread.join();
            return Err("Attach broker authentication failed".into());
        }
        let _ = conn.set_read_timeout(Some(Duration::from_secs(30)));
        (&conn)
            .write_all(b"ATTACH\n")
            .map_err(|e| e.to_string())?;
        let reply = read_broker_line(&conn)?;
        if reply.trim() != "OK" {
            server.shutdown();
            let _ = server_thread.join();
            return Err(format!("Attach broker failed: {reply}"));
        }

        Ok(Self {
            server,
            server_thread: Some(server_thread),
            broker_conn: Some(conn),
        })
    }

    pub fn stop(mut self) {
        if let Some(conn) = self.broker_conn.take() {
            let _ = (&conn).write_all(b"QUIT\n");
            let _ = read_broker_line(&conn);
        } else if is_elevated() {
            if let Ok(target) = ServerTarget::from_listen(DEFAULT_LISTEN) {
                let _ = run_attach_helper(false, false, &target);
            }
        }
        self.server.shutdown();
        if let Some(t) = self.server_thread.take() {
            let _ = t.join();
        }
    }
}

fn new_broker_token() -> Result<String, String> {
    let mut raw = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            raw.as_mut_ptr(),
            raw.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(format!("BCryptGenRandom failed: 0x{:08x}", status as u32));
    }
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

fn read_broker_line(mut conn: &TcpStream) -> Result<String, String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match conn.read(&mut byte) {
            Ok(0) => return Err("EOF".into()),
            Ok(_) if byte[0] == b'\n' => {
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            Ok(_) => line.push(byte[0]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn run_attach_broker(address: &str, token: &str, listen: &str) -> Result<(), String> {
    if !address.starts_with("127.0.0.1:") || token.len() != 64 {
        return Err("invalid broker parameters".into());
    }
    let target = ServerTarget::from_listen(listen)?;
    let addr: SocketAddr = address.parse().map_err(|e| format!("{e}"))?;
    let conn = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|e| format!("broker connect failed: {e}"))?;
    (&conn)
        .write_all(format!("{token}\n").as_bytes())
        .map_err(|e| e.to_string())?;

    let mut writer = &conn;
    for line in BufReader::new(&conn).lines() {
        let Ok(line) = line else {
            break;
        };
        match line.trim() {
            "ATTACH" => {
                let res = run_attach_helper(true, true, &target);
                let msg = match res {
                    Ok(()) => "OK\n".to_string(),
                    Err(e) => format!("ERR {e}\n"),
                };
                let _ = writer.write_all(msg.as_bytes());
            }
            "QUIT" => {
                let _ = run_attach_helper(false, false, &target);
                let _ = writer.write_all(b"OK\n");
                return Ok(());
            }
            _ => {
                let _ = writer.write_all(b"ERR unknown\n");
            }
        }
    }

    let _ = run_attach_helper(false, false, &target);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_numbers() {
        assert_eq!(port_number("Port 01: <Port in Use>"), Some("01"));
        assert_eq!(port_number("PORT  3: <x>"), Some("3"));
        assert_eq!(port_number("Portal 1:"), None);
    }

    #[test]
    fn matches_own_usbip_ports() {
        let output = "Port 01: <Port in Use>\n   ?-? -> usbip://127.0.0.1:3245/1-1\nPort 02: <Port in Use>\n   ?-? -> usbip://127.0.0.1:3240/1-1\n";
        let target = ServerTarget::from_listen("127.0.0.1:3245").unwrap();
        assert_eq!(own_ports(output, &target), vec![("01".into(), "1-1".into())]);
    }
}
