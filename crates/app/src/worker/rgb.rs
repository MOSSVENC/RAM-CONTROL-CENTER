use neo_control_protocol::{Dchu, RgbColor};

use super::CommandOutcome;
use crate::localization::tr;

pub(super) fn apply_brightness(dchu: Option<&Dchu>, level: u8) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_rgb_keyboard_brightness(level),
        tr("Keyboard brightness applied.", "键盘亮度已应用。"),
    )
}

pub(super) fn apply_color(dchu: Option<&Dchu>, color: RgbColor) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_rgb_keyboard_color(color),
        tr("Keyboard color applied.", "键盘颜色已应用。"),
    )
}

pub(super) fn apply_effect(dchu: Option<&Dchu>, effect: u8) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_rgb_keyboard_effect(effect),
        tr("Keyboard effect applied.", "键盘灯效已应用。"),
    )
}

pub(super) fn apply_sleep_timer(
    dchu: Option<&Dchu>,
    enabled: bool,
    seconds: u32,
) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_rgb_keyboard_sleep_timer(enabled, seconds),
        tr("Keyboard sleep timer applied.", "键盘休眠计时已应用。"),
    )
}

pub(super) fn apply_boot_effect(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_rgb_keyboard_boot_effect(enabled),
        tr("Keyboard boot effect applied.", "键盘开机灯效已应用。"),
    )
}
