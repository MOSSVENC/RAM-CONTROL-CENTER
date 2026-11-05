use super::{Dchu, status_non_negative};
use crate::{
    DchuError, PlatformCapabilities, PlatformState, Result, TELEMETRY_COMMAND, ThermalSample,
    decode_platform_capabilities, decode_thermal,
};

impl Dchu {
    /// Read-only diagnostic access to the signed AcpiBridge transport.
    pub fn raw_read_buffer(&self, command: i32) -> (i32, [u8; 256]) {
        match self.acpi_read_buffer(command) {
            Ok(buffer) => (0, buffer),
            Err(_) => (-1, [0_u8; 256]),
        }
    }

    pub fn raw_read_integer(&self, command: i32) -> (i32, i32) {
        match self.read_integer(command) {
            Some(value) => (0, value),
            None => (-1, 0),
        }
    }

    pub fn raw_read_settings(&self, page: i32, offset: i32, length: usize) -> (i32, Vec<u8>) {
        match self.read_settings(page, offset, length) {
            Ok(buffer) => (0, buffer),
            Err(_) => (-1, vec![0_u8; length]),
        }
    }

    pub fn read_thermal(&self) -> Result<ThermalSample> {
        let (status, buffer) = self.raw_read_buffer(TELEMETRY_COMMAND);
        status_non_negative("ACPI command 12", status)?;
        Ok(decode_thermal(&buffer))
    }

    pub fn read_platform_state(&self) -> PlatformState {
        PlatformState {
            power_mode: self.read_byte(1, 1),
            fan_mode: self.read_byte(4, 5),
            fan_offset: self.read_byte(4, 7),
            turbo_fan: self.read_byte(4, 8).map(|value| value != 0),
            slow_fan: self.read_byte(4, 9).map(|value| value != 0),
            anti_dust_running: self.read_byte(4, 80).map(|value| value != 0),
            power_policy: self.read_byte(1, 16),
            capabilities: self.read_integer(16),
            platform_id: self.read_integer(70),
            low_refresh_rate_supported: self.read_byte(1, 27).map(|value| value == 1),
            low_refresh_rate_enabled: self.read_byte(1, 28).map(|value| value == 1),
        }
    }

    /// Read one persisted DCHU AppSettings byte. This is used for vendor
    /// preference mirrors such as the display-care pages.
    pub fn read_setting_byte(&self, page: i32, offset: i32) -> Option<u8> {
        self.read_byte(page, offset)
    }

    /// Write and verify one persisted DCHU AppSettings byte, restoring the
    /// previous value when the readback does not match.
    pub fn write_setting_byte(&self, page: i32, offset: i32, value: u8) -> Result<()> {
        let previous = self.read_byte(page, offset);
        self.write_settings(page, offset, &[value])?;
        let actual = self.read_byte(page, offset);
        if actual == Some(value) {
            return Ok(());
        }
        let rollback = previous.map_or_else(
            || "previous value was unavailable".to_owned(),
            |previous| match self.write_settings(page, offset, &[previous]) {
                Ok(()) => format!("restored value {previous}"),
                Err(error) => format!("could not restore value {previous}: {error}"),
            },
        );
        Err(DchuError::ControlFailed {
            operation: "write AppSettings byte",
            detail: format!(
                "page {page} offset {offset} expected {value}, got {actual:?}; {rollback}"
            ),
        })
    }

    pub fn read_capabilities(&self) -> Result<PlatformCapabilities> {
        let feature = self.read_settings(7, 0, 256)?;
        let feature: [u8; 256] = feature.try_into().map_err(|_| DchuError::ControlFailed {
            operation: "read capabilities",
            detail: "feature page had an unexpected length".to_owned(),
        })?;
        let wmi_16 = self.read_integer_checked(16, "read WMI support 16")?;
        let wmi_122 = self.read_integer_checked(122, "read WMI support 122")?;
        let wmi_96 = self.read_integer_checked(96, "read WMI support 96")?;
        let ui_style_id = self
            .read_byte(1, 45)
            .ok_or_else(|| DchuError::ControlFailed {
                operation: "read capabilities",
                detail: "UI style setting was unavailable".to_owned(),
            })?;
        let oled_panel = self.read_byte(1, 49) == Some(1);
        let fan_info = self.acpi_read_buffer(13)?;
        Ok(decode_platform_capabilities(
            &feature,
            wmi_16,
            wmi_122,
            wmi_96,
            ui_style_id,
            oled_panel,
            &fan_info,
        ))
    }
}
