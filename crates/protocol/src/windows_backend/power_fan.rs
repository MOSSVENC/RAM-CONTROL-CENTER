use std::thread;
use std::time::Duration;

use super::{Dchu, status_non_negative};
use crate::{
    AntiDustSchedule, CustomFanCurve, DchuError, FanCurveState, FanMode, PowerMode, Result,
    RpmFanChannel, RpmFanTable, curve_with_custom_nodes, decode_fan_curve, decode_rpm_fan_channel,
    encode_fan_curve_frame, encode_rpm_fan_channel, fan_curve_matches, readback_confirmed,
    rpm_fan_channel_matches,
};

impl Dchu {
    pub fn set_power_mode(&self, mode: PowerMode) -> Result<()> {
        let target = mode.as_u8();
        if target == PowerMode::Turbo.as_u8() {
            return Err(DchuError::InvalidArgument(
                "Turbo is a composite platform mode and is not exposed by RAM CONTROL CENTER"
                    .to_owned(),
            ));
        }

        let previous = self.read_byte(1, 1);
        let command_result = self.apply_power_mode(target);
        if let Err(error) = command_result {
            return Err(self.rollback_power(previous, "power command failed", error));
        }

        let actual = self.wait_for_byte(1, 1, target, Duration::from_millis(600));
        let policy = self.wait_for_byte(1, 16, target, Duration::from_millis(300));
        if readback_confirmed(target, actual) && readback_confirmed(target, policy) {
            return Ok(());
        }
        let rollback = self.rollback_power_after_mismatch(previous);
        if !readback_confirmed(target, actual) {
            return Err(DchuError::ReadbackMismatch {
                operation: "set power mode",
                expected: target,
                actual,
                rollback,
            });
        }
        Err(DchuError::ControlFailed {
            operation: "set power policy",
            detail: format!(
                "readback expected {target}, got {policy:?}; profile readback {actual:?}; {}",
                rollback
                    .as_deref()
                    .unwrap_or("previous state was unavailable")
            ),
        })
    }

    pub fn set_fan_mode(&self, mode: FanMode) -> Result<()> {
        if mode == FanMode::AntiDust {
            return self.run_anti_dust();
        }
        if mode == FanMode::Noiseless {
            return self.set_slow_fan(true);
        }
        let target = mode.as_u8();
        let previous = self.read_byte(4, 5);
        if let Err(error) = self.apply_fan_mode(mode) {
            return Err(self.rollback_fan(previous, "fan command failed", error));
        }

        let actual = self.wait_for_byte(4, 5, target, Duration::from_millis(600));
        if readback_confirmed(target, actual) {
            return Ok(());
        }
        let rollback = self.rollback_fan_after_mismatch(previous);
        Err(DchuError::ReadbackMismatch {
            operation: "set fan mode",
            expected: target,
            actual,
            rollback,
        })
    }

    /// Enable or disable CC40's Noiseless fan mode (vendor fan value 8).
    pub fn set_slow_fan(&self, enabled: bool) -> Result<()> {
        let previous_mode = self.read_byte(4, 5);
        let previous_slow = self.read_byte(4, 9);
        if let Err(error) = self.apply_slow_fan(enabled) {
            return Err(self.rollback_slow_fan(
                previous_mode,
                previous_slow,
                "slow-fan command failed",
                error,
            ));
        }
        let expected = u8::from(enabled);
        let actual = self.wait_for_byte(4, 9, expected, Duration::from_millis(600));
        if actual == Some(expected) {
            return Ok(());
        }
        let rollback = self.rollback_slow_fan_values(previous_mode, previous_slow);
        Err(DchuError::ControlFailed {
            operation: "set slow fan",
            detail: format!("readback expected {expected}, got {actual:?}; {rollback}"),
        })
    }

    /// Enable or disable the Performance-mode Turbo fan composite.
    pub fn set_turbo_fan(&self, enabled: bool) -> Result<()> {
        let previous = self.read_byte(4, 8);
        if let Err(error) = self.apply_turbo_fan(enabled) {
            return Err(self.rollback_turbo_fan(previous, error));
        }
        let expected = u8::from(enabled);
        let actual = self.wait_for_byte(4, 8, expected, Duration::from_millis(600));
        if actual == Some(expected) {
            Ok(())
        } else {
            let rollback = previous.map_or_else(
                || "previous Turbo fan state was unavailable".to_owned(),
                |previous| match self.apply_turbo_fan(previous != 0) {
                    Ok(()) => format!("restored Turbo fan state {previous}"),
                    Err(error) => format!("could not restore Turbo fan state {previous}: {error}"),
                },
            );
            Err(DchuError::ControlFailed {
                operation: "set Turbo fan",
                detail: format!("readback expected {expected}, got {actual:?}; {rollback}"),
            })
        }
    }

    /// Start the vendor anti-dust routine. Completion is reported in page 4,
    /// offset 80 by the signed EC path.
    /// Enable or disable the panel's lowest-refresh-rate mode. CC40 exposes
    /// this only in Power saving and Quiet+ profiles, but the persisted byte
    /// is available in every profile for capability detection.
    pub fn set_low_refresh_rate(&self, enabled: bool) -> Result<()> {
        let supported = self
            .read_byte(1, 27)
            .ok_or_else(|| DchuError::ControlFailed {
                operation: "read low refresh rate support",
                detail: "settings page 1 offset 27 was unavailable".to_owned(),
            })?;
        if supported != 1 {
            return Err(DchuError::ControlFailed {
                operation: "set low refresh rate",
                detail: "the platform reports no low-refresh-rate control".to_owned(),
            });
        }

        let previous = self.read_byte(1, 28);
        let requested = u8::from(enabled);
        self.write_settings(1, 28, &[requested])?;
        let actual = self.read_byte(1, 28);
        if readback_confirmed(requested, actual) {
            return Ok(());
        }

        let rollback = previous.map(|value| match self.write_settings(1, 28, &[value]) {
            Ok(()) => format!("restored low refresh rate state {value}"),
            Err(error) => format!("could not restore low refresh rate state {value}: {error}"),
        });
        Err(DchuError::ReadbackMismatch {
            operation: "set low refresh rate",
            expected: requested,
            actual,
            rollback,
        })
    }

    pub fn run_anti_dust(&self) -> Result<()> {
        self.set_data(121, &[1, 0, 0, 41])?;
        self.write_settings(4, 80, &[1])?;
        let actual = self.wait_for_byte(4, 80, 1, Duration::from_millis(600));
        if actual == Some(1) {
            Ok(())
        } else {
            Err(DchuError::ControlFailed {
                operation: "run anti-dust",
                detail: format!("readback expected 1, got {actual:?}"),
            })
        }
    }

    /// Read CC40's scheduled anti-dust configuration from page 4 offsets
    /// 81..83. The scheduler is stored in the same page as the fan mode.
    pub fn read_anti_dust_schedule(&self) -> Result<AntiDustSchedule> {
        let bytes = self.read_settings(4, 81, 3)?;
        AntiDustSchedule::from_settings(&bytes).ok_or_else(|| DchuError::ControlFailed {
            operation: "read anti-dust schedule",
            detail: format!("response had {} bytes, expected 3", bytes.len()),
        })
    }

    /// Apply CC40's anti-dust schedule. The firmware receives the packed
    /// day/resume/hour value through 121/40 and the UI state is persisted in
    /// page 4 offset 81.
    pub fn set_anti_dust_schedule(&self, schedule: AntiDustSchedule) -> Result<()> {
        schedule.validate()?;
        let previous = self.read_anti_dust_schedule().ok();
        if let Err(error) = self.apply_anti_dust_schedule(schedule) {
            let rollback = previous.map_or_else(
                || "previous anti-dust schedule was unavailable".to_owned(),
                |previous| match self.apply_anti_dust_schedule(previous) {
                    Ok(()) => "restored previous anti-dust schedule".to_owned(),
                    Err(rollback_error) => {
                        format!("could not restore anti-dust schedule: {rollback_error}")
                    }
                },
            );
            return Err(DchuError::ControlFailed {
                operation: "set anti-dust schedule",
                detail: format!("{error}; {rollback}"),
            });
        }
        let expected_day = if schedule.enabled { schedule.day } else { 127 };
        let actual = self.read_anti_dust_schedule()?;
        if actual.enabled == schedule.enabled
            && actual.resume == schedule.resume
            && actual.day == expected_day
            && actual.hour == schedule.hour
        {
            return Ok(());
        }
        let rollback = previous.map_or_else(
            || "previous anti-dust schedule was unavailable".to_owned(),
            |previous| match self.apply_anti_dust_schedule(previous) {
                Ok(()) => "restored previous anti-dust schedule".to_owned(),
                Err(error) => format!("could not restore anti-dust schedule: {error}"),
            },
        );
        Err(DchuError::ControlFailed {
            operation: "set anti-dust schedule",
            detail: format!("readback expected {schedule:?}, got {actual:?}; {rollback}"),
        })
    }

    fn apply_anti_dust_schedule(&self, schedule: AntiDustSchedule) -> Result<()> {
        let value = schedule.command_value();
        self.set_data(
            121,
            &[value as u8, (value >> 8) as u8, (value >> 16) as u8, 40],
        )?;
        self.write_settings(4, 81, &schedule.to_settings())
    }

    /// Set the fan offset shown in CC40. The EC receives a 0..255 value while
    /// AppSettings stores the user-facing percentage.
    pub fn set_fan_offset(&self, percent: u8) -> Result<()> {
        if percent > 100 {
            return Err(DchuError::InvalidArgument(
                "fan offset must be within 0..=100".to_owned(),
            ));
        }
        let previous = self.read_byte(4, 7);
        let raw = ((f32::from(percent) / 100.0) * 255.0).round() as u8;
        self.set_data(121, &[raw, 0, 0, 14])?;
        self.write_settings(4, 7, &[percent])?;
        let actual = self.wait_for_byte(4, 7, percent, Duration::from_millis(600));
        if actual == Some(percent) {
            return Ok(());
        }
        let rollback = previous.map_or_else(
            || "previous fan offset was unavailable".to_owned(),
            |previous| {
                let raw = ((f32::from(previous) / 100.0) * 255.0).round() as u8;
                match self
                    .set_data(121, &[raw, 0, 0, 14])
                    .and_then(|()| self.write_settings(4, 7, &[previous]))
                {
                    Ok(()) => format!("restored fan offset {previous}%"),
                    Err(error) => format!("could not restore fan offset {previous}%: {error}"),
                }
            },
        );
        Err(DchuError::ControlFailed {
            operation: "set fan offset",
            detail: format!("readback expected {percent}, got {actual:?}; {rollback}"),
        })
    }

    fn apply_slow_fan(&self, enabled: bool) -> Result<()> {
        let mode = if enabled {
            FanMode::Noiseless.as_u8()
        } else {
            FanMode::Auto.as_u8()
        };
        self.set_data(121, &[mode, 0, 0, 1])?;
        self.write_settings(4, 9, &[u8::from(enabled)])?;
        self.write_settings(4, 5, &[0])
    }

    fn rollback_slow_fan(
        &self,
        previous_mode: Option<u8>,
        previous_slow: Option<u8>,
        context: &'static str,
        original: DchuError,
    ) -> DchuError {
        let rollback = self.rollback_slow_fan_values(previous_mode, previous_slow);
        DchuError::ControlFailed {
            operation: context,
            detail: format!("{original}; {rollback}"),
        }
    }

    fn rollback_slow_fan_values(
        &self,
        previous_mode: Option<u8>,
        previous_slow: Option<u8>,
    ) -> String {
        let Some(mode) = previous_mode else {
            return "previous fan mode was unavailable".to_owned();
        };
        let Some(slow) = previous_slow else {
            return "previous slow-fan state was unavailable".to_owned();
        };
        match self
            .set_data(121, &[mode, 0, 0, 1])
            .and_then(|()| self.write_settings(4, 9, &[slow]))
            .and_then(|()| self.write_settings(4, 5, &[mode]))
        {
            Ok(()) => format!("restored fan mode {mode} and slow-fan state {slow}"),
            Err(error) => format!("could not restore slow fan: {error}"),
        }
    }

    fn apply_turbo_fan(&self, enabled: bool) -> Result<()> {
        self.write_settings(4, 8, &[u8::from(enabled)])?;
        self.write_settings(1, 32, &[0])?;
        let data = 2_u8 | (u8::from(enabled) << 6);
        self.set_data(121, &[data, 0, 0, 25])?;
        self.write_settings(4, 5, &[0])?;
        // 1096 is the vendor's encoded AppSettings key: page 4 * 256 + 72.
        // The old 4/40 write was an unrelated field and could leave the Turbo
        // fan request only half-applied.
        self.write_settings(4, 72, &[0])?;
        if enabled {
            self.set_data(121, &[0, 0, 0, 1])?;
        }
        Ok(())
    }

    fn rollback_turbo_fan(&self, previous: Option<u8>, original: DchuError) -> DchuError {
        let rollback = previous.map_or_else(
            || "previous Turbo fan state was unavailable".to_owned(),
            |previous| match self.apply_turbo_fan(previous != 0) {
                Ok(()) => format!("restored Turbo fan state {previous}"),
                Err(error) => format!("could not restore Turbo fan state {previous}: {error}"),
            },
        );
        DchuError::ControlFailed {
            operation: "Turbo fan command failed",
            detail: format!("{original}; {rollback}"),
        }
    }

    /// Read the EC's runtime four-point fan table via DCHU command 13.
    pub fn read_fan_curve(&self) -> Result<FanCurveState> {
        let (status, buffer) = self.raw_read_buffer(13);
        status_non_negative("ACPI command 13", status)?;
        Ok(decode_fan_curve(&buffer))
    }

    /// Read CC40's RPM-based ten-point fan tables. This is the active custom
    /// fan path when the platform advertises RPM fan control.
    pub fn read_rpm_fan_table(&self) -> Result<RpmFanTable> {
        let fan12_raw = self.read_wmi_subcommand(40)?;
        let fan_exist_mask = self.acpi_read_buffer(13)?[50];
        let mut table = RpmFanTable {
            channels: [RpmFanChannel::default(); 4],
            fan12_raw,
            fan34_raw: [0_u8; 256],
        };
        for (channel_index, offset) in [(0_usize, 0_usize), (1, 128)] {
            let mut raw = [0_u8; 112];
            raw.copy_from_slice(&fan12_raw[offset..offset + 112]);
            table.channels[channel_index] =
                decode_rpm_fan_channel(&raw, fan_exist_mask & (1 << channel_index) != 0);
        }
        if fan_exist_mask & 0x0c != 0 {
            let fan34_raw = self.read_wmi_subcommand(42)?;
            table.fan34_raw = fan34_raw;
            for (channel_index, offset) in [(2_usize, 0_usize), (3, 128)] {
                let mut raw = [0_u8; 112];
                raw.copy_from_slice(&fan34_raw[offset..offset + 112]);
                table.channels[channel_index] =
                    decode_rpm_fan_channel(&raw, fan_exist_mask & (1 << channel_index) != 0);
            }
        }
        Ok(table)
    }

    /// Write a previously captured RPM fan table without changing fan mode.
    pub fn write_rpm_fan_table(&self, table: &RpmFanTable) -> Result<()> {
        for channel in &table.channels {
            crate::validate_rpm_fan_channel(channel)?;
        }
        let mut fan12 = table.fan12_raw;
        fan12[0] = 41;
        for (channel_index, offset) in [(0_usize, 16_usize), (1, 128)] {
            if table.channels[channel_index].present {
                fan12[offset..offset + 112]
                    .copy_from_slice(&encode_rpm_fan_channel(&table.channels[channel_index]));
            }
        }
        self.set_data_ex(4, &fan12, &mut [0_u8; 256])?;

        if table.channels[2].present || table.channels[3].present {
            let mut fan34 = table.fan34_raw;
            fan34[0] = 43;
            for (channel_index, offset) in [(2_usize, 16_usize), (3, 128)] {
                if table.channels[channel_index].present {
                    fan34[offset..offset + 112]
                        .copy_from_slice(&encode_rpm_fan_channel(&table.channels[channel_index]));
                }
            }
            self.set_data_ex(4, &fan34, &mut [0_u8; 256])?;
        }

        thread::sleep(Duration::from_millis(120));
        let actual = self.read_rpm_fan_table()?;
        for index in 0..4 {
            if table.channels[index].present
                && !rpm_fan_channel_matches(&table.channels[index], &actual.channels[index])
            {
                return Err(DchuError::ControlFailed {
                    operation: "verify RPM fan table",
                    detail: format!("fan channel {index} did not match the requested table"),
                });
            }
        }
        Ok(())
    }

    /// Write a custom ten-point RPM fan table, select Custom mode, and verify
    /// both the fan table and the persisted mode. The previous table is restored
    /// if the selected mode does not read back.
    pub fn set_custom_rpm_fan_table(&self, table: &RpmFanTable) -> Result<()> {
        let previous = self.read_rpm_fan_table()?;
        let previous_mode = self.read_byte(4, 5);
        if let Err(error) = self.write_rpm_fan_table(table) {
            return Err(DchuError::ControlFailed {
                operation: "apply RPM fan table",
                detail: error.to_string(),
            });
        }
        if let Err(error) = self.apply_fan_mode_value(FanMode::Custom.as_u8()) {
            let rollback = match self.write_rpm_fan_table(&previous) {
                Ok(()) => "restored previous RPM fan table".to_owned(),
                Err(error) => format!("could not restore previous RPM fan table: {error}"),
            };
            return Err(DchuError::ControlFailed {
                operation: "select RPM custom fan mode",
                detail: format!("{error}; {rollback}"),
            });
        }
        let mode = self.wait_for_byte(4, 5, FanMode::Custom.as_u8(), Duration::from_millis(700));
        if mode == Some(FanMode::Custom.as_u8()) {
            return Ok(());
        }
        let rollback = match self.write_rpm_fan_table(&previous) {
            Ok(()) => self.rollback_fan_curve_mode(previous_mode).to_string(),
            Err(error) => format!("could not restore previous RPM fan table: {error}"),
        };
        Err(DchuError::ControlFailed {
            operation: "select RPM custom fan mode",
            detail: format!("mode readback expected 6, got {mode:?}; {rollback}"),
        })
    }

    fn read_wmi_subcommand(&self, subcommand: u8) -> Result<[u8; 256]> {
        let mut output = [0_u8; 256];
        self.set_data_ex(4, &[subcommand], &mut output)?;
        Ok(output)
    }

    /// Recovery/diagnostic path: write a previously captured runtime table back
    /// to block 14 without changing the active fan mode.
    pub fn write_fan_curve_table(&self, curve: &FanCurveState) -> Result<()> {
        let frame = encode_fan_curve_frame(curve);
        let status = self.set_data_ex_status(14, &frame, &mut [0_u8; 256])?;
        if status != 14 {
            return Err(DchuError::Status {
                operation: "ACPI command 14",
                code: status,
            });
        }
        Ok(())
    }

    /// Write customized middle nodes to block 14, switch to Custom mode, and
    /// verify against block 13, which is the table the EC is actually running.
    pub fn set_custom_fan_curve(&self, curve: CustomFanCurve) -> Result<()> {
        let previous_curve = self.read_fan_curve()?;
        let previous_mode = self.read_byte(4, 5);
        let target_curve = curve_with_custom_nodes(&previous_curve, &curve)?;
        self.write_fan_curve_table(&target_curve)?;

        if let Err(error) = self.apply_fan_mode_value(6) {
            let rollback = self.restore_previous_fan_curve(&previous_curve, previous_mode);
            return Err(DchuError::ControlFailed {
                operation: "apply custom fan curve",
                detail: format!("{error}; {rollback}"),
            });
        }

        let mode_readback = self.wait_for_byte(4, 5, 6, Duration::from_millis(600));
        if !readback_confirmed(6, mode_readback) {
            let rollback = self.restore_previous_fan_curve(&previous_curve, previous_mode);
            return Err(DchuError::ReadbackMismatch {
                operation: "select custom fan mode",
                expected: 6,
                actual: mode_readback,
                rollback: Some(rollback),
            });
        }

        thread::sleep(Duration::from_millis(450));
        let actual_curve = self.read_fan_curve()?;
        if fan_curve_matches(&target_curve, &actual_curve) {
            return Ok(());
        }

        let rollback = self.restore_previous_fan_curve(&previous_curve, previous_mode);
        Err(DchuError::ControlFailed {
            operation: "verify custom fan curve",
            detail: format!(
                "block-13 runtime table did not match the requested middle nodes; {rollback}"
            ),
        })
    }

    pub(super) fn apply_fan_mode_value(&self, value: u8) -> Result<()> {
        self.set_data(121, &[value, 0, 0, 1])?;
        self.write_settings(4, 5, &[value])
    }

    fn set_data_ex_status(&self, command: i32, input: &[u8], output: &mut [u8]) -> Result<i32> {
        self.acpi_ioctl(command, input, output)?;
        Ok(command)
    }

    fn rollback_fan_curve_mode(&self, previous_mode: Option<u8>) -> String {
        let mode = previous_mode
            .and_then(FanMode::from_u8)
            .unwrap_or(FanMode::Auto);
        match self.apply_fan_mode(mode) {
            Ok(()) => format!("restored fan mode {}", mode.label()),
            Err(error) => format!("could not restore fan mode {}: {error}", mode.label()),
        }
    }

    fn restore_previous_fan_curve(
        &self,
        previous: &FanCurveState,
        previous_mode: Option<u8>,
    ) -> String {
        if let Err(error) = self.write_fan_curve_table(previous) {
            return format!("could not restore previous fan curve: {error}");
        }
        self.rollback_fan_curve_mode(previous_mode)
    }
}
