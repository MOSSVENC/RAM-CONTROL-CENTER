#[cfg(windows)]
use super::registry::{
    broadcast_setting_change, delete_registry_value, read_registry_dword, read_registry_string,
    wide, write_registry_dword, write_registry_string,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayCareState {
    pub taskbar_auto_hide: bool,
    pub taskbar_transparency: bool,
    pub dark_mode: bool,
    pub monitor_ac_minutes: u32,
    pub monitor_dc_minutes: u32,
    pub screen_saver_enabled: bool,
    pub screen_saver_type: u8,
    pub screen_saver_minutes: u32,
}

#[cfg(windows)]
pub fn display_care_state() -> Result<DisplayCareState, String> {
    let (monitor_ac_minutes, monitor_dc_minutes) = read_monitor_timeouts()?;
    let screen_saver_enabled = read_screen_saver_active()?;
    let screen_saver_type = read_screen_saver_type()?;
    let screen_saver_minutes = read_screen_saver_timeout_minutes()?;
    Ok(DisplayCareState {
        taskbar_auto_hide: read_taskbar_auto_hide()?,
        taskbar_transparency: read_registry_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "EnableTransparency",
        )?
        .unwrap_or(1)
            != 0,
        dark_mode: read_dark_mode()?,
        monitor_ac_minutes,
        monitor_dc_minutes,
        screen_saver_enabled,
        screen_saver_type,
        screen_saver_minutes,
    })
}

#[cfg(not(windows))]
pub fn display_care_state() -> Result<DisplayCareState, String> {
    Err("display care is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_taskbar_auto_hide(enabled: bool) -> Result<(), String> {
    let previous = read_taskbar_auto_hide()?;
    set_taskbar_auto_hide_raw(enabled)?;
    if read_taskbar_auto_hide()? == enabled {
        return Ok(());
    }
    let rollback = set_taskbar_auto_hide_raw(previous).map_or_else(
        |error| format!("could not restore taskbar auto-hide: {error}"),
        |()| "restored taskbar auto-hide".to_owned(),
    );
    Err(format!(
        "taskbar auto-hide readback expected {enabled}; {rollback}"
    ))
}

#[cfg(not(windows))]
pub fn set_taskbar_auto_hide(_enabled: bool) -> Result<(), String> {
    Err("taskbar auto-hide is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_taskbar_transparency(enabled: bool) -> Result<(), String> {
    let previous = read_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "EnableTransparency",
    )?
    .unwrap_or(1);
    write_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "EnableTransparency",
        u32::from(enabled),
    )?;
    broadcast_setting_change("ImmersiveColorSet");
    let actual = read_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "EnableTransparency",
    )?
    .unwrap_or(1)
        != 0;
    if actual == enabled {
        return Ok(());
    }
    let _ = write_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "EnableTransparency",
        previous,
    );
    broadcast_setting_change("ImmersiveColorSet");
    Err(format!(
        "taskbar transparency readback expected {enabled}, got {actual}"
    ))
}

#[cfg(not(windows))]
pub fn set_taskbar_transparency(_enabled: bool) -> Result<(), String> {
    Err("taskbar transparency is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_dark_mode(enabled: bool) -> Result<(), String> {
    let previous = read_dark_mode()?;
    set_dark_mode_raw(enabled)?;
    let actual = read_dark_mode()?;
    if actual == enabled {
        return Ok(());
    }
    let rollback = set_dark_mode_raw(previous).map_or_else(
        |error| format!("could not restore appearance mode: {error}"),
        |()| "restored appearance mode".to_owned(),
    );
    Err(format!(
        "appearance-mode readback expected {enabled}, got {actual}; {rollback}"
    ))
}

#[cfg(not(windows))]
pub fn set_dark_mode(_enabled: bool) -> Result<(), String> {
    Err("appearance mode is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_monitor_timeouts(ac_minutes: u32, dc_minutes: u32) -> Result<(), String> {
    if ac_minutes > 1_440 || dc_minutes > 1_440 {
        return Err("monitor timeout must be within 24 hours".to_owned());
    }
    let previous = read_monitor_timeouts()?;
    write_monitor_timeouts_raw(ac_minutes, dc_minutes)?;
    let (actual_ac, actual_dc) = read_monitor_timeouts()?;
    if actual_ac == ac_minutes && actual_dc == dc_minutes {
        return Ok(());
    }
    let rollback = write_monitor_timeouts_raw(previous.0, previous.1).map_or_else(
        |error| format!("could not restore monitor timeouts: {error}"),
        |()| "restored monitor timeouts".to_owned(),
    );
    Err(format!(
        "monitor timeout readback expected AC={ac_minutes} DC={dc_minutes}, got AC={actual_ac} DC={actual_dc}; {rollback}"
    ))
}

#[cfg(not(windows))]
pub fn set_monitor_timeouts(_ac_minutes: u32, _dc_minutes: u32) -> Result<(), String> {
    Err("monitor timeout is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_screen_saver(enabled: bool, screen_saver_type: u8, minutes: u32) -> Result<(), String> {
    if screen_saver_type > 6 {
        return Err("unknown screen-saver type".to_owned());
    }
    if enabled && screen_saver_type == 0 {
        return Err("choose a screen saver before enabling it".to_owned());
    }
    if minutes > 999 {
        return Err("screen-saver timeout must be at most 999 minutes".to_owned());
    }
    let previous = display_care_state()?;
    set_screen_saver_raw(enabled, screen_saver_type, minutes)?;
    let actual_enabled = read_screen_saver_active()?;
    let actual_type = read_screen_saver_type()?;
    let actual_minutes = read_screen_saver_timeout_minutes()?;
    if actual_enabled == enabled && actual_type == screen_saver_type && actual_minutes == minutes {
        return Ok(());
    }
    let rollback = set_screen_saver_raw(
        previous.screen_saver_enabled,
        previous.screen_saver_type,
        previous.screen_saver_minutes,
    )
    .map_or_else(
        |error| format!("could not restore screen saver: {error}"),
        |()| "restored screen saver".to_owned(),
    );
    Err(format!(
        "screen-saver readback expected enabled={enabled} type={screen_saver_type} minutes={minutes}, got enabled={actual_enabled} type={actual_type} minutes={actual_minutes}; {rollback}"
    ))
}

#[cfg(not(windows))]
pub fn set_screen_saver(
    _enabled: bool,
    _screen_saver_type: u8,
    _minutes: u32,
) -> Result<(), String> {
    Err("screen saver is only available on Windows".to_owned())
}

#[cfg(windows)]
fn read_taskbar_auto_hide() -> Result<bool, String> {
    use windows_sys::Win32::UI::Shell::{ABM_GETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage};

    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..APPBARDATA::default()
    };
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
    Ok(state as u32 & ABS_AUTOHIDE != 0)
}

#[cfg(windows)]
fn set_taskbar_auto_hide_raw(enabled: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{
        ABM_SETSTATE, ABS_ALWAYSONTOP, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

    let class = wide("Shell_TrayWnd");
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return Err("the Windows taskbar window was not found".to_owned());
    }
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        lParam: if enabled {
            (ABS_AUTOHIDE | ABS_ALWAYSONTOP) as isize
        } else {
            ABS_ALWAYSONTOP as isize
        },
        ..APPBARDATA::default()
    };
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut data);
    }
    Ok(())
}

#[cfg(windows)]
fn read_dark_mode() -> Result<bool, String> {
    Ok(read_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme",
    )?
    .unwrap_or(1)
        == 0)
}

#[cfg(windows)]
fn set_dark_mode_raw(enabled: bool) -> Result<(), String> {
    let light = u32::from(!enabled);
    write_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme",
        light,
    )?;
    write_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "SystemUsesLightTheme",
        light,
    )?;
    broadcast_setting_change("ImmersiveColorSet");
    Ok(())
}

#[cfg(windows)]
fn read_monitor_timeouts() -> Result<(u32, u32), String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::System::Power::{
        PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex,
    };
    use windows_sys::core::GUID;

    const VIDEO_SUBGROUP: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
    const VIDEO_POWERDOWN: GUID = GUID::from_u128(0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e);

    unsafe {
        let mut scheme_ptr: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(std::ptr::null_mut(), &mut scheme_ptr) != 0 || scheme_ptr.is_null()
        {
            return Err("could not read the active power scheme".to_owned());
        }
        let scheme = *scheme_ptr;
        let mut ac = 0_u32;
        let mut dc = 0_u32;
        let ac_result = PowerReadACValueIndex(
            std::ptr::null_mut(),
            &scheme,
            &VIDEO_SUBGROUP,
            &VIDEO_POWERDOWN,
            &mut ac,
        );
        let dc_result = PowerReadDCValueIndex(
            std::ptr::null_mut(),
            &scheme,
            &VIDEO_SUBGROUP,
            &VIDEO_POWERDOWN,
            &mut dc,
        );
        LocalFree(scheme_ptr.cast());
        if ac_result != 0 || dc_result != 0 {
            return Err("the active power scheme does not expose monitor timeout".to_owned());
        }
        Ok((ac / 60, dc / 60))
    }
}

#[cfg(windows)]
fn write_monitor_timeout_raw(ac: bool, minutes: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::System::Power::{
        PowerGetActiveScheme, PowerSetActiveScheme, PowerWriteACValueIndex, PowerWriteDCValueIndex,
    };
    use windows_sys::core::GUID;

    const VIDEO_SUBGROUP: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
    const VIDEO_POWERDOWN: GUID = GUID::from_u128(0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e);
    let seconds = minutes.saturating_mul(60);

    unsafe {
        let mut scheme_ptr: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(std::ptr::null_mut(), &mut scheme_ptr) != 0 || scheme_ptr.is_null()
        {
            return Err("could not read the active power scheme".to_owned());
        }
        let scheme = *scheme_ptr;
        let result = if ac {
            PowerWriteACValueIndex(
                std::ptr::null_mut(),
                &scheme,
                &VIDEO_SUBGROUP,
                &VIDEO_POWERDOWN,
                seconds,
            )
        } else {
            PowerWriteDCValueIndex(
                std::ptr::null_mut(),
                &scheme,
                &VIDEO_SUBGROUP,
                &VIDEO_POWERDOWN,
                seconds,
            )
        };
        if result == 0 {
            let _ = PowerSetActiveScheme(std::ptr::null_mut(), &scheme);
        }
        LocalFree(scheme_ptr.cast());
        if result == 0 {
            Ok(())
        } else {
            Err("could not update monitor timeout".to_owned())
        }
    }
}

#[cfg(windows)]
fn write_monitor_timeouts_raw(ac: u32, dc: u32) -> Result<(), String> {
    write_monitor_timeout_raw(true, ac)?;
    write_monitor_timeout_raw(false, dc)
}

#[cfg(windows)]
fn read_screen_saver_active() -> Result<bool, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETSCREENSAVEACTIVE, SystemParametersInfoW,
    };

    let mut active = 0_i32;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETSCREENSAVEACTIVE,
            0,
            (&mut active as *mut i32).cast(),
            0,
        )
    };
    if ok == 0 {
        return read_registry_string("Control Panel\\Desktop", "ScreenSaveActive")
            .map(|value| value.as_deref() == Some("1"));
    }
    Ok(active != 0)
}

#[cfg(windows)]
fn read_screen_saver_timeout_minutes() -> Result<u32, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETSCREENSAVETIMEOUT, SystemParametersInfoW,
    };

    let mut seconds = 0_u32;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETSCREENSAVETIMEOUT,
            0,
            (&mut seconds as *mut u32).cast(),
            0,
        )
    };
    if ok == 0 {
        seconds = read_registry_string("Control Panel\\Desktop", "ScreenSaveTimeOut")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
    }
    Ok(seconds / 60)
}

#[cfg(windows)]
fn read_screen_saver_type() -> Result<u8, String> {
    let Some(path) = read_registry_string("Control Panel\\Desktop", "SCRNSAVE.EXE")? else {
        return Ok(0);
    };
    Ok(screen_saver_type_from_path(&path))
}

#[cfg(windows)]
fn screen_saver_type_from_path(path: &str) -> u8 {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    match name.as_str() {
        "sstext3d.scr" => 1,
        "scrnsave.scr" => 2,
        "bubbles.scr" => 3,
        "mystify.scr" => 4,
        "photoscreensaver.scr" => 5,
        "ribbons.scr" => 6,
        _ => 0,
    }
}

#[cfg(windows)]
fn screen_saver_path(screen_saver_type: u8) -> Option<String> {
    let file = match screen_saver_type {
        1 => "ssText3d.scr",
        2 => "scrnsave.scr",
        3 => "Bubbles.scr",
        4 => "Mystify.scr",
        5 => "PhotoScreensaver.scr",
        6 => "Ribbons.scr",
        _ => return None,
    };
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Some(
        std::path::PathBuf::from(root)
            .join("System32")
            .join(file)
            .to_string_lossy()
            .to_string(),
    )
}

#[cfg(windows)]
fn set_screen_saver_raw(enabled: bool, screen_saver_type: u8, minutes: u32) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_SETSCREENSAVEACTIVE, SPI_SETSCREENSAVETIMEOUT, SPIF_SENDWININICHANGE,
        SystemParametersInfoW,
    };

    write_registry_string(
        "Control Panel\\Desktop",
        "ScreenSaveActive",
        if enabled { "1" } else { "0" },
    )?;
    match screen_saver_path(screen_saver_type) {
        Some(path) => write_registry_string("Control Panel\\Desktop", "SCRNSAVE.EXE", &path)?,
        None => delete_registry_value("Control Panel\\Desktop", "SCRNSAVE.EXE")?,
    }
    let seconds = minutes.saturating_mul(60);
    write_registry_string(
        "Control Panel\\Desktop",
        "ScreenSaveTimeOut",
        &seconds.to_string(),
    )?;
    let active_ok = unsafe {
        SystemParametersInfoW(
            SPI_SETSCREENSAVEACTIVE,
            u32::from(enabled),
            std::ptr::null_mut(),
            SPIF_SENDWININICHANGE,
        )
    };
    let timeout_ok = unsafe {
        SystemParametersInfoW(
            SPI_SETSCREENSAVETIMEOUT,
            seconds,
            std::ptr::null_mut(),
            SPIF_SENDWININICHANGE,
        )
    };
    if active_ok == 0 || timeout_ok == 0 {
        return Err("Windows rejected the screen-saver update".to_owned());
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::screen_saver_type_from_path;

    #[test]
    fn maps_vendor_screen_saver_types() {
        assert_eq!(
            screen_saver_type_from_path(r"C:\Windows\System32\ssText3d.scr"),
            1
        );
        assert_eq!(
            screen_saver_type_from_path(r"C:\Windows\System32\scrnsave.scr"),
            2
        );
        assert_eq!(
            screen_saver_type_from_path(r"C:\Windows\System32\Bubbles.scr"),
            3
        );
        assert_eq!(
            screen_saver_type_from_path(r"C:\Windows\System32\Mystify.scr"),
            4
        );
        assert_eq!(
            screen_saver_type_from_path(r"C:\Windows\System32\PhotoScreensaver.scr"),
            5
        );
        assert_eq!(
            screen_saver_type_from_path(r"C:\Windows\System32\Ribbons.scr"),
            6
        );
        assert_eq!(screen_saver_type_from_path(r"C:\Other\saver.scr"), 0);
    }
}
