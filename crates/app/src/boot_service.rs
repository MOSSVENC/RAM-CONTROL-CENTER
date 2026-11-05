//! Native Windows service mode for the early boot policy replay.
//!
//! The interactive UI and the boot worker are the same executable. Windows
//! starts this mode headless before the user shell appears, the worker applies
//! the persisted power policy through the signed AcpiBridge transport, and the
//! process exits after the readback completes.

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::ERROR_SERVICE_SPECIFIC_ERROR;
    use windows_sys::Win32::Foundation::{
        ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_EXISTS,
    };
    use windows_sys::Win32::Storage::FileSystem::DELETE;
    use windows_sys::Win32::System::Services::{
        ChangeServiceConfig2W, ChangeServiceConfigW, ControlService, CreateServiceW, DeleteService,
        RegisterServiceCtrlHandlerExW, SC_ACTION, SC_ACTION_RESTART, SC_MANAGER_CONNECT,
        SC_MANAGER_CREATE_SERVICE, SERVICE_ALL_ACCESS, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
        SERVICE_CONFIG_DESCRIPTION, SERVICE_CONFIG_FAILURE_ACTIONS,
        SERVICE_CONFIG_FAILURE_ACTIONS_FLAG, SERVICE_CONTROL_STOP, SERVICE_DESCRIPTIONW,
        SERVICE_ERROR_NORMAL, SERVICE_FAILURE_ACTIONS_FLAG, SERVICE_FAILURE_ACTIONSW,
        SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_START_PENDING,
        SERVICE_STATUS, SERVICE_STATUS_HANDLE, SERVICE_STOP, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW,
        SERVICE_WIN32_OWN_PROCESS, SetServiceStatus, StartServiceCtrlDispatcherW, StartServiceW,
    };

    use crate::boot_agent;
    use crate::service_handle::{ScHandle, error_code};

    const SERVICE_NAME: &str = "NeoControlBoot";
    const SERVICE_DISPLAY_NAME: &str = "Neo Control Boot Agent";
    const SERVICE_DESCRIPTION: &str =
        "Applies the persisted Neo Control power policy before the user shell starts.";
    const SERVICE_DEPENDENCY: &str = "AcpiBridge";
    const SERVICE_WAIT_MS: u32 = 120_000;

    pub fn run_dispatcher() -> i32 {
        let mut name = wide(SERVICE_NAME);
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: name.as_mut_ptr(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: ptr::null_mut(),
                lpServiceProc: None,
            },
        ];

        let started = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) };
        if started == 0 {
            let code = error_code();
            eprintln!(
                "could not connect to the Windows service dispatcher: {} (code {code})",
                std::io::Error::from_raw_os_error(code as i32)
            );
            return 1;
        }
        0
    }

    unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
        let name = wide(SERVICE_NAME);
        let status_handle = unsafe {
            RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(service_handler), ptr::null())
        };
        if status_handle.is_null() {
            return;
        }

        set_status(status_handle, SERVICE_START_PENDING, 0, 0, 15_000);
        set_status(status_handle, SERVICE_RUNNING, 0, 0, 0);

        let result = std::panic::catch_unwind(boot_agent::run).unwrap_or(1);
        if result == 0 {
            set_status(status_handle, SERVICE_STOPPED, 0, 0, 0);
        } else {
            set_status(
                status_handle,
                SERVICE_STOPPED,
                ERROR_SERVICE_SPECIFIC_ERROR,
                result as u32,
                0,
            );
        }
    }

    unsafe extern "system" fn service_handler(
        _control: u32,
        _event_type: u32,
        _event_data: *mut std::ffi::c_void,
        _context: *mut std::ffi::c_void,
    ) -> u32 {
        0
    }

    fn set_status(
        handle: SERVICE_STATUS_HANDLE,
        state: u32,
        win32_exit_code: u32,
        service_exit_code: u32,
        wait_hint: u32,
    ) {
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: 0,
            dwWin32ExitCode: win32_exit_code,
            dwServiceSpecificExitCode: service_exit_code,
            dwCheckPoint: u32::from(state == SERVICE_START_PENDING),
            dwWaitHint: wait_hint,
        };
        unsafe {
            SetServiceStatus(handle, &status);
        }
    }

    pub fn install() -> i32 {
        match install_inner() {
            Ok(path) => {
                println!("installed and started {SERVICE_DISPLAY_NAME}: {path}");
                0
            }
            Err(error) => {
                eprintln!("could not install {SERVICE_DISPLAY_NAME}: {error}");
                1
            }
        }
    }

    fn install_inner() -> Result<String, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not resolve the current executable: {error}"))?;
        let command_line = service_command_line(&executable)?;
        let service_name = wide(SERVICE_NAME);
        let display_name = wide(SERVICE_DISPLAY_NAME);
        let mut description = wide(SERVICE_DESCRIPTION);
        let dependency = wide_multi(&[SERVICE_DEPENDENCY]);
        let command_line_wide = wide(&command_line);

        let manager = ScHandle::open_manager(SC_MANAGER_CREATE_SERVICE | SC_MANAGER_CONNECT)?;
        let created = unsafe {
            CreateServiceW(
                manager.raw(),
                service_name.as_ptr(),
                display_name.as_ptr(),
                SERVICE_ALL_ACCESS,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                command_line_wide.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                dependency.as_ptr(),
                ptr::null(),
                ptr::null(),
            )
        };
        let service = if let Some(service) = ScHandle::owned(created) {
            service
        } else {
            let code = error_code();
            if code != ERROR_SERVICE_EXISTS {
                return Err(format!(
                    "CreateServiceW failed: {} (code {code})",
                    std::io::Error::from_raw_os_error(code as i32)
                ));
            }
            let service = manager
                .open_service(
                    &service_name,
                    SERVICE_CHANGE_CONFIG | SERVICE_START | SERVICE_QUERY_STATUS,
                )
                .map_err(|code| {
                    format!(
                        "OpenServiceW failed: {} (code {code})",
                        std::io::Error::from_raw_os_error(code as i32)
                    )
                })?;
            let changed = unsafe {
                ChangeServiceConfigW(
                    service.raw(),
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    command_line_wide.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                    dependency.as_ptr(),
                    ptr::null(),
                    ptr::null(),
                    display_name.as_ptr(),
                )
            };
            if changed == 0 {
                return Err(last_error("ChangeServiceConfigW"));
            }
            service
        };

        let service_description = SERVICE_DESCRIPTIONW {
            lpDescription: description.as_mut_ptr(),
        };
        unsafe {
            ChangeServiceConfig2W(
                service.raw(),
                SERVICE_CONFIG_DESCRIPTION,
                (&service_description as *const SERVICE_DESCRIPTIONW).cast(),
            );
        }

        configure_failure_actions(&service)?;

        let started = unsafe { StartServiceW(service.raw(), 0, ptr::null()) };
        let code = error_code();
        if started == 0 && code != ERROR_SERVICE_ALREADY_RUNNING {
            return Err(format!(
                "StartServiceW failed: {} (code {code})",
                std::io::Error::from_raw_os_error(code as i32)
            ));
        }

        Ok(command_line)
    }

    fn configure_failure_actions(service: &ScHandle) -> Result<(), String> {
        let mut actions = [
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 1_000,
            },
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 2_000,
            },
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 5_000,
            },
        ];
        let failure_actions = SERVICE_FAILURE_ACTIONSW {
            dwResetPeriod: 86_400,
            lpRebootMsg: ptr::null_mut(),
            lpCommand: ptr::null_mut(),
            cActions: actions.len() as u32,
            lpsaActions: actions.as_mut_ptr(),
        };
        let configured = unsafe {
            ChangeServiceConfig2W(
                service.raw(),
                SERVICE_CONFIG_FAILURE_ACTIONS,
                (&failure_actions as *const SERVICE_FAILURE_ACTIONSW).cast(),
            )
        };
        if configured == 0 {
            return Err(last_error("ChangeServiceConfig2W(failure actions)"));
        }

        let failure_flag = SERVICE_FAILURE_ACTIONS_FLAG {
            fFailureActionsOnNonCrashFailures: 1,
        };
        let configured = unsafe {
            ChangeServiceConfig2W(
                service.raw(),
                SERVICE_CONFIG_FAILURE_ACTIONS_FLAG,
                (&failure_flag as *const SERVICE_FAILURE_ACTIONS_FLAG).cast(),
            )
        };
        if configured == 0 {
            return Err(last_error("ChangeServiceConfig2W(non-crash failures)"));
        }
        Ok(())
    }

    pub fn uninstall() -> i32 {
        match uninstall_inner() {
            Ok(()) => {
                println!("removed {SERVICE_DISPLAY_NAME}");
                0
            }
            Err(error) => {
                eprintln!("could not remove {SERVICE_DISPLAY_NAME}: {error}");
                1
            }
        }
    }

    fn uninstall_inner() -> Result<(), String> {
        let service_name = wide(SERVICE_NAME);
        let manager = ScHandle::open_manager(SC_MANAGER_CONNECT)?;
        let service = match manager
            .open_service(&service_name, DELETE | SERVICE_STOP | SERVICE_QUERY_STATUS)
        {
            Ok(service) => service,
            Err(_) => {
                let code = error_code();
                if code == ERROR_SERVICE_DOES_NOT_EXIST {
                    return Ok(());
                }
                return Err(format!(
                    "OpenServiceW failed: {} (code {code})",
                    std::io::Error::from_raw_os_error(code as i32)
                ));
            }
        };

        let mut status = SERVICE_STATUS::default();
        unsafe {
            ControlService(service.raw(), SERVICE_CONTROL_STOP, &mut status);
            std::thread::sleep(Duration::from_millis(100));
        }

        let deleted = unsafe { DeleteService(service.raw()) };
        let code = error_code();
        if deleted == 0 {
            return Err(format!(
                "DeleteService failed: {} (code {code})",
                std::io::Error::from_raw_os_error(code as i32)
            ));
        }
        Ok(())
    }

    fn service_command_line(executable: &Path) -> Result<String, String> {
        let path = executable
            .to_str()
            .ok_or_else(|| "the executable path is not valid UTF-16".to_owned())?;
        if path.contains('"') {
            return Err("the executable path contains a quote".to_owned());
        }
        Ok(format!(
            "\"{path}\" --boot-service --wait-ms {SERVICE_WAIT_MS}"
        ))
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain([0]).collect()
    }

    fn wide_multi(values: &[&str]) -> Vec<u16> {
        let mut output = Vec::new();
        for value in values {
            output.extend(OsStr::new(value).encode_wide());
            output.push(0);
        }
        output.push(0);
        output
    }

    fn last_error(operation: &str) -> String {
        crate::service_handle::last_error(operation)
    }
}

#[cfg(windows)]
pub use platform::{install, run_dispatcher, uninstall};

#[cfg(not(windows))]
mod platform {
    pub fn run_dispatcher() -> i32 {
        eprintln!("the Windows boot service is only available on Windows");
        1
    }

    pub fn install() -> i32 {
        eprintln!("the Windows boot service is only available on Windows");
        1
    }

    pub fn uninstall() -> i32 {
        eprintln!("the Windows boot service is only available on Windows");
        1
    }
}

#[cfg(not(windows))]
pub use platform::{install, run_dispatcher, uninstall};
