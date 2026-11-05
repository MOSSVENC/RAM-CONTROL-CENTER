#[cfg(windows)]
pub fn screen_brightness() -> Result<u8, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::System::Power::{PowerGetActiveScheme, PowerReadACValueIndex};
    use windows_sys::core::GUID;

    const VIDEO_SUBGROUP: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
    const VIDEO_BRIGHTNESS: GUID = GUID::from_u128(0xaded5e82_b909_4619_9949_f5d71dac0bcb);

    unsafe {
        let mut scheme_ptr: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(std::ptr::null_mut(), &mut scheme_ptr) != 0 || scheme_ptr.is_null()
        {
            return Err("could not read the active power scheme".to_owned());
        }
        let scheme = *scheme_ptr;
        let mut value = 0_u32;
        let result = PowerReadACValueIndex(
            std::ptr::null_mut(),
            &scheme,
            &VIDEO_SUBGROUP,
            &VIDEO_BRIGHTNESS,
            &mut value,
        );
        LocalFree(scheme_ptr.cast());
        if result != 0 {
            return Err("the active power scheme does not expose display brightness".to_owned());
        }
        Ok(value.min(100) as u8)
    }
}

#[cfg(not(windows))]
pub fn screen_brightness() -> Result<u8, String> {
    Err("display brightness is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_screen_brightness(percent: u8) -> Result<(), String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::System::Power::{
        PowerGetActiveScheme, PowerSetActiveScheme, PowerWriteACValueIndex, PowerWriteDCValueIndex,
    };
    use windows_sys::core::GUID;

    const VIDEO_SUBGROUP: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
    const VIDEO_BRIGHTNESS: GUID = GUID::from_u128(0xaded5e82_b909_4619_9949_f5d71dac0bcb);
    let value = u32::from(percent.min(100));

    unsafe {
        let mut scheme_ptr: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(std::ptr::null_mut(), &mut scheme_ptr) != 0 || scheme_ptr.is_null()
        {
            return Err("could not read the active power scheme".to_owned());
        }
        let scheme = *scheme_ptr;
        let ac = PowerWriteACValueIndex(
            std::ptr::null_mut(),
            &scheme,
            &VIDEO_SUBGROUP,
            &VIDEO_BRIGHTNESS,
            value,
        );
        let dc = PowerWriteDCValueIndex(
            std::ptr::null_mut(),
            &scheme,
            &VIDEO_SUBGROUP,
            &VIDEO_BRIGHTNESS,
            value,
        );
        if ac == 0 && dc == 0 {
            let _ = PowerSetActiveScheme(std::ptr::null_mut(), &scheme);
        }
        LocalFree(scheme_ptr.cast());
        if ac == 0 && dc == 0 {
            Ok(())
        } else {
            Err("could not update display brightness".to_owned())
        }
    }
}

#[cfg(not(windows))]
pub fn set_screen_brightness(_percent: u8) -> Result<(), String> {
    Err("display brightness is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_blue_light_filter(level: u8) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    use windows_sys::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};

    #[repr(C)]
    struct GammaRamp {
        red: [u16; 256],
        green: [u16; 256],
        blue: [u16; 256],
    }

    let scale = if level == 0 {
        1.0_f64
    } else {
        (f64::from(level.min(10)) + 10.0) / 10.0
    };
    unsafe {
        let dc = GetDC(std::ptr::null_mut());
        if dc.is_null() {
            return Err("could not acquire the display device context".to_owned());
        }
        let mut ramp = GammaRamp {
            red: [0; 256],
            green: [0; 256],
            blue: [0; 256],
        };
        let _ = GetDeviceGammaRamp(dc, (&mut ramp as *mut GammaRamp).cast());
        for index in 0..256 {
            let linear = (index as f64 * 256.0).min(65535.0) as u16;
            ramp.red[index] = linear;
            ramp.green[index] = linear;
            ramp.blue[index] = (256.0 / scale * index as f64).min(65535.0) as u16;
        }
        let ok = SetDeviceGammaRamp(dc, (&ramp as *const GammaRamp).cast()) != 0;
        ReleaseDC(std::ptr::null_mut(), dc);
        if ok {
            Ok(())
        } else {
            Err("the display driver rejected the gamma ramp".to_owned())
        }
    }
}

#[cfg(not(windows))]
pub fn set_blue_light_filter(_level: u8) -> Result<(), String> {
    Err("blue-light control is only available on Windows".to_owned())
}
