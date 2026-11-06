#[cfg(windows)]
use super::power;
#[cfg(windows)]
use windows_sys::core::GUID;

#[cfg(windows)]
const VIDEO_SUBGROUP: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
#[cfg(windows)]
const VIDEO_BRIGHTNESS: GUID = GUID::from_u128(0xaded5e82_b909_4619_9949_f5d71dac0bcb);

#[cfg(windows)]
pub fn screen_brightness() -> Result<u8, String> {
    let (ac, _) = power::read_pair(&VIDEO_SUBGROUP, &VIDEO_BRIGHTNESS)
        .map_err(|_| "the active power scheme does not expose display brightness".to_owned())?;
    Ok(ac.min(100) as u8)
}

#[cfg(not(windows))]
pub fn screen_brightness() -> Result<u8, String> {
    Err("display brightness is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_screen_brightness(percent: u8) -> Result<(), String> {
    power::write_both(
        &VIDEO_SUBGROUP,
        &VIDEO_BRIGHTNESS,
        u32::from(percent.min(100)),
    )
    .map_err(|_| "could not update display brightness".to_owned())
}

#[cfg(not(windows))]
pub fn set_screen_brightness(_percent: u8) -> Result<(), String> {
    Err("display brightness is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_blue_light_filter(level: u8) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, HDC, ReleaseDC};
    use windows_sys::Win32::UI::ColorSystem::SetDeviceGammaRamp;

    #[repr(C)]
    struct GammaRamp {
        red: [u16; 256],
        green: [u16; 256],
        blue: [u16; 256],
    }

    struct DeviceContext(HDC);

    impl DeviceContext {
        fn acquire() -> Result<Self, String> {
            // SAFETY: a null HWND requests the screen DC and has no pointer
            // aliasing requirements; ownership is tracked by DeviceContext.
            let dc = unsafe { GetDC(std::ptr::null_mut()) };
            if dc.is_null() {
                Err("could not acquire the display device context".to_owned())
            } else {
                Ok(Self(dc))
            }
        }
    }

    impl Drop for DeviceContext {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns the non-null screen DC acquired above;
            // the same HWND/HDC pair is released exactly once in Drop.
            unsafe { ReleaseDC(std::ptr::null_mut(), self.0) };
        }
    }

    let scale = if level == 0 {
        1.0_f64
    } else {
        (f64::from(level.min(10)) + 10.0) / 10.0
    };
    let dc = DeviceContext::acquire()?;
    let mut ramp = GammaRamp {
        red: [0; 256],
        green: [0; 256],
        blue: [0; 256],
    };
    for index in 0..256 {
        let linear = (index as f64 * 256.0).min(65535.0) as u16;
        ramp.red[index] = linear;
        ramp.green[index] = linear;
        ramp.blue[index] = (256.0 / scale * index as f64).min(65535.0) as u16;
    }
    // SAFETY: `dc.0` is a live screen DC and `ramp` is a repr(C) structure
    // containing the three 256-entry u16 ramps expected by SetDeviceGammaRamp.
    let written = unsafe { SetDeviceGammaRamp(dc.0, (&ramp as *const GammaRamp).cast()) };
    if written == 0 {
        return Err("the display driver rejected the gamma ramp".to_owned());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_blue_light_filter(_level: u8) -> Result<(), String> {
    Err("blue-light control is only available on Windows".to_owned())
}
