use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use neo_control_protocol::{Dchu, FanMode, PowerMode};

#[derive(Clone, Copy, Debug)]
struct BootProfile {
    power: PowerMode,
    fan: FanMode,
}

impl Default for BootProfile {
    fn default() -> Self {
        Self {
            power: PowerMode::Performance,
            fan: FanMode::Auto,
        }
    }
}

/// Headless boot path used by the single `neo-control.exe` binary.
///
/// It never constructs the Slint window. The signed AcpiBridge device is
/// retried until its settings channel is ready, the full persisted power
/// transition is reapplied, and a non-Auto fan policy is applied only after
/// power succeeds. The process exits immediately after readback succeeds.
pub fn run() -> i32 {
    let started = Instant::now();
    let config_path = config_path();
    let mut profile = read_profile(&config_path).unwrap_or_default();
    let mut wait_ms = 45_000_u64;

    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--power" => {
                let Some(value) = args.next() else {
                    log_line("missing value for --power");
                    return 1;
                };
                let Ok(raw) = value.parse::<u8>() else {
                    log_line(&format!("invalid --power value {value:?}"));
                    return 1;
                };
                let Some(power) = PowerMode::from_u8(raw) else {
                    log_line(&format!("unsupported --power value {raw}"));
                    return 1;
                };
                profile.power = power;
            }
            "--fan" => {
                let Some(value) = args.next() else {
                    log_line("missing value for --fan");
                    return 1;
                };
                let Ok(raw) = value.parse::<u8>() else {
                    log_line(&format!("invalid --fan value {value:?}"));
                    return 1;
                };
                let Some(fan) = FanMode::from_u8(raw) else {
                    log_line(&format!("unsupported --fan value {raw}"));
                    return 1;
                };
                profile.fan = fan;
            }
            "--wait-ms" => {
                let Some(value) = args.next() else {
                    log_line("missing value for --wait-ms");
                    return 1;
                };
                let Ok(value) = value.parse::<u64>() else {
                    log_line(&format!("invalid --wait-ms value {value:?}"));
                    return 1;
                };
                wait_ms = value.min(180_000);
            }
            "--config" => {
                let Some(path) = args.next() else {
                    log_line("missing value for --config");
                    return 1;
                };
                profile = read_profile(Path::new(&path)).unwrap_or(profile);
            }
            _ => log_line(&format!("ignoring unknown boot-agent argument {arg:?}")),
        }
    }

    log_line(&format!(
        "agent start: target power={} fan={} wait_ms={wait_ms}",
        profile.power.as_u8(),
        profile.fan.as_u8()
    ));

    let deadline = started + Duration::from_millis(wait_ms);
    let mut last_error = None;
    let mut attempt = 0_u32;
    let mut service_checked = false;
    // `Instant` is boot-relative on Windows. Avoid subtracting five seconds
    // when the service starts very early in boot; use the start marker instead.
    let mut last_progress_log = started;

    while Instant::now() <= deadline {
        attempt += 1;

        // Do not read the whole platform state here. The boot path only needs
        // the settings channel and the power command; that keeps the cold-boot
        // critical path short and avoids the expensive thermal/fan reads.
        let opened = match Dchu::open() {
            Ok(dchu) => Ok(dchu),
            Err(first_error) => {
                if !service_checked {
                    service_checked = true;
                    match ensure_acpi_bridge_service() {
                        Ok(()) => log_line("acpi bridge service checked/started"),
                        Err(error) => {
                            log_line(&format!("could not start acpi bridge service: {error}"))
                        }
                    }
                }
                Dchu::open().map_err(|second_error| format!("{first_error}; retry: {second_error}"))
            }
        };

        match opened {
            Ok(dchu) => {
                let power_ok = match dchu.set_power_mode(profile.power) {
                    Ok(()) => {
                        log_line(&format!(
                            "power reapplied and read back: {}",
                            profile.power.label()
                        ));
                        true
                    }
                    Err(error) => {
                        last_error = Some(format!("power write failed: {error}"));
                        if !service_checked {
                            service_checked = true;
                            match ensure_acpi_bridge_service() {
                                Ok(()) => log_line("acpi bridge service checked/started"),
                                Err(error) => log_line(&format!(
                                    "could not start acpi bridge service: {error}"
                                )),
                            }
                        }
                        false
                    }
                };

                // Auto is already the firmware fallback and does not need a
                // boot-time write. Non-auto policies are applied only after the
                // power command has succeeded.
                let fan_ok = if !power_ok || profile.fan == FanMode::Auto {
                    true
                } else {
                    match dchu.set_fan_mode(profile.fan) {
                        Ok(()) => {
                            log_line(&format!(
                                "fan applied and read back: {}",
                                profile.fan.label()
                            ));
                            true
                        }
                        Err(error) => {
                            last_error = Some(format!("fan write failed: {error}"));
                            false
                        }
                    }
                };

                if power_ok && fan_ok {
                    log_line(&format!(
                        "agent complete in {} ms ({} attempts)",
                        started.elapsed().as_millis(),
                        attempt
                    ));
                    return 0;
                }
                if last_progress_log.elapsed() >= Duration::from_secs(5) {
                    log_line(&format!(
                        "still waiting after {} ms (attempt {}): {}",
                        started.elapsed().as_millis(),
                        attempt,
                        last_error.as_deref().unwrap_or("unknown error")
                    ));
                    last_progress_log = Instant::now();
                }
            }
            Err(error) => {
                last_error = Some(error);
                if !service_checked {
                    service_checked = true;
                    match ensure_acpi_bridge_service() {
                        Ok(()) => log_line("acpi bridge service checked/started"),
                        Err(error) => {
                            log_line(&format!("could not start acpi bridge service: {error}"))
                        }
                    }
                }
                if last_progress_log.elapsed() >= Duration::from_secs(5) {
                    log_line(&format!(
                        "still waiting after {} ms (attempt {}): {}",
                        started.elapsed().as_millis(),
                        attempt,
                        last_error.as_deref().unwrap_or("unknown error")
                    ));
                    last_progress_log = Instant::now();
                }
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    let reason = last_error.unwrap_or_else(|| "DCHU did not become ready".to_owned());
    log_line(&format!(
        "agent failed after {} ms ({} attempts): {reason}",
        started.elapsed().as_millis(),
        attempt
    ));
    1
}

#[cfg(windows)]
fn ensure_acpi_bridge_service() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_START,
        StartServiceW,
    };

    const ERROR_SERVICE_ALREADY_RUNNING: u32 = 1056;

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let manager_name = wide("AcpiBridge");
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(format!(
            "OpenSCManagerW failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    let service = unsafe { OpenServiceW(manager, manager_name.as_ptr(), SERVICE_START) };
    if service.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe { CloseServiceHandle(manager) };
        return Err(format!("OpenServiceW(AcpiBridge) failed: {error}"));
    }

    let started = unsafe { StartServiceW(service, 0, std::ptr::null()) };
    let code = unsafe { GetLastError() };
    unsafe {
        CloseServiceHandle(service);
        CloseServiceHandle(manager);
    }
    if started != 0 || code == ERROR_SERVICE_ALREADY_RUNNING {
        return Ok(());
    }
    Err(format!(
        "StartServiceW(AcpiBridge) failed: {} (code {code})",
        std::io::Error::from_raw_os_error(code as i32)
    ))
}

#[cfg(not(windows))]
fn ensure_acpi_bridge_service() -> Result<(), String> {
    Ok(())
}

fn config_path() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("NeoControl")
        .join("boot-profile.ini")
}

fn read_profile(path: &Path) -> Option<BootProfile> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut result = BootProfile::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let raw = value.trim().parse::<u8>().ok();
        match key.trim() {
            "power" => result.power = raw.and_then(PowerMode::from_u8).unwrap_or(result.power),
            "fan" => result.fan = raw.and_then(FanMode::from_u8).unwrap_or(result.fan),
            _ => {}
        }
    }
    Some(result)
}

fn log_line(message: &str) {
    let root = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("NeoControl");
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("boot-agent.log"))
    else {
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    let _ = writeln!(file, "{stamp} {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_defaults_are_conservative() {
        let profile = BootProfile::default();
        assert_eq!(profile.power, PowerMode::Performance);
        assert_eq!(profile.fan, FanMode::Auto);
    }
}
