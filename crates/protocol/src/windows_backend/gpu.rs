use std::thread;
use std::time::{Duration, Instant};

use super::Dchu;
use crate::{DchuError, GpuSwitchMode, GpuSwitchState, Result};

impl Dchu {
    pub fn read_gpu_switch(&self) -> Result<GpuSwitchState> {
        // The ACPI response can carry a full 256-byte descriptor. Always use a
        // full-size landing buffer even when only the first few bytes matter.
        let mut output = [0_u8; 256];
        self.set_data_ex(4, &[21], &mut output)?;
        Ok(GpuSwitchState {
            current_mode: output[0],
            supported_mask: output[1],
        })
    }

    pub fn set_gpu_switch(&self, mode: GpuSwitchMode) -> Result<()> {
        if mode == GpuSwitchMode::Integrated {
            return Err(DchuError::InvalidArgument(
                "Neo Control does not allow leaving the discrete GPU disabled".to_owned(),
            ));
        }

        let previous = self.read_gpu_switch()?;
        if !previous.supports(mode) {
            return Err(DchuError::InvalidArgument(format!(
                "{} is not supported by mask 0x{:02X}",
                mode.label(),
                previous.supported_mask
            )));
        }
        if previous.current_mode == mode.as_u8() {
            return Ok(());
        }

        self.apply_gpu_switch(mode.as_u8())?;
        let actual = self.wait_for_gpu_switch(mode.as_u8(), Duration::from_millis(1200));
        if actual == Some(mode.as_u8()) {
            return Ok(());
        }

        let rollback = self.apply_gpu_switch(previous.current_mode).map_or_else(
            |error| {
                format!(
                    "could not restore GPU mode {}: {error}",
                    previous.current_mode
                )
            },
            |()| format!("restored GPU mode {}", previous.current_mode),
        );
        Err(DchuError::ControlFailed {
            operation: "set GPU switch",
            detail: format!("expected mode {}, got {actual:?}; {rollback}", mode.as_u8()),
        })
    }

    /// Stage a GPU mode exactly as the vendor pre-boot control does. This
    /// value is consumed by the BIOS at POST, so there is intentionally no
    /// online readback. It must only be called after an explicit user choice.
    pub fn stage_gpu_switch(&self, mode: GpuSwitchMode) -> Result<()> {
        let mut output = [0_u8; 256];
        self.set_data_ex(4, &[22, mode.as_u8()], &mut output)
    }

    fn apply_gpu_switch(&self, mode: u8) -> Result<()> {
        // Match CC40's wrapper call exactly: command 4 with a two-byte input
        // array and a full 256-byte response landing buffer. A same-mode write
        // is rejected by the caller before reaching this function.
        let mut output = [0_u8; 256];
        self.set_data_ex(4, &[22, mode], &mut output)
    }

    fn wait_for_gpu_switch(&self, expected: u8, timeout: Duration) -> Option<u8> {
        let started = Instant::now();
        let mut last = None;
        while started.elapsed() <= timeout {
            if let Ok(state) = self.read_gpu_switch() {
                last = Some(state.current_mode);
                if state.current_mode == expected {
                    return last;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        last
    }
}
