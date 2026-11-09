use neo_control_protocol::{Dchu, QuickToggle};

use super::CommandOutcome;
use crate::input::{send_touchpad_chord, send_virtual_key};
use crate::localization::{localized_toggle_label, tr};
use crate::system_controls;

pub(super) fn apply_camera(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_camera_enabled(enabled),
        if enabled {
            tr("Camera enabled.")
        } else {
            tr("Camera disabled.")
        },
    )
}

pub(super) fn apply_touchpad(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    let input = send_touchpad_chord();
    let persisted = active.set_touchpad_enabled(enabled);
    match (input, persisted) {
        (Ok(()), Ok(())) => CommandOutcome::success(if enabled {
            tr("Touchpad enabled.")
        } else {
            tr("Touchpad disabled.")
        }),
        (input, persisted) => CommandOutcome::failure(format!(
            "{}; {}",
            input.err().unwrap_or_else(|| "input path ready".to_owned()),
            persisted
                .err()
                .map(|error| error.to_string())
                .unwrap_or_else(|| "setting persisted".to_owned())
        )),
    }
}

pub(super) fn apply_airplane(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    match system_controls::set_airplane_mode(enabled) {
        Ok(()) => {
            if let Some(active) = dchu {
                let _ = active.notify_airplane_mode(enabled);
            }
            CommandOutcome::success(if enabled {
                tr("Airplane mode enabled.")
            } else {
                tr("Airplane mode disabled.")
            })
        }
        Err(error) => CommandOutcome::failure(error),
    }
}

pub(super) fn apply_quick_toggle(
    dchu: Option<&Dchu>,
    toggle: QuickToggle,
    enabled: bool,
) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_quick_toggle(toggle, enabled),
        format!(
            "{} {}{}",
            localized_toggle_label(toggle),
            tr("set to "),
            if enabled {
                tr("enabled")
            } else {
                tr("disabled")
            }
        ),
    )
}

pub(super) fn apply_lock_key(vk: u8) -> CommandOutcome {
    CommandOutcome::from_result(send_virtual_key(vk), tr("Lock key toggled."))
}
