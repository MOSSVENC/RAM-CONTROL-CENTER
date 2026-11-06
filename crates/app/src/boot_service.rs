//! Native Windows service mode for the early boot policy replay.
//!
//! The interactive UI and the boot worker are the same executable. Windows
//! starts this mode headless before the user shell appears, the worker applies
//! the persisted power policy through the signed AcpiBridge transport, and the
//! process exits after the readback completes.

#[cfg(windows)]
mod platform {
    use std::ffi::{OsStr, OsString};
    use std::time::Duration;

    use windows_service::define_windows_service;
    use windows_service::service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControlAccept, ServiceDependency,
        ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_sys::Win32::Foundation::{
        ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_EXISTS,
    };

    use crate::boot_agent;

    const SERVICE_NAME: &str = "NeoControlBoot";
    const SERVICE_DISPLAY_NAME: &str = "Neo Control Boot Agent";
    const SERVICE_DESCRIPTION: &str =
        "Applies the persisted Neo Control power policy before the user shell starts.";
    const SERVICE_DEPENDENCY: &str = "AcpiBridge";
    const SERVICE_WAIT_MS: u32 = 120_000;

    define_windows_service!(ffi_service_main, service_main);

    pub fn run_dispatcher() -> i32 {
        match service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!(
                    "could not connect to the Windows service dispatcher: {}",
                    describe("StartServiceCtrlDispatcherW", error)
                );
                1
            }
        }
    }

    fn service_main(_arguments: Vec<OsString>) {
        let status_handle = match service_control_handler::register(SERVICE_NAME, |_| {
            ServiceControlHandlerResult::NoError
        }) {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!(
                    "could not register the Windows service control handler: {}",
                    describe("RegisterServiceCtrlHandlerExW", error)
                );
                return;
            }
        };

        set_status(
            &status_handle,
            ServiceState::StartPending,
            ServiceExitCode::NO_ERROR,
        );
        set_status(
            &status_handle,
            ServiceState::Running,
            ServiceExitCode::NO_ERROR,
        );

        let result = std::panic::catch_unwind(boot_agent::run).unwrap_or(1);
        if result == 0 {
            set_status(
                &status_handle,
                ServiceState::Stopped,
                ServiceExitCode::NO_ERROR,
            );
        } else {
            set_status(
                &status_handle,
                ServiceState::Stopped,
                ServiceExitCode::ServiceSpecific(result as u32),
            );
        }
    }

    fn set_status(
        status_handle: &service_control_handler::ServiceStatusHandle,
        state: ServiceState,
        exit_code: ServiceExitCode,
    ) {
        let pending = state == ServiceState::StartPending;
        let status = ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code,
            checkpoint: u32::from(pending),
            wait_hint: if pending {
                Duration::from_secs(15)
            } else {
                Duration::ZERO
            },
            process_id: None,
        };
        if let Err(error) = status_handle.set_service_status(status) {
            eprintln!(
                "could not update the Windows service status: {}",
                describe("SetServiceStatus", error)
            );
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
        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY_NAME),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: executable.clone(),
            launch_arguments: vec![
                OsString::from("--boot-service"),
                OsString::from("--wait-ms"),
                OsString::from(SERVICE_WAIT_MS.to_string()),
            ],
            dependencies: vec![ServiceDependency::Service(OsString::from(
                SERVICE_DEPENDENCY,
            ))],
            account_name: None,
            account_password: None,
        };

        let manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )
        .map_err(|error| describe("OpenSCManagerW", error))?;

        let service_access =
            ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS;
        let service = match manager.create_service(&service_info, service_access) {
            Ok(service) => service,
            Err(error) if error_code(&error) == Some(ERROR_SERVICE_EXISTS as i32) => {
                let service = manager
                    .open_service(SERVICE_NAME, service_access)
                    .map_err(|error| describe("OpenServiceW(NeoControlBoot)", error))?;
                service
                    .change_config(&service_info)
                    .map_err(|error| describe("ChangeServiceConfigW", error))?;
                service
            }
            Err(error) => return Err(describe("CreateServiceW", error)),
        };

        service
            .set_description(SERVICE_DESCRIPTION)
            .map_err(|error| describe("ChangeServiceConfig2W(description)", error))?;
        configure_failure_actions(&service)?;

        match service.start::<&OsStr>(&[]) {
            Ok(()) => {}
            Err(error) if error_code(&error) == Some(ERROR_SERVICE_ALREADY_RUNNING as i32) => {}
            Err(error) => return Err(describe("StartServiceW", error)),
        }

        Ok(format!(
            "{} --boot-service --wait-ms {SERVICE_WAIT_MS}",
            executable.display()
        ))
    }

    fn configure_failure_actions(
        service: &windows_service::service::Service,
    ) -> Result<(), String> {
        service
            .update_failure_actions(ServiceFailureActions {
                reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86_400)),
                reboot_msg: None,
                command: None,
                actions: Some(vec![
                    ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(1),
                    },
                    ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(2),
                    },
                    ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(5),
                    },
                ]),
            })
            .map_err(|error| describe("ChangeServiceConfig2W(failure actions)", error))?;
        service
            .set_failure_actions_on_non_crash_failures(true)
            .map_err(|error| describe("ChangeServiceConfig2W(non-crash failures)", error))
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
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|error| describe("OpenSCManagerW", error))?;
        let service = match manager.open_service(
            SERVICE_NAME,
            ServiceAccess::DELETE | ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
        ) {
            Ok(service) => service,
            Err(error) if error_code(&error) == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => {
                return Ok(());
            }
            Err(error) => return Err(describe("OpenServiceW(NeoControlBoot)", error)),
        };

        if service
            .query_status()
            .map(|status| status.current_state != ServiceState::Stopped)
            .unwrap_or(true)
        {
            let _ = service.stop();
            std::thread::sleep(Duration::from_millis(100));
        }

        service
            .delete()
            .map_err(|error| describe("DeleteService", error))
    }

    fn error_code(error: &windows_service::Error) -> Option<i32> {
        match error {
            windows_service::Error::Winapi(error) => error.raw_os_error(),
            _ => None,
        }
    }

    fn describe(operation: &str, error: windows_service::Error) -> String {
        match error {
            windows_service::Error::Winapi(error) => {
                let code = error.raw_os_error().unwrap_or_default();
                format!("{operation} failed: {error} (code {code})")
            }
            error => format!("{operation} failed: {error}"),
        }
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
