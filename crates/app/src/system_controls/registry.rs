#[cfg(windows)]
use windows_registry::CURRENT_USER;

#[cfg(windows)]
const HRESULT_FILE_NOT_FOUND: u32 = 0x8007_0002;
#[cfg(windows)]
const HRESULT_PATH_NOT_FOUND: u32 = 0x8007_0003;

#[cfg(windows)]
fn is_not_found(error: &windows::core::Error) -> bool {
    matches!(
        error.code().0 as u32,
        HRESULT_FILE_NOT_FOUND | HRESULT_PATH_NOT_FOUND
    )
}

#[cfg(windows)]
fn optional_value<T>(
    operation: &str,
    result: windows_registry::Result<T>,
) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if is_not_found(&error) => Ok(None),
        Err(error) => Err(format!("{operation} failed: {}", error.message())),
    }
}

#[cfg(windows)]
pub(crate) fn read_registry_dword(subkey: &str, value: &str) -> Result<Option<u32>, String> {
    optional_value(
        "read registry DWORD",
        CURRENT_USER.open(subkey).and_then(|key| key.get_u32(value)),
    )
}

#[cfg(windows)]
pub(crate) fn read_registry_string(subkey: &str, value: &str) -> Result<Option<String>, String> {
    optional_value(
        "read registry string",
        CURRENT_USER
            .open(subkey)
            .and_then(|key| key.get_string(value)),
    )
}

#[cfg(windows)]
pub(crate) fn write_registry_dword(subkey: &str, value: &str, data: u32) -> Result<(), String> {
    CURRENT_USER
        .create(subkey)
        .and_then(|key| key.set_u32(value, data))
        .map_err(|error| format!("write registry DWORD failed: {}", error.message()))
}

#[cfg(windows)]
pub(crate) fn write_registry_string(subkey: &str, value: &str, data: &str) -> Result<(), String> {
    CURRENT_USER
        .create(subkey)
        .and_then(|key| key.set_string(value, data))
        .map_err(|error| format!("write registry string failed: {}", error.message()))
}

#[cfg(windows)]
pub(crate) fn delete_registry_value(subkey: &str, value: &str) -> Result<(), String> {
    match CURRENT_USER
        .options()
        .write()
        .open(subkey)
        .and_then(|key| key.remove_value(value))
    {
        Ok(()) => Ok(()),
        Err(error) if is_not_found(&error) => Ok(()),
        Err(error) => Err(format!("delete registry value failed: {}", error.message())),
    }
}

#[cfg(windows)]
pub(crate) fn broadcast_setting_change(area: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    let area = wide(area);
    let mut result = 0_usize;
    // SAFETY: `area` is a live NUL-terminated UTF-16 string for this synchronous
    // call, and `result` is valid writable storage for the duration of the call.
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
pub(crate) fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_registry::CURRENT_USER;

    const TEST_KEY: &str = r"Software\NeoControl\Tests";

    #[test]
    fn registry_values_round_trip_and_missing_values_are_none() {
        let _ = write_registry_dword(TEST_KEY, "dword", 42);
        let _ = write_registry_string(TEST_KEY, "string", "Neo Control");

        assert_eq!(read_registry_dword(TEST_KEY, "dword").unwrap(), Some(42));
        assert_eq!(
            read_registry_string(TEST_KEY, "string").unwrap().as_deref(),
            Some("Neo Control")
        );
        assert_eq!(read_registry_dword(TEST_KEY, "absent").unwrap(), None);

        delete_registry_value(TEST_KEY, "dword").unwrap();
        delete_registry_value(TEST_KEY, "string").unwrap();
        assert_eq!(read_registry_dword(TEST_KEY, "dword").unwrap(), None);
        assert_eq!(read_registry_string(TEST_KEY, "string").unwrap(), None);
        let _ = CURRENT_USER.remove_tree(TEST_KEY);
    }
}
