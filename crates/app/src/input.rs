#[cfg(windows)]
fn send_key_events(events: &[(u8, u32)]) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::keybd_event;

    unsafe {
        for &(virtual_key, flags) in events {
            keybd_event(virtual_key, 0, flags, 0);
        }
    }
}

#[cfg(windows)]
pub(crate) fn send_virtual_key(vk: u8) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP;

    send_key_events(&[(vk, 0), (vk, KEYEVENTF_KEYUP)]);
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn send_virtual_key(_vk: u8) -> Result<(), String> {
    Err("virtual key input is only available on Windows".to_owned())
}

#[cfg(windows)]
pub(crate) fn send_touchpad_chord() -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP;
    const VK_CONTROL: u8 = 0x11;
    const VK_LWIN: u8 = 0x5b;
    const VK_F24: u8 = 0x87;

    send_key_events(&[
        (VK_CONTROL, 0),
        (VK_LWIN, 0),
        (VK_F24, 0),
        (VK_F24, KEYEVENTF_KEYUP),
        (VK_LWIN, KEYEVENTF_KEYUP),
        (VK_CONTROL, KEYEVENTF_KEYUP),
    ]);
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn send_touchpad_chord() -> Result<(), String> {
    Err("touchpad input is only available on Windows".to_owned())
}
