use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::System::Power::{
    PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex, PowerSetActiveScheme,
    PowerWriteACValueIndex, PowerWriteDCValueIndex,
};
use windows_sys::core::GUID;

struct ActivePowerScheme(*mut GUID);

impl ActivePowerScheme {
    fn get() -> Result<Self, String> {
        let mut scheme = std::ptr::null_mut();
        // SAFETY: the output pointer refers to a live local pointer variable;
        // on success Windows returns an allocation owned by this wrapper.
        let status = unsafe { PowerGetActiveScheme(std::ptr::null_mut(), &mut scheme) };
        if status != 0 || scheme.is_null() {
            return Err("could not read the active power scheme".to_owned());
        }
        Ok(Self(scheme))
    }

    fn as_ptr(&self) -> *const GUID {
        self.0
    }

    fn activate(&self) {
        // SAFETY: `self.0` is the non-null GUID allocation owned by this value
        // until Drop, and PowerSetActiveScheme only borrows it for the call.
        let _ = unsafe { PowerSetActiveScheme(std::ptr::null_mut(), self.0) };
    }
}

impl Drop for ActivePowerScheme {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` is the allocation returned by
            // PowerGetActiveScheme and is freed exactly once here.
            unsafe { LocalFree(self.0.cast()) };
        }
    }
}

pub(super) fn read_pair(subgroup: &GUID, setting: &GUID) -> Result<(u32, u32), String> {
    let scheme = ActivePowerScheme::get()?;
    let mut ac = 0_u32;
    let mut dc = 0_u32;
    // SAFETY: the scheme allocation and both GUID references are valid for the
    // call, and `ac`/`dc` are live mutable u32 outputs.
    let (ac_status, dc_status) = unsafe {
        (
            PowerReadACValueIndex(
                std::ptr::null_mut(),
                scheme.as_ptr(),
                subgroup,
                setting,
                &mut ac,
            ),
            PowerReadDCValueIndex(
                std::ptr::null_mut(),
                scheme.as_ptr(),
                subgroup,
                setting,
                &mut dc,
            ),
        )
    };
    if ac_status != 0 || dc_status != 0 {
        return Err("the active power scheme does not expose this setting".to_owned());
    }
    Ok((ac, dc))
}

pub(super) fn write_ac(subgroup: &GUID, setting: &GUID, value: u32) -> Result<(), String> {
    write_one(subgroup, setting, value, true)
}

pub(super) fn write_dc(subgroup: &GUID, setting: &GUID, value: u32) -> Result<(), String> {
    write_one(subgroup, setting, value, false)
}

fn write_one(subgroup: &GUID, setting: &GUID, value: u32, ac: bool) -> Result<(), String> {
    let scheme = ActivePowerScheme::get()?;
    // SAFETY: the active scheme and GUID references remain valid for the call;
    // the selected write function only reads those inputs.
    let status = unsafe {
        if ac {
            PowerWriteACValueIndex(
                std::ptr::null_mut(),
                scheme.as_ptr(),
                subgroup,
                setting,
                value,
            )
        } else {
            PowerWriteDCValueIndex(
                std::ptr::null_mut(),
                scheme.as_ptr(),
                subgroup,
                setting,
                value,
            )
        }
    };
    if status != 0 {
        return Err("could not update the active power scheme".to_owned());
    }
    scheme.activate();
    Ok(())
}

pub(super) fn write_both(subgroup: &GUID, setting: &GUID, value: u32) -> Result<(), String> {
    let scheme = ActivePowerScheme::get()?;
    // SAFETY: the active scheme and GUID references remain valid for the call;
    // both write functions only consume scalar values and borrowed inputs.
    let (ac_status, dc_status) = unsafe {
        (
            PowerWriteACValueIndex(
                std::ptr::null_mut(),
                scheme.as_ptr(),
                subgroup,
                setting,
                value,
            ),
            PowerWriteDCValueIndex(
                std::ptr::null_mut(),
                scheme.as_ptr(),
                subgroup,
                setting,
                value,
            ),
        )
    };
    if ac_status != 0 || dc_status != 0 {
        return Err("could not update the active power scheme".to_owned());
    }
    scheme.activate();
    Ok(())
}
