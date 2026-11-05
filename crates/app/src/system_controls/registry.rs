#[cfg(windows)]
pub(super) fn read_registry_dword(subkey: &str, value: &str) -> Result<Option<u32>, String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

    let subkey = wide(subkey);
    let value = wide(value);
    let mut data = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    match result {
        ERROR_SUCCESS => Ok(Some(data)),
        ERROR_FILE_NOT_FOUND => Ok(None),
        code => Err(format!("RegGetValueW failed with code {code}")),
    }
}

#[cfg(windows)]
pub(super) fn read_registry_string(subkey: &str, value: &str) -> Result<Option<String>, String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

    let subkey = wide(subkey);
    let value = wide(value);
    let mut buffer = [0_u16; 512];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    match result {
        ERROR_SUCCESS => {
            let length = buffer
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(buffer.len());
            Ok(Some(String::from_utf16_lossy(&buffer[..length])))
        }
        ERROR_FILE_NOT_FOUND => Ok(None),
        code => Err(format!("RegGetValueW(string) failed with code {code}")),
    }
}

#[cfg(windows)]
pub(super) fn write_registry_dword(subkey: &str, value: &str, data: u32) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        HKEY, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, RegCloseKey, RegCreateKeyExW,
        RegSetValueExW,
    };

    let subkey = wide(subkey);
    let value = wide(value);
    let mut key: HKEY = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(format!("RegCreateKeyExW failed with code {result}"));
    }
    let result = unsafe {
        RegSetValueExW(
            key,
            value.as_ptr(),
            0,
            REG_DWORD,
            (&data as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    unsafe {
        RegCloseKey(key);
    }
    if result == 0 {
        Ok(())
    } else {
        Err(format!("RegSetValueExW failed with code {result}"))
    }
}

#[cfg(windows)]
pub(super) fn write_registry_string(subkey: &str, value: &str, data: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        HKEY, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegSetValueExW,
    };

    let subkey = wide(subkey);
    let value = wide(value);
    let data = wide(data);
    let mut key: HKEY = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(format!("RegCreateKeyExW failed with code {result}"));
    }
    let result = unsafe {
        RegSetValueExW(
            key,
            value.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    unsafe {
        RegCloseKey(key);
    }
    if result == 0 {
        Ok(())
    } else {
        Err(format!("RegSetValueExW failed with code {result}"))
    }
}

#[cfg(windows)]
pub(super) fn delete_registry_value(subkey: &str, value: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        HKEY, KEY_SET_VALUE, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    };

    let subkey = wide(subkey);
    let value = wide(value);
    let mut key: HKEY = std::ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if result != 0 {
        return Err(format!("RegOpenKeyExW failed with code {result}"));
    }
    let result = unsafe { RegDeleteValueW(key, value.as_ptr()) };
    unsafe {
        RegCloseKey(key);
    }
    match result {
        0 | ERROR_FILE_NOT_FOUND => Ok(()),
        code => Err(format!("RegDeleteValueW failed with code {code}")),
    }
}

#[cfg(windows)]
pub(super) fn broadcast_setting_change(area: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    let area = wide(area);
    let mut result = 0_usize;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            area.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1_000,
            &mut result,
        );
    }
}

#[cfg(windows)]
pub(super) fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
