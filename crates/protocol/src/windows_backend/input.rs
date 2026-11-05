use super::Dchu;
use crate::{DchuError, Result};

impl Dchu {
    /// Notify the vendor EC path that the Windows radio state changed. The
    /// actual radio switch is performed through the Windows Radio API by the
    /// application layer, so no vendor HID helper DLL is loaded.
    pub fn notify_airplane_mode(&self, enabled: bool) -> Result<()> {
        let value = u8::from(enabled);
        self.set_data(121, &[value, 0, 0, 26])?;
        self.set_data(109, &[0, 0, 0, 0])
    }

    pub fn read_camera_enabled(&self) -> Option<bool> {
        self.read_byte(1, 6).map(|value| value != 0)
    }

    pub fn set_camera_enabled(&self, enabled: bool) -> Result<()> {
        let value = u8::from(enabled);
        let previous = self.read_camera_enabled();
        self.write_settings(1, 6, &[value])?;
        self.set_data(34, &[value, 0, 0, 0])?;
        let actual = self.read_camera_enabled();
        if actual == Some(enabled) {
            return Ok(());
        }
        let rollback = previous.map_or_else(
            || "previous camera state was unavailable".to_owned(),
            |previous| match self
                .write_settings(1, 6, &[u8::from(previous)])
                .and_then(|()| self.set_data(34, &[u8::from(previous), 0, 0, 0]))
            {
                Ok(()) => format!("restored camera state {previous}"),
                Err(error) => format!("could not restore camera state {previous}: {error}"),
            },
        );
        Err(DchuError::ControlFailed {
            operation: "set camera state",
            detail: format!("readback expected {enabled}, got {actual:?}; {rollback}"),
        })
    }

    pub fn read_touchpad_enabled(&self) -> Option<bool> {
        self.read_byte(1, 10).map(|value| value != 0)
    }

    /// Persist the touchpad state after the caller sends the vendor's
    /// Ctrl+Win+F24 chord. This keeps the setting page as the authoritative
    /// readback source used by CC40.
    pub fn set_touchpad_enabled(&self, enabled: bool) -> Result<()> {
        let value = u8::from(enabled);
        let previous = self.read_touchpad_enabled();
        self.write_settings(1, 10, &[value])?;
        let actual = self.read_touchpad_enabled();
        if actual == Some(enabled) {
            return Ok(());
        }
        let rollback = previous.map_or_else(
            || "previous touchpad state was unavailable".to_owned(),
            |previous| match self.write_settings(1, 10, &[u8::from(previous)]) {
                Ok(()) => format!("restored touchpad state {previous}"),
                Err(error) => format!("could not restore touchpad state {previous}: {error}"),
            },
        );
        Err(DchuError::ControlFailed {
            operation: "set touchpad state",
            detail: format!("readback expected {enabled}, got {actual:?}; {rollback}"),
        })
    }
}
