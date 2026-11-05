use super::Dchu;
use crate::{DchuError, Result, RgbColor, RgbKeyboardState, rgb_part_value, rgb_subcommand_packet};

impl Dchu {
    pub fn read_rgb_keyboard_state(&self) -> Result<RgbKeyboardState> {
        let store = self.read_settings_store()?;
        let base = 2 * 256;
        let sleep_seconds = u32::from(store[base + 37]) * 3600
            + u32::from(store[base + 38]) * 60
            + u32::from(store[base + 39]);
        Ok(RgbKeyboardState {
            boot_effect: store[base + 7] != 0,
            effect: store[base + 32],
            brightness: store[base + 35],
            enabled: store[base + 80] != 0,
            color: RgbColor {
                red: store[base + 81],
                green: store[base + 82],
                blue: store[base + 83],
            },
            sleep_enabled: store[base + 36] != 0,
            sleep_seconds,
        })
    }

    /// Set the 15-color RGB keyboard brightness. Level 0 follows the vendor
    /// behavior: the stored brightness is retained while the keyboard is off.
    pub fn set_rgb_keyboard_brightness(&self, level: u8) -> Result<()> {
        if level > 4 {
            return Err(DchuError::InvalidArgument(
                "keyboard brightness must be within 0..=4".to_owned(),
            ));
        }
        let previous = self.read_rgb_keyboard_state()?;
        let stored = if level == 0 {
            previous.brightness
        } else {
            level
        };
        self.apply_rgb_keyboard_brightness(level, stored, level != 0)?;
        let actual = self.read_rgb_keyboard_state()?;
        if actual.brightness == stored && actual.enabled == (level != 0) {
            return Ok(());
        }
        let rollback = self
            .apply_rgb_keyboard_brightness(
                if previous.enabled {
                    previous.brightness
                } else {
                    0
                },
                previous.brightness,
                previous.enabled,
            )
            .map_or_else(
                |error| format!("could not restore keyboard brightness: {error}"),
                |()| "restored keyboard brightness".to_owned(),
            );
        Err(DchuError::ControlFailed {
            operation: "set keyboard brightness",
            detail: format!(
                "readback expected brightness {stored} and enabled={}, got brightness {} and enabled={}; {rollback}",
                level != 0,
                actual.brightness,
                actual.enabled
            ),
        })
    }

    pub fn set_rgb_keyboard_color(&self, color: RgbColor) -> Result<()> {
        let previous = self.read_rgb_keyboard_state()?;
        self.apply_rgb_keyboard_color(color)?;
        let actual = self.read_rgb_keyboard_state()?;
        if actual.color == color && actual.effect == 8 {
            return Ok(());
        }
        let rollback = self.apply_rgb_keyboard_color(previous.color).map_or_else(
            |error| format!("could not restore keyboard color: {error}"),
            |()| "restored keyboard color".to_owned(),
        );
        Err(DchuError::ControlFailed {
            operation: "set keyboard color",
            detail: format!(
                "readback expected {:?}, got {:?}; {rollback}",
                color, actual.color
            ),
        })
    }

    /// Effects confirmed by the vendor: 8 static, 2 breathe, 3 cycle.
    pub fn set_rgb_keyboard_effect(&self, effect: u8) -> Result<()> {
        if !matches!(effect, 2 | 3 | 8) {
            return Err(DchuError::InvalidArgument(
                "keyboard effect must be 2 (breathe), 3 (cycle), or 8 (static)".to_owned(),
            ));
        }
        let previous = self.read_rgb_keyboard_state()?;
        self.apply_rgb_keyboard_effect(effect)?;
        let actual = self.read_rgb_keyboard_state()?;
        if actual.effect == effect {
            return Ok(());
        }
        let rollback = self.apply_rgb_keyboard_effect(previous.effect).map_or_else(
            |error| format!("could not restore keyboard effect: {error}"),
            |()| "restored keyboard effect".to_owned(),
        );
        Err(DchuError::ControlFailed {
            operation: "set keyboard effect",
            detail: format!(
                "readback expected effect {effect}, got {}; {rollback}",
                actual.effect
            ),
        })
    }

    pub fn set_rgb_keyboard_sleep_timer(&self, enabled: bool, seconds: u32) -> Result<()> {
        if seconds > 86_399 {
            return Err(DchuError::InvalidArgument(
                "keyboard sleep timer must be within 24 hours".to_owned(),
            ));
        }
        let previous = self.read_rgb_keyboard_state()?;
        let stored_seconds = if enabled {
            seconds.max(5)
        } else {
            previous.sleep_seconds
        };
        self.apply_rgb_keyboard_sleep_timer(enabled, stored_seconds)?;
        let actual = self.read_rgb_keyboard_state()?;
        if actual.sleep_enabled == enabled && actual.sleep_seconds == stored_seconds {
            return Ok(());
        }
        let rollback = self
            .apply_rgb_keyboard_sleep_timer(previous.sleep_enabled, previous.sleep_seconds)
            .map_or_else(
                |error| format!("could not restore keyboard sleep timer: {error}"),
                |()| "restored keyboard sleep timer".to_owned(),
            );
        Err(DchuError::ControlFailed {
            operation: "set keyboard sleep timer",
            detail: format!(
                "readback expected enabled={enabled} seconds={stored_seconds}, got enabled={} seconds={}; {rollback}",
                actual.sleep_enabled, actual.sleep_seconds
            ),
        })
    }

    pub fn set_rgb_keyboard_boot_effect(&self, enabled: bool) -> Result<()> {
        let previous = self.read_rgb_keyboard_state()?;
        self.apply_rgb_keyboard_boot_effect(enabled)?;
        let actual = self.read_rgb_keyboard_state()?;
        if actual.boot_effect == enabled {
            return Ok(());
        }
        let rollback = self
            .apply_rgb_keyboard_boot_effect(previous.boot_effect)
            .map_or_else(
                |error| format!("could not restore keyboard boot effect: {error}"),
                |()| "restored keyboard boot effect".to_owned(),
            );
        Err(DchuError::ControlFailed {
            operation: "set keyboard boot effect",
            detail: format!(
                "readback expected enabled={enabled}, got {}; {rollback}",
                actual.boot_effect
            ),
        })
    }

    fn apply_rgb_keyboard_brightness(&self, level: u8, stored: u8, enabled: bool) -> Result<()> {
        let kb_type = self.acpi_read_buffer(13)?[15];
        let raw = match level {
            0 => 0,
            1 => 47,
            2 if kb_type == 38 => 102,
            2 => 95,
            3 if kb_type == 38 => 155,
            3 => 143,
            4 if kb_type == 38 => 212,
            4 => 191,
            _ => 0,
        };
        self.set_data(103, &rgb_subcommand_packet(244, raw))?;
        self.write_settings(2, 35, &[stored])?;
        self.write_settings(2, 80, &[u8::from(enabled)])
    }

    fn apply_rgb_keyboard_color(&self, color: RgbColor) -> Result<()> {
        let mut output = [0_u8; 256];
        self.set_data_ex(4, &[63, 0], &mut output)?;
        let kb_type = self.acpi_read_buffer(13)?[15];
        let value = rgb_part_value(kb_type, color);
        for subcommand in [240_u8, 241, 242] {
            self.set_data(103, &rgb_subcommand_packet(subcommand, value))?;
        }
        self.write_settings(2, 81, &[color.red, color.green, color.blue])?;
        self.write_settings(2, 32, &[8])
    }

    fn apply_rgb_keyboard_effect(&self, effect: u8) -> Result<()> {
        let hardware_effect = match effect {
            2 => 1,
            3 => 2,
            8 => 0,
            _ => {
                return Err(DchuError::InvalidArgument(
                    "keyboard effect must be 2, 3, or 8".to_owned(),
                ));
            }
        };
        let mut output = [0_u8; 256];
        self.set_data_ex(4, &[63, hardware_effect], &mut output)?;
        self.write_settings(2, 32, &[effect])
    }

    fn apply_rgb_keyboard_sleep_timer(&self, enabled: bool, seconds: u32) -> Result<()> {
        let data = if enabled { (seconds << 8) | 0xff } else { 0 };
        let mut packet = data.to_le_bytes();
        packet[3] = 24;
        self.set_data(121, &packet)?;
        let hours = (seconds / 3600) as u8;
        let minutes = ((seconds / 60) % 60) as u8;
        let secs = (seconds % 60) as u8;
        self.write_settings(2, 36, &[u8::from(enabled)])?;
        self.write_settings(2, 37, &[hours, minutes, secs])
    }

    fn apply_rgb_keyboard_boot_effect(&self, enabled: bool) -> Result<()> {
        let value = u8::from(enabled);
        self.set_data(121, &[value, 0, 0, 30])?;
        self.write_settings(2, 7, &[value])
    }
}
