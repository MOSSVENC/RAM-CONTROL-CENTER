//! Direct transport for the signed HotKey keyboard filter driver.
//!
//! The vendor exposes its macro/filter API through `HKFltrLib.dll`, but the
//! device itself is a normal signed control device. The recovered interface
//! GUID is used here so Neo Control can talk to `HKKbdFltr.sys` without
//! loading any vendor user-mode DLL.

#[cfg(any(windows, test))]
use crate::{DchuError, Result};

pub const IOCTL_ENABLE_FILTER: u32 = 0x000b_a400;
pub const IOCTL_ADD_FILTER_MACRO: u32 = 0x000b_a404;
pub const IOCTL_CLEAR_FILTER_MACRO: u32 = 0x000b_a40c;
pub const IOCTL_REMOVE_FILTER_MACRO: u32 = 0x000b_a408;
pub const IOCTL_ENABLE_USB_DEVICE: u32 = 0x000b_a41c;

/// One 16-byte item in the two key collections consumed by the filter driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterRecord {
    pub mode: u32,
    pub down_delay_ms: u32,
    pub up_delay_ms: u32,
    pub key: u16,
    pub kind: u16,
}

impl FilterRecord {
    pub fn key_code(key: u16) -> Self {
        Self {
            mode: 0,
            down_delay_ms: 0,
            up_delay_ms: 0,
            key,
            kind: 0,
        }
    }

    pub fn macro_down(key: u16, delay_ms: u32) -> Self {
        Self {
            mode: 0,
            down_delay_ms: delay_ms,
            up_delay_ms: 0,
            key,
            kind: 1,
        }
    }

    pub fn macro_up(key: u16, delay_ms: u32) -> Self {
        Self {
            mode: 0,
            down_delay_ms: 0,
            up_delay_ms: delay_ms,
            key,
            kind: 2,
        }
    }

    fn write_to(self, output: &mut Vec<u8>) {
        output.extend_from_slice(&self.mode.to_le_bytes());
        output.extend_from_slice(&self.down_delay_ms.to_le_bytes());
        output.extend_from_slice(&self.up_delay_ms.to_le_bytes());
        output.extend_from_slice(&self.key.to_le_bytes());
        output.extend_from_slice(&self.kind.to_le_bytes());
    }
}

/// Serialize the filter's two key collections for `IOCTL_ADD_FILTER_MACRO`.
pub fn encode_filter_macro(trigger: &[FilterRecord], actions: &[FilterRecord]) -> Vec<u8> {
    let trigger_size = (trigger.len() * 16) as u32;
    let action_size = (actions.len() * 16) as u32;
    let mut output = Vec::with_capacity(8 + (trigger.len() + actions.len()) * 16);
    output.extend_from_slice(&trigger_size.to_le_bytes());
    output.extend_from_slice(&action_size.to_le_bytes());
    for record in trigger.iter().chain(actions) {
        record.write_to(&mut output);
    }
    output
}

/// One keyboard-filter macro selected by a trigger key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterMacro {
    pub trigger_key: u16,
    pub actions: Vec<FilterRecord>,
}

impl FilterMacro {
    /// Build a key-to-key remap with a short make/break sequence.
    pub fn remap(trigger_key: u16, action_key: u16) -> Self {
        Self {
            trigger_key,
            actions: vec![
                FilterRecord::macro_down(action_key, 10),
                FilterRecord::macro_up(action_key, 10),
            ],
        }
    }
}

#[cfg(any(windows, test))]
pub const EXTENDED_KEY_MAKE: u16 = 57463;
#[cfg(any(windows, test))]
pub const EXTENDED_KEY_BREAK: u16 = 57464;

#[cfg(any(windows, test))]
fn validate_macros(macros: &[FilterMacro]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for item in macros {
        if !matches!(item.trigger_key >> 8, 0xe0 | 0xe1) {
            return Err(DchuError::InvalidArgument(format!(
                "filter trigger key {:#06x} must use an E0/E1 scancode",
                item.trigger_key
            )));
        }
        if item.actions.is_empty() {
            return Err(DchuError::InvalidArgument(format!(
                "filter trigger {:#06x} has no action records",
                item.trigger_key
            )));
        }
        if !seen.insert(item.trigger_key) {
            return Err(DchuError::InvalidArgument(format!(
                "duplicate filter trigger key {:#06x}",
                item.trigger_key
            )));
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn trigger_variants(key: u16) -> [Vec<FilterRecord>; 3] {
    [
        vec![FilterRecord::key_code(key)],
        vec![
            FilterRecord::key_code(EXTENDED_KEY_MAKE),
            FilterRecord::key_code(key),
        ],
        vec![
            FilterRecord::key_code(EXTENDED_KEY_BREAK),
            FilterRecord::key_code(key),
        ],
    ]
}

/// Serialize the trigger collection for `IOCTL_REMOVE_FILTER_MACRO`.
pub fn encode_filter_macro_removal(trigger: &[FilterRecord]) -> Vec<u8> {
    let trigger_size = (trigger.len() * 16) as u32;
    let mut output = Vec::with_capacity(8 + trigger.len() * 16);
    output.extend_from_slice(&trigger_size.to_le_bytes());
    output.extend_from_slice(&0_u32.to_le_bytes());
    for record in trigger {
        record.write_to(&mut output);
    }
    output
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    use super::{
        FilterMacro, FilterRecord, IOCTL_ADD_FILTER_MACRO, IOCTL_CLEAR_FILTER_MACRO,
        IOCTL_ENABLE_FILTER, IOCTL_ENABLE_USB_DEVICE, encode_filter_macro, trigger_variants,
        validate_macros,
    };
    use crate::{DchuError, Result};

    pub const DEVICE_PATH: &str = r"\\.\{2dc8164f-3a23-4d71-9fa6-91f9ead88613}";

    #[derive(Debug)]
    pub struct KeyboardFilter {
        handle: HANDLE,
    }

    impl KeyboardFilter {
        pub fn open() -> Result<Self> {
            let wide = std::ffi::OsStr::new(DEVICE_PATH)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0x80,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(DchuError::Status {
                    operation: "CreateFileW(HKKbdFltr)",
                    code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
                });
            }
            Ok(Self { handle })
        }

        pub fn set_enabled(&self, enabled: bool) -> Result<()> {
            let input = [u8::from(enabled)];
            self.ioctl(IOCTL_ENABLE_FILTER, &input, "enable filter")
        }

        pub fn set_usb_enabled(&self, enabled: bool) -> Result<()> {
            let input = [u8::from(enabled)];
            self.ioctl(IOCTL_ENABLE_USB_DEVICE, &input, "enable USB filter device")
        }

        pub fn clear_macros(&self) -> Result<()> {
            self.ioctl(IOCTL_CLEAR_FILTER_MACRO, &[], "clear filter macro")
        }

        /// Install a simple remap: when `trigger_key` is pressed, emit
        /// `action_key` down/up. Keys use the vendor's extended key codes.
        ///
        /// CC40 installs three trigger variants per key (make, extended make,
        /// extended break). The filter driver requires all three to bind the
        /// physical key consistently.
        pub fn add_macro(&self, trigger_key: u16, action_key: u16) -> Result<()> {
            self.add_filter_selection(&FilterMacro::remap(trigger_key, action_key))
        }

        /// Replace the complete filter-macro set.
        ///
        /// The driver's `0xBA408` path requires an internal match state that
        /// the vendor helper never establishes; the vendor helper first clears
        /// every macro with `0xBA40C` and ignores the remove result. Neo
        /// Control therefore performs a transactional clear-and-reapply of the
        /// desired set instead of exposing a misleading single-delete call.
        pub fn replace_macros(&self, macros: &[FilterMacro]) -> Result<()> {
            validate_macros(macros)?;
            self.clear_macros()?;
            for item in macros {
                if let Err(error) = self.add_filter_selection(item) {
                    let cleanup = self.clear_macros();
                    return Err(DchuError::ControlFailed {
                        operation: "replace filter macros",
                        detail: format!(
                            "could not install trigger {:#06x}: {error}; cleanup: {}",
                            item.trigger_key,
                            cleanup
                                .map(|()| "succeeded".to_owned())
                                .unwrap_or_else(|cleanup| cleanup.to_string())
                        ),
                    });
                }
            }
            Ok(())
        }

        fn add_filter_selection(&self, item: &FilterMacro) -> Result<()> {
            validate_macros(std::slice::from_ref(item))?;
            for trigger in trigger_variants(item.trigger_key) {
                // The vendor helper opens and closes one device handle per
                // trigger collection. Preserve that lifecycle.
                let filter = Self::open()?;
                filter.add_filter_macro(&trigger, &item.actions)?;
            }
            Ok(())
        }

        pub fn add_filter_macro(
            &self,
            trigger: &[FilterRecord],
            actions: &[FilterRecord],
        ) -> Result<()> {
            if trigger.is_empty() || actions.is_empty() {
                return Err(DchuError::InvalidArgument(
                    "filter macro requires trigger and action records".to_owned(),
                ));
            }
            let input = encode_filter_macro(trigger, actions);
            self.ioctl(IOCTL_ADD_FILTER_MACRO, &input, "add filter macro")
        }

        fn ioctl(&self, code: u32, input: &[u8], operation: &'static str) -> Result<()> {
            let mut returned = 0_u32;
            let ok = unsafe {
                DeviceIoControl(
                    self.handle,
                    code,
                    if input.is_empty() {
                        std::ptr::null()
                    } else {
                        input.as_ptr().cast()
                    },
                    input.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(DchuError::Status {
                    operation,
                    code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
                });
            }
            Ok(())
        }
    }

    impl Drop for KeyboardFilter {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{FilterMacro, FilterRecord};
    use crate::{DchuError, Result};

    pub const DEVICE_PATH: &str = r"\\.\{2dc8164f-3a23-4d71-9fa6-91f9ead88613}";

    #[derive(Debug)]
    pub struct KeyboardFilter;

    impl KeyboardFilter {
        pub fn open() -> Result<Self> {
            Err(DchuError::UnsupportedPlatform)
        }

        pub fn set_enabled(&self, _enabled: bool) -> Result<()> {
            Err(DchuError::UnsupportedPlatform)
        }

        pub fn set_usb_enabled(&self, _enabled: bool) -> Result<()> {
            Err(DchuError::UnsupportedPlatform)
        }

        pub fn clear_macros(&self) -> Result<()> {
            Err(DchuError::UnsupportedPlatform)
        }

        pub fn add_macro(&self, _trigger_key: u16, _action_key: u16) -> Result<()> {
            Err(DchuError::UnsupportedPlatform)
        }

        pub fn replace_macros(&self, _macros: &[FilterMacro]) -> Result<()> {
            Err(DchuError::UnsupportedPlatform)
        }

        pub fn add_filter_macro(
            &self,
            _trigger: &[FilterRecord],
            _actions: &[FilterRecord],
        ) -> Result<()> {
            Err(DchuError::UnsupportedPlatform)
        }
    }
}

pub use platform::{DEVICE_PATH, KeyboardFilter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_variants_cover_raw_and_extended_states() {
        let variants = trigger_variants(0xE044);
        assert_eq!(variants[0].len(), 1);
        assert_eq!(variants[0][0].key, 0xE044);
        assert_eq!(variants[1][0].key, EXTENDED_KEY_MAKE);
        assert_eq!(variants[1][1].key, 0xE044);
        assert_eq!(variants[2][0].key, EXTENDED_KEY_BREAK);
        assert_eq!(variants[2][1].key, 0xE044);
    }

    #[test]
    fn macro_validation_accepts_unique_extended_triggers() {
        let macros = [
            FilterMacro::remap(0xE044, 0xE045),
            FilterMacro::remap(0xE046, 0xE047),
        ];
        assert!(validate_macros(&macros).is_ok());
    }

    #[test]
    fn macro_validation_rejects_repeated_or_invalid_triggers() {
        let repeated = [
            FilterMacro::remap(0xE044, 0xE045),
            FilterMacro::remap(0xE044, 0xE047),
        ];
        assert!(validate_macros(&repeated).is_err());

        let invalid = [FilterMacro::remap(0x0044, 0xE045)];
        assert!(validate_macros(&invalid).is_err());
    }

    #[test]
    fn macro_validation_rejects_an_empty_action_sequence() {
        let empty = [FilterMacro {
            trigger_key: 0xE044,
            actions: Vec::new(),
        }];
        assert!(validate_macros(&empty).is_err());
    }
}
