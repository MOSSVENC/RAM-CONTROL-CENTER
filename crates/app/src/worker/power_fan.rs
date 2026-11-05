use neo_control_protocol::{
    AntiDustSchedule, CustomFanCurve, Dchu, FanMode, FlexiChargerMode, GpuSwitchMode, PowerMode,
};

use super::CommandOutcome;
use crate::fan_curve_ui::apply_rpm_edit_point;
use crate::localization::{localized_gpu_label, tr};
use crate::runtime_state::persist_power_mode;

pub(super) fn apply_power(dchu: Option<&Dchu>, mode: PowerMode) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    match active.set_power_mode(mode) {
        Ok(()) => {
            persist_power_mode(mode);
            CommandOutcome::success(format!(
                "{} applied. DCHU page 1 offset 1 read back successfully.",
                mode.label()
            ))
        }
        Err(error) => CommandOutcome::failure(error),
    }
}

pub(super) fn apply_fan(dchu: Option<&Dchu>, mode: FanMode) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_fan_mode(mode),
        format!(
            "{} cooling policy applied. Fan settings read back successfully.",
            mode.label()
        ),
    )
}

pub(super) fn apply_slow_fan(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_slow_fan(enabled),
        tr("Noiseless fan setting applied.", "静音风扇设置已应用。"),
    )
}

pub(super) fn apply_turbo_fan(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_turbo_fan(enabled),
        tr("Turbo fan setting applied.", "涡轮风扇设置已应用。"),
    )
}

pub(super) fn apply_fan_offset(dchu: Option<&Dchu>, percent: u8) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_fan_offset(percent),
        tr("Fan offset applied.", "风扇补偿已应用。"),
    )
}

pub(super) fn run_anti_dust(dchu: Option<&Dchu>) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.run_anti_dust(),
        tr("Anti-dust routine started.", "防尘清洁已启动。"),
    )
}

pub(super) fn apply_anti_dust_schedule(
    dchu: Option<&Dchu>,
    schedule: AntiDustSchedule,
) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_anti_dust_schedule(schedule),
        tr("Anti-dust schedule applied.", "防尘计划已应用。"),
    )
}

pub(super) fn apply_rpm_edit(
    dchu: Option<&Dchu>,
    cpu: [(u8, u16); 10],
    gpu: [(u8, u16); 10],
) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    let mut table = match active.read_rpm_fan_table() {
        Ok(table) => table,
        Err(error) => return CommandOutcome::failure(error),
    };
    for (index, (temperature, rpm)) in cpu.iter().enumerate() {
        apply_rpm_edit_point(&mut table.channels[0].levels[index], *temperature, *rpm);
    }
    for (index, (temperature, rpm)) in gpu.iter().enumerate() {
        apply_rpm_edit_point(&mut table.channels[1].levels[index], *temperature, *rpm);
    }
    CommandOutcome::from_result(
        active.set_custom_rpm_fan_table(&table),
        tr(
            "Ten-point fan table applied and read back.",
            "十点风扇曲线已应用并读回。",
        ),
    )
}

pub(super) fn apply_low_refresh(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    let success = if enabled {
        tr(
            "Lowest panel refresh rate enabled.",
            "已启用最低屏幕刷新率。",
        )
    } else {
        tr("Panel refresh rate restored.", "已恢复屏幕刷新率。")
    };
    CommandOutcome::from_result(active.set_low_refresh_rate(enabled), success)
}

pub(super) fn apply_fan_curve(dchu: Option<&Dchu>, curve: CustomFanCurve) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_custom_fan_curve(curve),
        tr(
            "Custom fan curve written; block-13 runtime table read back successfully.",
            "自定义风扇曲线已写入，block 13 运行时表读回成功。",
        ),
    )
}

pub(super) fn apply_quiet(dchu: Option<&Dchu>, disable_dgpu: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    let quiet_applied = tr("Quiet mode applied.", "静音模式已应用。");
    let mut message = match active.set_power_mode(PowerMode::Quiet) {
        Ok(()) => {
            persist_power_mode(PowerMode::Quiet);
            quiet_applied.to_owned()
        }
        Err(error) => error.to_string(),
    };
    let mut tone = if message == quiet_applied { 1 } else { 2 };
    if disable_dgpu {
        match active.stage_gpu_switch(GpuSwitchMode::Integrated) {
            Ok(()) => {
                message = tr(
                    "Quiet mode applied. Discrete GPU disable was staged for the next reboot.",
                    "静音模式已应用。关闭独显已暂存，将在下次重启后生效。",
                )
                .to_owned();
                tone = 1;
            }
            Err(error) => {
                message = format!(
                    "{} {}",
                    tr(
                        "Quiet mode applied, but GPU staging failed:",
                        "静音模式已应用，但显卡策略暂存失败："
                    ),
                    error
                );
                tone = 2;
            }
        }
    }
    CommandOutcome::with_tone(message, tone)
}

pub(super) fn apply_gpu_switch(dchu: Option<&Dchu>, mode: GpuSwitchMode) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.stage_gpu_switch(mode),
        format!(
            "{} {} {}",
            tr("GPU mode staged:", "显卡模式已暂存："),
            localized_gpu_label(mode.as_u8()),
            tr("Restart to apply.", "重启后生效。")
        ),
    )
}

pub(super) fn apply_flexi_charger(
    dchu: Option<&Dchu>,
    mode: u8,
    start: u8,
    stop: u8,
) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    let Some(mode) = FlexiChargerMode::from_u8(mode) else {
        return CommandOutcome::failure(tr("Unsupported charge mode.", "不支持的充电模式。"))
            .without_selection_sync();
    };
    CommandOutcome::from_result(
        active.set_flexi_charger(mode, start, stop),
        tr(
            "Charge policy applied and verified.",
            "充电策略已应用并校验。",
        ),
    )
}
