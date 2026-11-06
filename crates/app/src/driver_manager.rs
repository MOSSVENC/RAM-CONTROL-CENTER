//! Driver health detection and the elevated, driver-only installer path.
//!
//! The application only needs the signed OEM `AcpiBridge` package. The
//! keyboard filter remains visible as an optional component because the
//! current FlexiKey engine uses a user-mode hook rather than HKKbdFltr.

use std::path::{Path, PathBuf};

use neo_control_protocol::{Dchu, KeyboardFilter};

const ACPI_SERVICE: &str = "AcpiBridge";
const KEYBOARD_SERVICE: &str = "HKKbdFltr";
const ACPI_PAYLOAD_FILES: [&str; 3] = ["AcpiBridge.inf", "AcpiBridge.sys", "acpibridge.cat"];

#[derive(Clone, Debug, Default)]
pub(crate) struct DriverHealth {
    pub(crate) core_ready: bool,
    pub(crate) core_installed: bool,
    pub(crate) keyboard_ready: bool,
    pub(crate) keyboard_installed: bool,
    pub(crate) payload_ready: bool,
    pub(crate) keyboard_payload_ready: bool,
    pub(crate) payload_path: PathBuf,
}

impl DriverHealth {
    pub(crate) fn required_missing(&self) -> bool {
        !self.core_ready
    }
}

#[derive(Clone, Debug)]
struct AcpiPayload {
    #[cfg_attr(not(windows), allow(dead_code))]
    inf: PathBuf,
}

pub(crate) fn detect(acpi_ready: bool) -> DriverHealth {
    let root = payload_root();
    let payload = acpi_payload_at(&root);
    let keyboard_ready = KeyboardFilter::open().is_ok();
    DriverHealth {
        core_ready: acpi_ready,
        core_installed: acpi_ready || service_exists(ACPI_SERVICE),
        keyboard_ready,
        keyboard_installed: keyboard_ready || service_exists(KEYBOARD_SERVICE),
        payload_ready: payload.is_some(),
        keyboard_payload_ready: keyboard_payload_at(&root).is_some(),
        payload_path: root,
    }
}

pub(crate) fn smoke() -> i32 {
    let acpi_ready = Dchu::open().is_ok();
    let health = detect(acpi_ready);
    println!("acpi_ready={}", health.core_ready);
    println!("acpi_installed={}", health.core_installed);
    println!("keyboard_filter_ready={}", health.keyboard_ready);
    println!("keyboard_filter_installed={}", health.keyboard_installed);
    println!("acpi_payload_ready={}", health.payload_ready);
    println!("keyboard_payload_ready={}", health.keyboard_payload_ready);
    println!("payload_path={}", health.payload_path.display());
    if health.core_ready { 0 } else { 1 }
}

pub(crate) fn run_installer() -> i32 {
    #[cfg(windows)]
    {
        platform::run_installer()
    }
    #[cfg(not(windows))]
    {
        eprintln!("AcpiBridge installation is only available on Windows");
        1
    }
}

#[derive(Debug)]
pub(crate) enum InstallLaunchError {
    #[cfg_attr(not(windows), allow(dead_code))]
    Cancelled,
    Failed(String),
}

pub(crate) fn install_and_wait() -> Result<u32, InstallLaunchError> {
    #[cfg(windows)]
    {
        platform::install_and_wait()
    }
    #[cfg(not(windows))]
    {
        Err(InstallLaunchError::Failed(
            "driver installation is only available on Windows".to_owned(),
        ))
    }
}

#[cfg(windows)]
struct ElevatedInstaller {
    handle: usize,
}

fn payload_root() -> PathBuf {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        candidates.push(parent.join("drivers"));
    }
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("NeoControl")
        .join("drivers");
    if !candidates
        .iter()
        .any(|candidate| candidate == &program_data)
    {
        candidates.push(program_data);
    }

    candidates
        .iter()
        .find(|candidate| acpi_payload_at(candidate).is_some())
        .cloned()
        .or_else(|| candidates.into_iter().next())
        .unwrap_or_default()
}

fn acpi_payload_at(root: &Path) -> Option<AcpiPayload> {
    let directory = root.join("acpibridge");
    if ACPI_PAYLOAD_FILES
        .iter()
        .all(|name| directory.join(name).is_file())
    {
        Some(AcpiPayload {
            inf: directory.join("AcpiBridge.inf"),
        })
    } else {
        None
    }
}

fn keyboard_payload_at(root: &Path) -> Option<PathBuf> {
    let directory = root.join("hkkbdfltr");
    let files = [
        "HKKbdFltr.inf",
        "HKKbdFltr.sys",
        "hkkbdfltr.cat",
        "HKFltrInstaller.exe",
    ];
    files
        .iter()
        .all(|name| directory.join(name).is_file())
        .then_some(directory)
}

#[cfg(windows)]
fn service_exists(name: &str) -> bool {
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
    else {
        return false;
    };
    manager
        .open_service(name, ServiceAccess::QUERY_STATUS)
        .is_ok()
}

#[cfg(not(windows))]
fn service_exists(_name: &str) -> bool {
    false
}

#[cfg(windows)]
mod platform {
    use std::ffi::{OsStr, OsString};
    use std::io::Error;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Output};
    use std::ptr;
    use std::thread;
    use std::time::Duration;

    use neo_control_protocol::Dchu;
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        IsUserAnAdmin, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };

    use super::{
        ACPI_SERVICE, ElevatedInstaller, InstallLaunchError, acpi_payload_at, payload_root,
    };

    const ERROR_CANCELLED: i32 = 1223;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub(super) fn run_installer() -> i32 {
        let log = log_path();
        append_log(&log, "installer: start");
        if Dchu::open().is_ok() {
            append_log(
                &log,
                "installer: AcpiBridge already responding; nothing to install",
            );
            return 0;
        }
        // SAFETY: IsUserAnAdmin takes no arguments and has no pointer preconditions.
        if unsafe { IsUserAnAdmin() } == 0 {
            append_log(&log, "installer: process is not elevated");
            return 3;
        }

        let root = payload_root();
        let Some(payload) = acpi_payload_at(&root) else {
            append_log(
                &log,
                &format!(
                    "installer: required payload missing under {}",
                    root.display()
                ),
            );
            return 2;
        };
        append_log(&log, &format!("installer: using {}", payload.inf.display()));

        let system_root =
            std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from("C:\\Windows"));
        let pnputil = Path::new(&system_root).join("System32").join("pnputil.exe");
        let output = match hidden_command(
            &pnputil,
            [
                OsStr::new("/add-driver"),
                payload.inf.as_os_str(),
                OsStr::new("/install"),
            ],
        ) {
            Ok(output) => output,
            Err(error) => {
                append_log(&log, &format!("installer: pnputil launch failed: {error}"));
                return 4;
            }
        };
        append_output(&log, "pnputil", &output);
        if !output.status.success() {
            append_log(
                &log,
                &format!("installer: pnputil exit={:?}", output.status.code()),
            );
            return 4;
        }

        for attempt in 0_u32..40 {
            if Dchu::open().is_ok() {
                append_log(&log, "installer: AcpiBridge device is ready");
                return 0;
            }
            if attempt.is_multiple_of(4) {
                start_acpi_service(&log);
            }
            thread::sleep(Duration::from_millis(250));
        }
        append_log(
            &log,
            "installer: driver package staged, but AcpiBridge did not become ready",
        );
        5
    }

    pub(super) fn install_and_wait() -> Result<u32, InstallLaunchError> {
        // ShellExecuteExW is a shell/COM entry point. This helper runs on its
        // own thread, so initialize an apartment and release it on return.
        // RPC_E_CHANGED_MODE is harmless if the thread already has another
        // apartment; ShellExecuteExW can still proceed.
        // SAFETY: both COM calls have no pointer preconditions.
        let initialized =
            unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;
        let result = launch_elevated_installer().and_then(|installer| {
            wait_for_process(installer.handle).map_err(InstallLaunchError::Failed)
        });
        if initialized {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        result
    }

    fn launch_elevated_installer() -> Result<ElevatedInstaller, InstallLaunchError> {
        let executable = std::env::current_exe()
            .map_err(|error| InstallLaunchError::Failed(format!("current executable: {error}")))?;
        let operation = wide(OsStr::new("runas"));
        let file = wide(executable.as_os_str());
        let parameters = wide(OsStr::new("--install-drivers"));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: ptr::null_mut(),
            lpVerb: operation.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            lpDirectory: ptr::null(),
            nShow: 1,
            ..Default::default()
        };
        // SAFETY: the SHELLEXECUTEINFOW and every referenced wide string stay
        // alive for the duration of the call; the structure is initialized by
        // the Windows API contract.
        let ok = unsafe { ShellExecuteExW(&mut info) };
        if ok == 0 {
            let error = Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_CANCELLED) {
                return Err(InstallLaunchError::Cancelled);
            }
            return Err(InstallLaunchError::Failed(format!(
                "ShellExecuteExW(runas): {error}"
            )));
        }
        if info.hProcess.is_null() {
            return Err(InstallLaunchError::Failed(
                "ShellExecuteExW(runas) returned no process handle".to_owned(),
            ));
        }
        Ok(ElevatedInstaller {
            handle: info.hProcess as usize,
        })
    }

    fn wait_for_process(raw_handle: usize) -> Result<u32, String> {
        let handle = raw_handle as HANDLE;
        // SAFETY: the handle was returned by ShellExecuteExW with
        // SEE_MASK_NOCLOSEPROCESS and is owned by this ElevatedInstaller.
        let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
        let mut code = 0_u32;
        // SAFETY: the handle remains alive until CloseHandle below, and `code`
        // points to valid writable storage.
        let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
        // SAFETY: this wrapper uniquely owns the process handle.
        unsafe { CloseHandle(handle) };
        if wait != WAIT_OBJECT_0 {
            return Err(format!("WaitForSingleObject returned {wait}"));
        }
        if ok == 0 {
            return Err(format!(
                "GetExitCodeProcess failed: {}",
                Error::last_os_error()
            ));
        }
        Ok(code)
    }

    fn hidden_command<const N: usize>(
        program: &Path,
        args: [&OsStr; N],
    ) -> std::io::Result<Output> {
        let mut command = Command::new(program);
        command.args(args);
        command.creation_flags(CREATE_NO_WINDOW);
        command.output()
    }

    fn start_acpi_service(log: &Path) {
        let result = (|| -> Result<(), String> {
            let manager =
                ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
                    .map_err(|error| error.to_string())?;
            let service = manager
                .open_service(
                    ACPI_SERVICE,
                    ServiceAccess::START | ServiceAccess::QUERY_STATUS,
                )
                .map_err(|error| error.to_string())?;
            service
                .start::<&OsStr>(&[])
                .map_err(|error| error.to_string())
        })();
        if let Err(error) = result
            && !error.contains("already running")
            && !error.contains("1056")
        {
            append_log(
                log,
                &format!("installer: StartServiceW(AcpiBridge): {error}"),
            );
        }
    }

    fn append_output(log: &Path, label: &str, output: &Output) {
        append_log(
            log,
            &format!(
                "{label}: exit={:?} stdout={} stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        );
    }

    fn log_path() -> std::path::PathBuf {
        std::env::var_os("ProgramData")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("NeoControl")
            .join("driver-install.log")
    }

    fn append_log(path: &Path, message: &str) {
        let Some(parent) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write as _;
            let _ = writeln!(file, "{message}");
        }
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_payload_requires_all_three_files() {
        let root =
            std::env::temp_dir().join(format!("neo-control-driver-test-{}", std::process::id()));
        let directory = root.join("acpibridge");
        std::fs::create_dir_all(&directory).expect("create test directory");
        std::fs::write(directory.join("AcpiBridge.inf"), b"inf").expect("write inf");
        std::fs::write(directory.join("AcpiBridge.sys"), b"sys").expect("write sys");
        std::fs::write(directory.join("acpibridge.cat"), b"cat").expect("write cat");
        assert!(acpi_payload_at(&root).is_some());
        std::fs::remove_dir_all(&root).expect("clean test directory");
    }

    #[test]
    fn required_health_missing_without_core_driver() {
        let health = DriverHealth {
            core_ready: false,
            core_installed: true,
            ..DriverHealth::default()
        };
        assert!(health.required_missing());
    }
}
