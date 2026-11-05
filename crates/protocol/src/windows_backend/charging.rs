use std::thread;
use std::time::Duration;

use super::Dchu;
use crate::{DchuError, FlexiChargerMode, FlexiChargerState, Result};

impl Dchu {
    pub fn read_flexi_charger(&self) -> Result<FlexiChargerState> {
        let mut output = [0_u8; 256];
        self.set_data_ex(4, &[30], &mut output)?;
        let threshold_choices = output[16..48]
            .iter()
            .copied()
            .filter(|value| (1..=100).contains(value))
            .collect();
        Ok(FlexiChargerState {
            mode: output[0],
            start_threshold: output[1],
            stop_threshold: output[2],
            threshold_choices,
        })
    }

    pub fn set_flexi_charger(
        &self,
        mode: FlexiChargerMode,
        start_threshold: u8,
        stop_threshold: u8,
    ) -> Result<()> {
        if !(1..=100).contains(&start_threshold) || !(1..=100).contains(&stop_threshold) {
            return Err(DchuError::InvalidArgument(
                "FlexiCharger thresholds must be within 1..=100".to_owned(),
            ));
        }
        if mode != FlexiChargerMode::Disabled && start_threshold >= stop_threshold {
            return Err(DchuError::InvalidArgument(
                "the start threshold must be lower than the stop threshold".to_owned(),
            ));
        }

        let previous = self.read_flexi_charger()?;
        if let Err(error) =
            self.apply_flexi_charger_values(mode.as_u8(), start_threshold, stop_threshold)
        {
            let rollback = self.rollback_flexi_charger(&previous);
            return Err(DchuError::ControlFailed {
                operation: "set FlexiCharger",
                detail: format!("{error}; {rollback}"),
            });
        }

        let persisted = self.wait_for_byte(1, 46, mode.as_u8(), Duration::from_millis(800));
        if persisted != Some(mode.as_u8()) {
            let rollback = self.rollback_flexi_charger(&previous);
            return Err(DchuError::ControlFailed {
                operation: "set FlexiCharger",
                detail: format!(
                    "persisted mode readback expected {}, got {persisted:?}; {rollback}",
                    mode.as_u8()
                ),
            });
        }

        thread::sleep(Duration::from_millis(100));
        match self.read_flexi_charger() {
            Ok(actual)
                if actual.mode == mode.as_u8()
                    && actual.start_threshold == start_threshold
                    && actual.stop_threshold == stop_threshold =>
            {
                Ok(())
            }
            Ok(actual) => {
                let rollback = self.rollback_flexi_charger(&previous);
                Err(DchuError::ControlFailed {
                    operation: "set FlexiCharger",
                    detail: format!(
                        "live readback expected mode {} at {}–{}%, got mode {} at {}–{}%; {rollback}",
                        mode.as_u8(),
                        start_threshold,
                        stop_threshold,
                        actual.mode,
                        actual.start_threshold,
                        actual.stop_threshold
                    ),
                })
            }
            Err(error) => {
                let rollback = self.rollback_flexi_charger(&previous);
                Err(DchuError::ControlFailed {
                    operation: "set FlexiCharger",
                    detail: format!("live readback failed: {error}; {rollback}"),
                })
            }
        }
    }

    /// Recover the known-safe disabled preset without first reading the vendor
    /// state. This is used only after an interrupted/malformed experimental
    /// write made SetDCHU_DataEx(4,[30]) fault. It does not touch firmware.
    pub fn recover_flexi_charger_disabled(&self) -> Result<()> {
        self.apply_flexi_charger_values(0, 95, 100)?;
        let persisted = self.wait_for_byte(1, 46, 0, Duration::from_millis(800));
        if persisted == Some(0) {
            Ok(())
        } else {
            Err(DchuError::ControlFailed {
                operation: "recover FlexiCharger",
                detail: format!("persisted mode readback expected 0, got {persisted:?}"),
            })
        }
    }

    fn apply_flexi_charger_values(
        &self,
        mode: u8,
        start_threshold: u8,
        stop_threshold: u8,
    ) -> Result<()> {
        let enabled = u8::from(mode != 0);
        let mut output = [0_u8; 256];
        self.set_data_ex(
            4,
            &[31, enabled, start_threshold, stop_threshold],
            &mut output,
        )?;
        self.write_settings(1, 46, &[mode])
    }

    fn rollback_flexi_charger(&self, previous: &FlexiChargerState) -> String {
        let Some(mode) = FlexiChargerMode::from_u8(previous.mode) else {
            return format!("previous FlexiCharger mode {} is unknown", previous.mode);
        };
        match self.apply_flexi_charger_values(
            mode.as_u8(),
            previous.start_threshold,
            previous.stop_threshold,
        ) {
            Ok(()) => format!(
                "restored FlexiCharger mode {} at {}–{}%",
                mode.as_u8(),
                previous.start_threshold,
                previous.stop_threshold
            ),
            Err(error) => format!("could not restore FlexiCharger: {error}"),
        }
    }
}
