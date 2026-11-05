use std::os::windows::ffi::OsStrExt;
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::{DchuError, FanMode, PowerMode, QuickToggle, Result};

mod charging;
mod gpu;
mod input;
mod power_fan;
mod quick_toggles;
mod rgb;
mod telemetry;

const DEVICE_PATH: &str = r"\\?\ACPI#CLV0001#1#{86994c74-ad43-4812-b7e7-0c420b5c5fd7}";
const IOCTL_ACPI_DCHU: u32 = 0x0032_2400;
const IOCTL_ACPI_SETTINGS: u32 = 0x0032_240C;
const ACPI_REQUEST_LEN: usize = 0x420;
const ACPI_RESPONSE_LEN: usize = 0x40c;
const SETTINGS_BUFFER_LEN: usize = 0x1000;
const SETTINGS_REQUEST_LEN: usize = 0x1008;
const ACPI_READ_GUID: [u8; 16] = [
    0xE4, 0x24, 0xF2, 0x93, 0xDC, 0xFB, 0xBF, 0x4B, 0xAD, 0xD6, 0xDB, 0x71, 0xBD, 0xC0, 0xAF, 0xAD,
];

pub struct Dchu {
    handle: HANDLE,
    path: String,
}

impl std::fmt::Debug for Dchu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dchu").field("path", &self.path).finish()
    }
}

impl Drop for Dchu {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE && !self.handle.is_null() {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

impl Dchu {
    pub fn open() -> Result<Self> {
        let handle = open_device()?;
        Ok(Self {
            handle,
            path: DEVICE_PATH.to_owned(),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn read_integer_checked(&self, command: i32, operation: &'static str) -> Result<i32> {
        self.read_integer(command)
            .ok_or_else(|| DchuError::ControlFailed {
                operation,
                detail: format!("ACPI command {command} did not return an integer"),
            })
    }

    pub fn has_extended_api(&self) -> bool {
        true
    }

    fn set_data_ex(&self, command: i32, input: &[u8], output: &mut [u8]) -> Result<()> {
        self.acpi_ioctl(command, input, output)?;
        Ok(())
    }

    fn read_byte(&self, page: i32, offset: i32) -> Option<u8> {
        self.read_settings(page, offset, 1)
            .ok()
            .and_then(|bytes| bytes.first().copied())
    }

    fn read_integer(&self, command: i32) -> Option<i32> {
        let mut data = [0_u8; 4];
        self.acpi_ioctl(command, &[], &mut data).ok()?;
        Some(i32::from_le_bytes(data))
    }

    fn read_settings(&self, page: i32, offset: i32, length: usize) -> Result<Vec<u8>> {
        if length == 0 {
            return Err(DchuError::InvalidArgument(
                "ReadAppSettings length must be non-zero".to_owned(),
            ));
        }
        let start = page
            .checked_mul(256)
            .and_then(|base| base.checked_add(offset))
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| DchuError::InvalidArgument("settings address is invalid".to_owned()))?;
        if start + length > SETTINGS_BUFFER_LEN {
            return Err(DchuError::InvalidArgument(
                "settings range exceeds the vendor page buffer".to_owned(),
            ));
        }
        let buffer = self.read_settings_store()?;
        Ok(buffer[start..start + length].to_vec())
    }

    fn set_data(&self, command: i32, payload: &[u8]) -> Result<()> {
        self.acpi_ioctl(command, payload, &mut [])?;
        Ok(())
    }

    fn write_settings(&self, page: i32, offset: i32, payload: &[u8]) -> Result<()> {
        if payload.len() > SETTINGS_BUFFER_LEN {
            return Err(DchuError::InvalidArgument(
                "WriteAppSettings payload is too large".to_owned(),
            ));
        }
        let address = page
            .checked_mul(256)
            .and_then(|base| base.checked_add(offset))
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| DchuError::InvalidArgument("settings address is invalid".to_owned()))?;
        let length = u16::try_from(payload.len())
            .map_err(|_| DchuError::InvalidArgument("settings length exceeds u16".to_owned()))?;
        let mut request = [0_u8; SETTINGS_REQUEST_LEN];
        request[0..4].copy_from_slice(&1_u32.to_le_bytes());
        request[4..6].copy_from_slice(&address.to_le_bytes());
        request[6..8].copy_from_slice(&length.to_le_bytes());
        request[8..8 + payload.len()].copy_from_slice(payload);
        let mut returned = 0_u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_ACPI_SETTINGS,
                request.as_ptr().cast(),
                request.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(DchuError::Status {
                operation: "DeviceIoControl(0x32240C write)",
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        Ok(())
    }

    fn read_settings_store(&self) -> Result<[u8; SETTINGS_BUFFER_LEN]> {
        let mut request = [0_u8; 12];
        request[4..8].copy_from_slice(&0x1000_0000_u32.to_le_bytes());
        let mut output = [0_u8; SETTINGS_BUFFER_LEN];
        let mut returned = 0_u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_ACPI_SETTINGS,
                request.as_ptr().cast(),
                request.len() as u32,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(DchuError::Status {
                operation: "DeviceIoControl(0x32240C read)",
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        if returned as usize != SETTINGS_BUFFER_LEN {
            return Err(DchuError::ControlFailed {
                operation: "read ACPI settings",
                detail: format!("expected {SETTINGS_BUFFER_LEN} bytes, received {returned} bytes"),
            });
        }
        Ok(output)
    }

    fn acpi_read_buffer(&self, command: i32) -> Result<[u8; 256]> {
        let mut output = [0_u8; 256];
        self.acpi_ioctl(command, &[], &mut output)?;
        Ok(output)
    }

    fn acpi_ioctl(&self, command: i32, payload: &[u8], output: &mut [u8]) -> Result<()> {
        if payload.len() > 0x100 {
            return Err(DchuError::InvalidArgument(
                "DCHU payload exceeds the 256-byte ACPI frame".to_owned(),
            ));
        }
        let mut request = [0_u8; ACPI_REQUEST_LEN];
        request[0..16].copy_from_slice(&ACPI_READ_GUID);
        request[20..24].copy_from_slice(b"_DSM");
        request[24..28].copy_from_slice(&command.to_le_bytes());
        request[28..30].copy_from_slice(&0x0104_u16.to_le_bytes());
        request[30..34].copy_from_slice(&0x0100_0002_u32.to_le_bytes());
        request[34..34 + payload.len()].copy_from_slice(payload);

        let mut response = [0_u8; ACPI_RESPONSE_LEN];
        let mut returned = 0_u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_ACPI_DCHU,
                request.as_ptr().cast(),
                request.len() as u32,
                response.as_mut_ptr().cast(),
                response.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(DchuError::Status {
                operation: "DeviceIoControl(0x322400)",
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        parse_acpi_payload(&response, returned, output)?;
        Ok(())
    }

    fn apply_power_mode(&self, mode: u8) -> Result<()> {
        // Match the vendor's power transition rather than treating one byte as
        // the whole operation. FnKey writes the platform policy at page 1/16,
        // persists the UI profile at page 1/1, sends 121/25, then commits the
        // processor boost policy with command 79. Without the last command
        // AppSettings can report Performance while the EC/CPU remains in its
        // cold-boot low-power state.
        self.write_settings(1, 16, &[mode])?;
        self.write_settings(1, 1, &[mode])?;
        self.set_data(121, &[mode, 0, 0, 25])?;
        if let Some(boost_mode) = PowerMode::from_u8(mode).and_then(PowerMode::boost_mode) {
            self.set_data(79, &[boost_mode, 0, 0, 0])?;
        }
        Ok(())
    }

    fn apply_fan_mode(&self, mode: FanMode) -> Result<()> {
        self.apply_fan_mode_value(mode.as_u8())?;
        if mode == FanMode::Auto {
            // CC40 resets both vendor fan offsets when Automatic is selected.
            self.write_settings(4, 72, &[0])?;
            self.write_settings(4, 76, &[0])?;
        }
        Ok(())
    }

    fn wait_for_byte(&self, page: i32, offset: i32, expected: u8, timeout: Duration) -> Option<u8> {
        let started = Instant::now();
        let mut last = None;
        while started.elapsed() <= timeout {
            let actual = self.read_byte(page, offset);
            last = actual;
            if actual == Some(expected) {
                return actual;
            }
            thread::sleep(Duration::from_millis(50));
        }
        last
    }

    fn rollback_power_after_mismatch(&self, previous: Option<u8>) -> Option<String> {
        let previous = previous?;
        Some(match self.apply_power_mode(previous) {
            Ok(()) => format!("restored power mode {previous}"),
            Err(error) => format!("could not restore power mode {previous}: {error}"),
        })
    }

    fn rollback_fan_after_mismatch(&self, previous: Option<u8>) -> Option<String> {
        let previous = previous?;
        let rollback = FanMode::from_u8(previous)
            .map(|mode| self.apply_fan_mode(mode))
            .unwrap_or_else(|| Ok(()));
        Some(match rollback {
            Ok(()) => format!("restored fan mode {previous}"),
            Err(error) => format!("could not restore fan mode {previous}: {error}"),
        })
    }

    fn rollback_power(
        &self,
        previous: Option<u8>,
        context: &'static str,
        original: DchuError,
    ) -> DchuError {
        let rollback = previous.map(|value| match self.apply_power_mode(value) {
            Ok(()) => format!("restored power mode {value}"),
            Err(error) => format!("could not restore power mode {value}: {error}"),
        });
        DchuError::ControlFailed {
            operation: context,
            detail: format!(
                "{original}; {}",
                rollback.unwrap_or_else(|| "previous state was unavailable".to_owned())
            ),
        }
    }

    fn rollback_fan(
        &self,
        previous: Option<u8>,
        context: &'static str,
        original: DchuError,
    ) -> DchuError {
        let rollback = previous.map(|value| {
            let result = FanMode::from_u8(value)
                .map(|mode| self.apply_fan_mode(mode))
                .unwrap_or_else(|| Ok(()));
            match result {
                Ok(()) => format!("restored fan mode {value}"),
                Err(error) => format!("could not restore fan mode {value}: {error}"),
            }
        });
        DchuError::ControlFailed {
            operation: context,
            detail: format!(
                "{original}; {}",
                rollback.unwrap_or_else(|| "previous state was unavailable".to_owned())
            ),
        }
    }
}

fn open_device() -> Result<HANDLE> {
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
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(DchuError::Status {
            operation: "CreateFileW(AcpiBridge)",
            code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
        });
    }
    Ok(handle)
}

fn parse_acpi_payload(response: &[u8], returned: u32, output: &mut [u8]) -> Result<usize> {
    const HEADER_LEN: usize = 12;
    const DESCRIPTOR_LEN: usize = 4;

    let returned = usize::try_from(returned).unwrap_or(0).min(response.len());
    if returned < HEADER_LEN {
        return Err(DchuError::ControlFailed {
            operation: "parse ACPI response",
            detail: format!("truncated header: received {returned} bytes, need {HEADER_LEN}"),
        });
    }
    let descriptor_count = u32::from_le_bytes(
        response
            .get(8..12)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| DchuError::ControlFailed {
                operation: "parse ACPI response",
                detail: "descriptor count field is missing".to_owned(),
            })?,
    );
    let mut offset = HEADER_LEN;
    let mut copied = 0_usize;

    for descriptor_index in 0..descriptor_count {
        if offset + DESCRIPTOR_LEN > returned {
            return Err(DchuError::ControlFailed {
                operation: "parse ACPI response",
                detail: format!(
                    "descriptor {descriptor_index} header is truncated at byte {offset}"
                ),
            });
        }
        let kind = u16::from_le_bytes([response[offset], response[offset + 1]]);
        let length = usize::from(u16::from_le_bytes([
            response[offset + 2],
            response[offset + 3],
        ]));
        let data_start = offset + DESCRIPTOR_LEN;
        let data_end = data_start
            .checked_add(length)
            .ok_or_else(|| DchuError::ControlFailed {
                operation: "parse ACPI response",
                detail: format!("descriptor {descriptor_index} length overflow"),
            })?;
        if data_end > returned {
            return Err(DchuError::ControlFailed {
                operation: "parse ACPI response",
                detail: format!(
                    "descriptor {descriptor_index} declares {length} bytes but only {} are available",
                    returned.saturating_sub(data_start)
                ),
            });
        }

        let payload = &response[data_start..data_end];
        match kind {
            // Byte-buffer descriptor.
            2 => {
                let take = payload.len().min(output.len().saturating_sub(copied));
                if take > 0 {
                    output[copied..copied + take].copy_from_slice(&payload[..take]);
                    copied += take;
                }
            }
            // Integer descriptor. DCHU integers are four little-endian bytes.
            0 => {
                if payload.len() < 4 {
                    return Err(DchuError::ControlFailed {
                        operation: "parse ACPI response",
                        detail: format!(
                            "descriptor {descriptor_index} is an integer with {} bytes, expected 4",
                            payload.len()
                        ),
                    });
                }
                let take = 4_usize.min(output.len().saturating_sub(copied));
                if take > 0 {
                    output[copied..copied + take].copy_from_slice(&payload[..take]);
                    copied += take;
                }
            }
            _ => {}
        }
        offset = data_end;
    }

    if !output.is_empty() && copied < output.len() {
        return Err(DchuError::ControlFailed {
            operation: "parse ACPI response",
            detail: format!(
                "response supplied {copied} bytes but the caller expected {}",
                output.len()
            ),
        });
    }
    Ok(copied)
}

fn decode_toggle_byte(toggle: QuickToggle, value: u8) -> Option<bool> {
    match (toggle, value) {
        (QuickToggle::WinLock, 1) => Some(false),
        (QuickToggle::WinLock, 2) => Some(true),
        (_, 1) => Some(true),
        (_, 2) => Some(false),
        _ => None,
    }
}

fn status_non_negative(operation: &'static str, status: i32) -> Result<()> {
    if status >= 0 {
        Ok(())
    } else {
        Err(DchuError::Status {
            operation,
            code: status,
        })
    }
}
