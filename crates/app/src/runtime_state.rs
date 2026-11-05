#[cfg(windows)]
use std::time::{Duration, SystemTime};

use neo_control_protocol::PowerMode;

pub(crate) fn persist_power_mode(mode: PowerMode) {
    let root = std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("NeoControl");
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    let path = root.join("boot-profile.ini");
    let mut lines = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut found_power = false;
    let mut found_fan = false;
    for line in &mut lines {
        let key = line.split('=').next().unwrap_or("").trim();
        if key == "power" {
            *line = format!("power={}", mode.as_u8());
            found_power = true;
        } else if key == "fan" {
            found_fan = true;
        }
    }
    if !found_power {
        lines.push(format!("power={}", mode.as_u8()));
    }
    if !found_fan {
        lines.push("fan=0".to_owned());
    }
    let text = format!("{}\n", lines.join("\n"));
    let _ = std::fs::write(path, text);
}

#[cfg(windows)]
pub(crate) fn flexi_recovery_ready() -> bool {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    let root = std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("NeoControl");
    let marker = root.join("flexi-recovery.marker");
    let now = SystemTime::now();
    let marker_time = match std::fs::metadata(&marker).and_then(|meta| meta.modified()) {
        Ok(time) => time,
        Err(_) => {
            let _ = std::fs::create_dir_all(&root);
            let _ = std::fs::write(&marker, b"");
            return false;
        }
    };
    let uptime_ms = unsafe { GetTickCount64() };
    let boot_time = now
        .checked_sub(Duration::from_millis(uptime_ms))
        .unwrap_or(now);
    boot_time
        .duration_since(marker_time)
        .map(|elapsed| elapsed > Duration::from_secs(5))
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub(crate) fn flexi_recovery_ready() -> bool {
    false
}
