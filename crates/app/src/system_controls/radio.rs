#[cfg(windows)]
pub fn set_airplane_mode(enabled: bool) -> Result<(), String> {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

    let access = Radio::RequestAccessAsync()
        .and_then(|operation| operation.join())
        .map_err(|error| error.to_string())?;
    if access != RadioAccessStatus::Allowed {
        return Err("Windows denied radio-control access".to_owned());
    }
    let radios = Radio::GetRadiosAsync()
        .and_then(|operation| operation.join())
        .map_err(|error| error.to_string())?;
    let count = radios.Size().map_err(|error| error.to_string())?;
    let target = if enabled {
        RadioState::Off
    } else {
        RadioState::On
    };
    let mut changed = 0_u32;
    for index in 0..count {
        let radio = radios.GetAt(index).map_err(|error| error.to_string())?;
        let kind = radio.Kind().map_err(|error| error.to_string())?;
        if !matches!(
            kind,
            RadioKind::WiFi | RadioKind::MobileBroadband | RadioKind::Bluetooth
        ) {
            continue;
        }
        let state = radio.State().map_err(|error| error.to_string())?;
        if state == RadioState::Disabled || state == RadioState::Unknown {
            continue;
        }
        let status = radio
            .SetStateAsync(target)
            .and_then(|operation| operation.join())
            .map_err(|error| error.to_string())?;
        if status == RadioAccessStatus::Allowed {
            changed += 1;
        }
    }
    if changed == 0 {
        Err("no controllable radios were reported by Windows".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn set_airplane_mode(_enabled: bool) -> Result<(), String> {
    Err("airplane mode is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn airplane_mode_enabled() -> Result<bool, String> {
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};

    let radios = Radio::GetRadiosAsync()
        .and_then(|operation| operation.join())
        .map_err(|error| error.to_string())?;
    let count = radios.Size().map_err(|error| error.to_string())?;
    let mut saw_network_radio = false;
    let mut any_on = false;
    for index in 0..count {
        let radio = radios.GetAt(index).map_err(|error| error.to_string())?;
        let kind = radio.Kind().map_err(|error| error.to_string())?;
        if !matches!(kind, RadioKind::WiFi | RadioKind::MobileBroadband) {
            continue;
        }
        saw_network_radio = true;
        if radio.State().map_err(|error| error.to_string())? == RadioState::On {
            any_on = true;
        }
    }
    if saw_network_radio {
        Ok(!any_on)
    } else {
        Err("Windows did not report a network radio".to_owned())
    }
}

#[cfg(not(windows))]
pub fn airplane_mode_enabled() -> Result<bool, String> {
    Err("airplane mode is only available on Windows".to_owned())
}
