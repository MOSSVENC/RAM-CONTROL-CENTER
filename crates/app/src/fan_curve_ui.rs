use crate::MainWindow;
use crate::localization::tr;
use neo_control_protocol::{
    FanCurveNodes, FanCurveState, QuickToggle, QuickToggleState, RpmFanLevel,
};

pub(crate) fn apply_rpm_edit_point(level: &mut RpmFanLevel, temperature: u8, rpm: u16) {
    level.temp_up_c = temperature;
    level.temp_down_c = temperature;
    level.rpm_up = rpm;
    level.rpm_down = rpm;
}

pub(crate) fn curve_nodes(t2: i32, d2: i32, t3: i32, d3: i32) -> Option<FanCurveNodes> {
    Some(FanCurveNodes {
        t2: u8::try_from(t2).ok()?,
        d2: u8::try_from(d2).ok()?,
        t3: u8::try_from(t3).ok()?,
        d3: u8::try_from(d3).ok()?,
    })
}

pub(crate) fn set_fan_curve_properties(ui: &MainWindow, curve: Option<&FanCurveState>, sync: bool) {
    let Some(curve) = curve else {
        ui.set_curve_available(false);
        ui.set_curve_current_text(tr("Runtime table unavailable").into());
        return;
    };
    let available = !curve.channels[0].is_unused() || !curve.channels[1].is_unused();
    ui.set_curve_available(available);
    ui.set_curve_current_text(
        format!(
            "{} {} · CPU {} / GPU {}",
            curve.fan_count,
            tr("channels"),
            curve_profile_text(&curve.channels[0]),
            curve_profile_text(&curve.channels[1])
        )
        .into(),
    );
    apply_curve_channel(ui, 0, &curve.channels[0], sync);
    apply_curve_channel(ui, 1, &curve.channels[1], sync);
}

fn apply_curve_channel(
    ui: &MainWindow,
    index: usize,
    channel: &neo_control_protocol::FanCurveChannel,
    sync_requested: bool,
) {
    if channel.is_unused() {
        return;
    }
    let t2 = i32::from(channel.points[1].temp_c);
    let d2 = i32::from(channel.points[1].duty_percent());
    let t3 = i32::from(channel.points[2].temp_c);
    let d3 = i32::from(channel.points[2].duty_percent());
    if index == 0 {
        ui.set_curve_current_cpu_t2(t2);
        ui.set_curve_current_cpu_d2(d2);
        ui.set_curve_current_cpu_t3(t3);
        ui.set_curve_current_cpu_d3(d3);
        if sync_requested {
            ui.set_curve_cpu_t2(t2);
            ui.set_curve_cpu_d2(d2);
            ui.set_curve_cpu_t3(t3);
            ui.set_curve_cpu_d3(d3);
        }
    } else {
        ui.set_curve_current_gpu_t2(t2);
        ui.set_curve_current_gpu_d2(d2);
        ui.set_curve_current_gpu_t3(t3);
        ui.set_curve_current_gpu_d3(d3);
        if sync_requested {
            ui.set_curve_gpu_t2(t2);
            ui.set_curve_gpu_d2(d2);
            ui.set_curve_gpu_t3(t3);
            ui.set_curve_gpu_d3(d3);
        }
    }
}

fn curve_profile_text(channel: &neo_control_protocol::FanCurveChannel) -> String {
    if channel.is_unused() {
        return tr("unused").to_owned();
    }
    format!(
        "{}°C/{}% · {}°C/{}%",
        channel.points[1].temp_c,
        channel.points[1].duty_percent(),
        channel.points[2].temp_c,
        channel.points[2].duty_percent()
    )
}

pub(crate) fn set_toggle_properties(ui: &MainWindow, state: &QuickToggleState) {
    ui.set_toggle_fn_value(state.fn_lock.unwrap_or(false));
    ui.set_toggle_fn_known(state.fn_lock.is_some());
    ui.set_toggle_win_value(state.win_lock.unwrap_or(false));
    ui.set_toggle_win_known(state.win_lock.is_some());
    ui.set_toggle_swap_value(state.swap_win_fn.unwrap_or(false));
    ui.set_toggle_swap_known(state.swap_win_fn.is_some());
    ui.set_toggle_hotkeys_value(state.hotkeys_control_center.unwrap_or(false));
    ui.set_toggle_hotkeys_known(state.hotkeys_control_center.is_some());
    ui.set_toggle_copilot_value(state.copilot.unwrap_or(false));
    ui.set_toggle_copilot_known(state.copilot.is_some());
}

pub(crate) fn quick_toggle_from_index(index: i32) -> Option<QuickToggle> {
    match index {
        0 => Some(QuickToggle::FnLock),
        1 => Some(QuickToggle::WinLock),
        2 => Some(QuickToggle::SwapWinFn),
        3 => Some(QuickToggle::HotkeysControlCenter),
        4 => Some(QuickToggle::Copilot),
        _ => None,
    }
}
