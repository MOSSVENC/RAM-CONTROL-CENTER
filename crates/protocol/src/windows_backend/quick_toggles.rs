use std::thread;
use std::time::{Duration, Instant};

use super::{Dchu, decode_toggle_byte};
use crate::{DchuError, QuickToggle, QuickToggleState, Result};

impl Dchu {
    pub fn read_quick_toggles(&self) -> QuickToggleState {
        QuickToggleState {
            fn_lock: self
                .read_byte(1, QuickToggle::FnLock.settings_offset())
                .and_then(|raw| decode_toggle_byte(QuickToggle::FnLock, raw)),
            win_lock: self
                .read_byte(1, QuickToggle::WinLock.settings_offset())
                .and_then(|raw| decode_toggle_byte(QuickToggle::WinLock, raw)),
            swap_win_fn: self
                .read_byte(1, QuickToggle::SwapWinFn.settings_offset())
                .and_then(|raw| decode_toggle_byte(QuickToggle::SwapWinFn, raw)),
            hotkeys_control_center: self
                .read_byte(1, QuickToggle::HotkeysControlCenter.settings_offset())
                .and_then(|raw| decode_toggle_byte(QuickToggle::HotkeysControlCenter, raw)),
            copilot: self
                .read_byte(1, QuickToggle::Copilot.settings_offset())
                .and_then(|raw| decode_toggle_byte(QuickToggle::Copilot, raw)),
        }
    }

    pub fn set_quick_toggle(&self, toggle: QuickToggle, enabled: bool) -> Result<()> {
        let previous = self.read_quick_toggles();
        // CC40 treats vendor value 0 as an uninitialized preference group and
        // builds the first write from its unchecked defaults. Match that path:
        // an explicit user click initializes the full five-field group.
        let previous_raw = previous.raw_values();
        let baseline_raw = previous_raw.unwrap_or_else(|| previous.raw_values_or_defaults());
        let mut target = QuickToggleState::from_raw_values(baseline_raw);
        target.set_value(toggle, enabled);
        let target_raw = target.raw_values().ok_or_else(|| {
            DchuError::InvalidArgument("the target quick-toggle state is incomplete".to_owned())
        })?;

        if let Err(error) = self.apply_quick_toggle_values(target_raw) {
            let rollback = self.rollback_quick_toggles(baseline_raw);
            return Err(DchuError::ControlFailed {
                operation: "set quick toggle",
                detail: format!("{error}; {rollback}"),
            });
        }

        let actual = self.wait_for_quick_toggle_values(target_raw, Duration::from_millis(700));
        if actual == Some(target_raw) {
            return Ok(());
        }
        let rollback = self.rollback_quick_toggles(baseline_raw);
        Err(DchuError::QuickToggleMismatch {
            expected: target_raw,
            actual,
            rollback: Some(rollback),
        })
    }

    fn apply_quick_toggle_values(&self, values: [u8; 5]) -> Result<()> {
        self.set_data(
            4,
            &[25, values[0], values[1], values[2], values[3], values[4]],
        )?;
        self.write_settings(1, 36, &values[0..3])?;
        self.write_settings(1, 43, &values[3..4])?;
        self.write_settings(1, 44, &values[4..5])
    }

    fn wait_for_quick_toggle_values(
        &self,
        expected: [u8; 5],
        timeout: Duration,
    ) -> Option<[u8; 5]> {
        let started = Instant::now();
        let mut last = None;
        while started.elapsed() <= timeout {
            let actual = self.read_quick_toggles().raw_values();
            last = actual;
            if actual == Some(expected) {
                return actual;
            }
            thread::sleep(Duration::from_millis(50));
        }
        last
    }

    fn rollback_quick_toggles(&self, previous: [u8; 5]) -> String {
        match self.apply_quick_toggle_values(previous) {
            Ok(()) => format!("restored quick-toggle state {previous:?}"),
            Err(error) => format!("could not restore quick-toggle state {previous:?}: {error}"),
        }
    }
}
