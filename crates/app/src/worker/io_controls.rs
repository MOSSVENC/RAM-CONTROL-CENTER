use neo_control_protocol::Dchu;

use super::CommandOutcome;
use crate::display_care::{display_care_offset, restore_display_toggle};
use crate::localization::tr;
use crate::system_controls;

pub(super) fn apply_display_toggle(
    dchu: Option<&Dchu>,
    toggle: u8,
    enabled: bool,
) -> CommandOutcome {
    let previous = match system_controls::display_care_state() {
        Ok(state) => state,
        Err(error) => {
            return CommandOutcome::failure(error).without_selection_sync();
        }
    };
    let result = match toggle {
        0 => system_controls::set_taskbar_auto_hide(enabled),
        1 => system_controls::set_taskbar_transparency(enabled),
        2 => system_controls::set_dark_mode(enabled),
        _ => Err("unknown display-care toggle".to_owned()),
    };
    match result {
        Ok(()) => {
            if let (Some(active), Some(offset)) = (dchu, display_care_offset(toggle)) {
                match active.write_setting_byte(0, offset, u8::from(enabled)) {
                    Ok(()) => CommandOutcome::success(tr(
                        "Display protection applied and read back.",
                        "显示保护设置已应用并读回。",
                    )),
                    Err(error) => {
                        let rollback = restore_display_toggle(toggle, &previous).map_or_else(
                            |error| format!("; rollback failed: {error}"),
                            |()| "; restored previous Windows state".to_owned(),
                        );
                        CommandOutcome::failure(format!(
                            "{}: {error}{rollback}",
                            tr("DCHU preference write failed", "DCHU 偏好写入失败")
                        ))
                    }
                }
            } else {
                CommandOutcome::success(tr(
                    "Windows display setting applied; DCHU preference mirror is unavailable.",
                    "Windows 显示设置已应用；DCHU 偏好镜像不可用。",
                ))
            }
        }
        Err(error) => CommandOutcome::failure(error),
    }
}

pub(super) fn apply_monitor_timeouts(ac: u32, dc: u32) -> CommandOutcome {
    CommandOutcome::from_result(
        system_controls::set_monitor_timeouts(ac, dc),
        tr(
            "Monitor timeouts applied and read back.",
            "显示器超时已应用并读回。",
        ),
    )
}

pub(super) fn apply_screen_saver(
    dchu: Option<&Dchu>,
    enabled: bool,
    screen_saver_type: u8,
    minutes: u32,
) -> CommandOutcome {
    let previous = system_controls::display_care_state().ok();
    match system_controls::set_screen_saver(enabled, screen_saver_type, minutes) {
        Ok(()) => {
            if let Some(active) = dchu
                && let Err(error) =
                    active.write_setting_byte(0, 48, if enabled { screen_saver_type } else { 0 })
            {
                let rollback = previous.map_or_else(
                    || "; previous Windows state was unavailable".to_owned(),
                    |previous| {
                        system_controls::set_screen_saver(
                            previous.screen_saver_enabled,
                            previous.screen_saver_type,
                            previous.screen_saver_minutes,
                        )
                        .map_or_else(
                            |error| format!("; rollback failed: {error}"),
                            |()| "; restored previous Windows state".to_owned(),
                        )
                    },
                );
                CommandOutcome::failure(format!(
                    "{}: {error}{rollback}",
                    tr("DCHU preference write failed", "DCHU 偏好写入失败")
                ))
            } else {
                CommandOutcome::success(tr(
                    "Screen saver applied and read back.",
                    "屏幕保护程序已应用并读回。",
                ))
            }
        }
        Err(error) => CommandOutcome::failure(error),
    }
}

pub(super) fn apply_brightness(percent: u8) -> CommandOutcome {
    CommandOutcome::from_result(
        system_controls::set_screen_brightness(percent),
        tr("Display brightness applied.", "屏幕亮度已应用。"),
    )
}

pub(super) fn apply_blue_light(level: u8) -> CommandOutcome {
    CommandOutcome::from_result(
        system_controls::set_blue_light_filter(level),
        tr("Blue-light filter applied.", "蓝光滤镜已应用。"),
    )
}

pub(super) fn apply_volume(percent: u8) -> CommandOutcome {
    CommandOutcome::from_result(
        system_controls::set_audio_volume(percent),
        tr("Volume applied.", "音量已应用。"),
    )
}

pub(super) fn apply_mute(muted: bool) -> CommandOutcome {
    CommandOutcome::from_result(
        system_controls::set_audio_mute(muted),
        if muted {
            tr("Audio muted.", "音频已静音。")
        } else {
            tr("Audio unmuted.", "音频已取消静音。")
        },
    )
}
