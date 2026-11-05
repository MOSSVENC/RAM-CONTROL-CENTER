#[cfg(windows)]
pub(crate) fn send_virtual_key(vk: u8) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, keybd_event};
    unsafe {
        keybd_event(vk, 0, 0, 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn send_virtual_key(_vk: u8) -> Result<(), String> {
    Err("virtual key input is only available on Windows".to_owned())
}

#[cfg(windows)]
pub(crate) fn send_touchpad_chord() -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, keybd_event};
    const VK_CONTROL: u8 = 0x11;
    const VK_LWIN: u8 = 0x5b;
    const VK_F24: u8 = 0x87;
    unsafe {
        keybd_event(VK_CONTROL, 0, 0, 0);
        keybd_event(VK_LWIN, 0, 0, 0);
        keybd_event(VK_F24, 0, 0, 0);
        keybd_event(VK_F24, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_LWIN, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn send_touchpad_chord() -> Result<(), String> {
    Err("touchpad input is only available on Windows".to_owned())
}
