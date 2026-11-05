use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use neo_control_protocol::{
    AntiDustSchedule, CustomFanCurve, Dchu, FanMode, FlexiChargerMode, GpuSwitchMode,
    PlatformState, PowerMode, QuickToggle, QuickToggleState, RgbColor,
};
use slint::Weak;

use crate::MainWindow;
use crate::diagnostics::log_action;
use crate::display_care::{display_care_offset, restore_display_toggle};
use crate::fan_curve_ui::apply_rpm_edit_point;
use crate::formatting::shorten_path;
use crate::input::{send_touchpad_chord, send_virtual_key};
use crate::localization::{localized_gpu_label, localized_toggle_label, tr};
use crate::runtime_state::{flexi_recovery_ready, persist_power_mode};
use crate::system_controls;
use crate::telemetry::SystemSampler;
use crate::ui_update::{UiUpdate, apply_update};

pub(crate) enum WorkerCommand {
    ApplyPower(PowerMode),
    ApplyFan(FanMode),
    ApplySlowFan(bool),
    ApplyTurboFan(bool),
    ApplyFanOffset(u8),
    RunAntiDust,
    ApplyAntiDustSchedule(AntiDustSchedule),
    ApplyDisplayToggle(u8, bool),
    ApplyMonitorTimeouts(u32, u32),
    ApplyScreenSaver(bool, u8, u32),
    ApplyRpmEdit {
        cpu: [(u8, u16); 10],
        gpu: [(u8, u16); 10],
    },
    ApplyQuickToggle(QuickToggle, bool),
    ApplyLowRefresh(bool),
    ApplyFanCurve(CustomFanCurve),
    ApplyQuiet(bool),
    ApplyGpuSwitch(GpuSwitchMode),
    ApplyFlexiCharger(u8, u8, u8),
    ApplyRgbBrightness(u8),
    ApplyRgbColor(RgbColor),
    ApplyRgbEffect(u8),
    ApplyRgbSleepTimer(bool, u32),
    ApplyRgbBootEffect(bool),
    ApplyCamera(bool),
    ApplyTouchpad(bool),
    ApplyAirplane(bool),
    ApplyLockKey(u8),
    ApplyBrightness(u8),
    ApplyBlueLight(u8),
    ApplyVolume(u8),
    ApplyMute(bool),
    Refresh,
    ClearStatus,
}

pub(crate) fn spawn_worker(receiver: mpsc::Receiver<WorkerCommand>, weak: Weak<MainWindow>) {
    thread::spawn(move || {
        log_action("worker: start");
        let mut sampler = SystemSampler::new();
        log_action("worker: sampler ready");
        log_action("worker: opening DCHU");
        let mut dchu = match Dchu::open() {
            Ok(dchu) => {
                log_action(&format!("worker: DCHU opened at {}", dchu.path()));
                Some(dchu)
            }
            Err(error) => {
                log_action(&format!("worker: DCHU open failed: {error}"));
                let update = UiUpdate {
                    transport_detail: tr("Signed codec not found", "未找到签名编解码库").to_owned(),
                    status: Some((error.to_string(), 2)),
                    ..UiUpdate::default()
                };
                push_update(&weak, update);
                None
            }
        };
        let mut status: Option<(String, i32)> = None;
        let mut next_retry = Instant::now() + Duration::from_secs(5);
        let mut sync_selection_once = true;
        let mut tick = 0_u64;

        loop {
            match receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(WorkerCommand::ApplyPower(mode)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    match active.set_power_mode(mode) {
                        Ok(()) => {
                            persist_power_mode(mode);
                            status = Some((
                                format!(
                                    "{} applied. DCHU page 1 offset 1 read back successfully.",
                                    mode.label()
                                ),
                                1,
                            ));
                        }
                        Err(error) => {
                            status = Some((error.to_string(), 2));
                        }
                    }
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyFan(mode)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    match active.set_fan_mode(mode) {
                        Ok(()) => {
                            status = Some((
                                format!(
                                    "{} cooling policy applied. Fan settings read back successfully.",
                                    mode.label()
                                ),
                                1,
                            ));
                        }
                        Err(error) => {
                            status = Some((error.to_string(), 2));
                        }
                    }
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplySlowFan(enabled)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_slow_fan(enabled) {
                        Ok(()) => Some((
                            tr("Noiseless fan setting applied.", "静音风扇设置已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyTurboFan(enabled)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_turbo_fan(enabled) {
                        Ok(()) => Some((
                            tr("Turbo fan setting applied.", "涡轮风扇设置已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyFanOffset(percent)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_fan_offset(percent) {
                        Ok(()) => {
                            Some((tr("Fan offset applied.", "风扇补偿已应用。").to_owned(), 1))
                        }
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyDisplayToggle(toggle, enabled)) => {
                    let previous = match system_controls::display_care_state() {
                        Ok(state) => state,
                        Err(error) => {
                            status = Some((error, 2));
                            push_snapshot(
                                &weak,
                                &mut sampler,
                                dchu.as_ref(),
                                status.clone(),
                                false,
                                false,
                                true,
                            );
                            continue;
                        }
                    };
                    let result = match toggle {
                        0 => system_controls::set_taskbar_auto_hide(enabled),
                        1 => system_controls::set_taskbar_transparency(enabled),
                        2 => system_controls::set_dark_mode(enabled),
                        _ => Err("unknown display-care toggle".to_owned()),
                    };
                    status = match result {
                        Ok(()) => {
                            if let (Some(active), Some(offset)) =
                                (dchu.as_ref(), display_care_offset(toggle))
                            {
                                match active.write_setting_byte(0, offset, u8::from(enabled)) {
                                    Ok(()) => Some((
                                        tr(
                                            "Display protection applied and read back.",
                                            "显示保护设置已应用并读回。",
                                        )
                                        .to_owned(),
                                        1,
                                    )),
                                    Err(error) => {
                                        let rollback = restore_display_toggle(toggle, &previous)
                                            .map_or_else(
                                                |error| format!("; rollback failed: {error}"),
                                                |()| "; restored previous Windows state".to_owned(),
                                            );
                                        Some((
                                            format!(
                                                "{}: {error}{rollback}",
                                                tr(
                                                    "DCHU preference write failed",
                                                    "DCHU 偏好写入失败"
                                                )
                                            ),
                                            2,
                                        ))
                                    }
                                }
                            } else {
                                Some((
                                    tr(
                                        "Windows display setting applied; DCHU preference mirror is unavailable.",
                                        "Windows 显示设置已应用；DCHU 偏好镜像不可用。",
                                    )
                                    .to_owned(),
                                    1,
                                ))
                            }
                        }
                        Err(error) => Some((error, 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyMonitorTimeouts(ac, dc)) => {
                    status = match system_controls::set_monitor_timeouts(ac, dc) {
                        Ok(()) => Some((
                            tr(
                                "Monitor timeouts applied and read back.",
                                "显示器超时已应用并读回。",
                            )
                            .to_owned(),
                            1,
                        )),
                        Err(error) => Some((error, 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyScreenSaver(enabled, screen_saver_type, minutes)) => {
                    let previous = system_controls::display_care_state().ok();
                    status = match system_controls::set_screen_saver(
                        enabled,
                        screen_saver_type,
                        minutes,
                    ) {
                        Ok(()) => {
                            if let Some(active) = dchu.as_ref()
                                && let Err(error) = active.write_setting_byte(
                                    0,
                                    48,
                                    if enabled { screen_saver_type } else { 0 },
                                )
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
                                Some((
                                    format!(
                                        "{}: {error}{rollback}",
                                        tr("DCHU preference write failed", "DCHU 偏好写入失败")
                                    ),
                                    2,
                                ))
                            } else {
                                Some((
                                    tr(
                                        "Screen saver applied and read back.",
                                        "屏幕保护程序已应用并读回。",
                                    )
                                    .to_owned(),
                                    1,
                                ))
                            }
                        }
                        Err(error) => Some((error, 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::RunAntiDust) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.run_anti_dust() {
                        Ok(()) => Some((
                            tr("Anti-dust routine started.", "防尘清洁已启动。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyAntiDustSchedule(schedule)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_anti_dust_schedule(schedule) {
                        Ok(()) => Some((
                            tr("Anti-dust schedule applied.", "防尘计划已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyRpmEdit { cpu, gpu }) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.read_rpm_fan_table() {
                        Ok(mut table) => {
                            for (index, (temperature, rpm)) in cpu.iter().enumerate() {
                                apply_rpm_edit_point(
                                    &mut table.channels[0].levels[index],
                                    *temperature,
                                    *rpm,
                                );
                            }
                            for (index, (temperature, rpm)) in gpu.iter().enumerate() {
                                apply_rpm_edit_point(
                                    &mut table.channels[1].levels[index],
                                    *temperature,
                                    *rpm,
                                );
                            }
                            match active.set_custom_rpm_fan_table(&table) {
                                Ok(()) => Some((
                                    tr(
                                        "Ten-point fan table applied and read back.",
                                        "十点风扇曲线已应用并读回。",
                                    )
                                    .to_owned(),
                                    1,
                                )),
                                Err(error) => Some((error.to_string(), 2)),
                            }
                        }
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyRgbBrightness(level)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_rgb_keyboard_brightness(level) {
                        Ok(()) => Some((
                            tr("Keyboard brightness applied.", "键盘亮度已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyRgbColor(color)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_rgb_keyboard_color(color) {
                        Ok(()) => Some((
                            tr("Keyboard color applied.", "键盘颜色已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyRgbEffect(effect)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_rgb_keyboard_effect(effect) {
                        Ok(()) => Some((
                            tr("Keyboard effect applied.", "键盘灯效已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyRgbSleepTimer(enabled, seconds)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_rgb_keyboard_sleep_timer(enabled, seconds) {
                        Ok(()) => Some((
                            tr("Keyboard sleep timer applied.", "键盘休眠计时已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyRgbBootEffect(enabled)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_rgb_keyboard_boot_effect(enabled) {
                        Ok(()) => Some((
                            tr("Keyboard boot effect applied.", "键盘开机灯效已应用。").to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyCamera(enabled)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_camera_enabled(enabled) {
                        Ok(()) => Some((
                            if enabled {
                                tr("Camera enabled.", "摄像头已启用。")
                            } else {
                                tr("Camera disabled.", "摄像头已关闭。")
                            }
                            .to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyTouchpad(enabled)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    let input = send_touchpad_chord();
                    let persisted = active.set_touchpad_enabled(enabled);
                    status = Some(match (input, persisted) {
                        (Ok(()), Ok(())) => (
                            if enabled {
                                tr("Touchpad enabled.", "触摸板已启用。")
                            } else {
                                tr("Touchpad disabled.", "触摸板已关闭。")
                            }
                            .to_owned(),
                            1,
                        ),
                        (input, persisted) => (
                            format!(
                                "{}; {}",
                                input.err().unwrap_or_else(|| "input path ready".to_owned()),
                                persisted
                                    .err()
                                    .map(|error| error.to_string())
                                    .unwrap_or_else(|| "setting persisted".to_owned())
                            ),
                            2,
                        ),
                    });
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyBrightness(percent)) => {
                    status = Some(match system_controls::set_screen_brightness(percent) {
                        Ok(()) => (
                            tr("Display brightness applied.", "屏幕亮度已应用。").to_owned(),
                            1,
                        ),
                        Err(error) => (error, 2),
                    });
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyBlueLight(level)) => {
                    status = Some(match system_controls::set_blue_light_filter(level) {
                        Ok(()) => (
                            tr("Blue-light filter applied.", "蓝光滤镜已应用。").to_owned(),
                            1,
                        ),
                        Err(error) => (error, 2),
                    });
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyVolume(percent)) => {
                    status = Some(match system_controls::set_audio_volume(percent) {
                        Ok(()) => (tr("Volume applied.", "音量已应用。").to_owned(), 1),
                        Err(error) => (error, 2),
                    });
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyMute(muted)) => {
                    status = Some(match system_controls::set_audio_mute(muted) {
                        Ok(()) => (
                            if muted {
                                tr("Audio muted.", "音频已静音。")
                            } else {
                                tr("Audio unmuted.", "音频已取消静音。")
                            }
                            .to_owned(),
                            1,
                        ),
                        Err(error) => (error, 2),
                    });
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyAirplane(enabled)) => {
                    status = Some(match system_controls::set_airplane_mode(enabled) {
                        Ok(()) => {
                            if let Some(active) = dchu.as_ref() {
                                let _ = active.notify_airplane_mode(enabled);
                            }
                            (
                                if enabled {
                                    tr("Airplane mode enabled.", "飞行模式已开启。")
                                } else {
                                    tr("Airplane mode disabled.", "飞行模式已关闭。")
                                }
                                .to_owned(),
                                1,
                            )
                        }
                        Err(error) => (error, 2),
                    });
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyLockKey(vk)) => {
                    status = Some(match send_virtual_key(vk) {
                        Ok(()) => (tr("Lock key toggled.", "锁定键已切换。").to_owned(), 1),
                        Err(error) => (error, 2),
                    });
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyLowRefresh(enabled)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    match active.set_low_refresh_rate(enabled) {
                        Ok(()) => {
                            status = Some((
                                if enabled {
                                    tr(
                                        "Lowest panel refresh rate enabled.",
                                        "已启用最低屏幕刷新率。",
                                    )
                                } else {
                                    tr("Panel refresh rate restored.", "已恢复屏幕刷新率。")
                                }
                                .to_owned(),
                                1,
                            ));
                        }
                        Err(error) => {
                            status = Some((error.to_string(), 2));
                        }
                    }
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyQuickToggle(toggle, enabled)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    match active.set_quick_toggle(toggle, enabled) {
                        Ok(()) => {
                            status = Some((
                                format!(
                                    "{} {}{}",
                                    localized_toggle_label(toggle),
                                    tr("set to ", "已设为"),
                                    if enabled {
                                        tr("enabled", "启用")
                                    } else {
                                        tr("disabled", "禁用")
                                    }
                                ),
                                1,
                            ));
                        }
                        Err(error) => {
                            status = Some((error.to_string(), 2));
                        }
                    }
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyFanCurve(curve)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    match active.set_custom_fan_curve(curve) {
                        Ok(()) => {
                            status = Some((
                                tr(
                                    "Custom fan curve written; block-13 runtime table read back successfully.",
                                    "自定义风扇曲线已写入，block 13 运行时表读回成功。",
                                )
                                .to_owned(),
                                1,
                            ));
                        }
                        Err(error) => {
                            status = Some((error.to_string(), 2));
                        }
                    }
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyQuiet(disable_dgpu)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    let mut message = match active.set_power_mode(PowerMode::Quiet) {
                        Ok(()) => {
                            persist_power_mode(PowerMode::Quiet);
                            tr("Quiet mode applied.", "静音模式已应用。").to_owned()
                        }
                        Err(error) => error.to_string(),
                    };
                    let mut tone = if message == tr("Quiet mode applied.", "静音模式已应用。")
                    {
                        1
                    } else {
                        2
                    };
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
                    status = Some((message, tone));
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyGpuSwitch(mode)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.stage_gpu_switch(mode) {
                        Ok(()) => Some((
                            format!(
                                "{} {} {}",
                                tr("GPU mode staged:", "显卡模式已暂存："),
                                localized_gpu_label(mode.as_u8()),
                                tr("Restart to apply.", "重启后生效。")
                            ),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ApplyFlexiCharger(mode, start, stop)) => {
                    let Some(active) = dchu.as_ref() else {
                        status = Some((
                            tr("Hardware transport is unavailable.", "硬件通道不可用。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    let Some(mode) = FlexiChargerMode::from_u8(mode) else {
                        status = Some((
                            tr("Unsupported charge mode.", "不支持的充电模式。").to_owned(),
                            2,
                        ));
                        push_snapshot(
                            &weak,
                            &mut sampler,
                            dchu.as_ref(),
                            status.clone(),
                            false,
                            false,
                            true,
                        );
                        continue;
                    };
                    status = match active.set_flexi_charger(mode, start, stop) {
                        Ok(()) => Some((
                            tr(
                                "Charge policy applied and verified.",
                                "充电策略已应用并校验。",
                            )
                            .to_owned(),
                            1,
                        )),
                        Err(error) => Some((error.to_string(), 2)),
                    };
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::Refresh) => {
                    if dchu.is_none() {
                        match Dchu::open() {
                            Ok(opened) => {
                                status = Some((
                                    tr(
                                        "Signed DCHU transport connected.",
                                        "签名 DCHU 通道已连接。",
                                    )
                                    .to_owned(),
                                    1,
                                ));
                                dchu = Some(opened);
                            }
                            Err(error) => status = Some((error.to_string(), 2)),
                        }
                        next_retry = Instant::now() + Duration::from_secs(5);
                    }
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        true,
                        true,
                    );
                }
                Ok(WorkerCommand::ClearStatus) => {
                    status = None;
                    push_snapshot(&weak, &mut sampler, dchu.as_ref(), None, false, false, true);
                }
                Err(RecvTimeoutError::Timeout) => {
                    tick = tick.wrapping_add(1);
                    if dchu.is_none() && Instant::now() >= next_retry {
                        if let Ok(opened) = Dchu::open() {
                            dchu = Some(opened);
                            status = Some((
                                tr("Signed DCHU transport connected.", "签名 DCHU 通道已连接。")
                                    .to_owned(),
                                1,
                            ));
                        }
                        next_retry = Instant::now() + Duration::from_secs(5);
                    }
                    let sync_selection = std::mem::take(&mut sync_selection_once);
                    push_snapshot(
                        &weak,
                        &mut sampler,
                        dchu.as_ref(),
                        status.clone(),
                        false,
                        sync_selection,
                        tick % 2 == 1,
                    );
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

fn push_snapshot(
    weak: &Weak<MainWindow>,
    sampler: &mut SystemSampler,
    dchu: Option<&Dchu>,
    status: Option<(String, i32)>,
    busy: bool,
    sync_selection: bool,
    extended_reads: bool,
) {
    log_action("worker: snapshot begin");
    let system = sampler.sample();
    log_action(&format!(
        "worker: system sampled cpu={:.0}% clock={} MHz max={} MHz",
        system.cpu_load_percent, system.cpu_clock_mhz, system.cpu_max_clock_mhz
    ));
    let thermal_started = Instant::now();
    let thermal = dchu.and_then(|dchu| dchu.read_thermal().ok());
    log_action(&format!(
        "worker: thermal sampled in {} us",
        thermal_started.elapsed().as_micros()
    ));
    let (
        platform,
        quick_toggles,
        fan_curve,
        rpm_fan_table,
        gpu_switch,
        capabilities,
        rgb_keyboard,
        camera_enabled,
        touchpad_enabled,
        airplane_enabled,
        brightness,
        blue_light,
        audio,
        anti_dust_schedule,
    ) = if extended_reads {
        let started = Instant::now();
        let platform = dchu.map(Dchu::read_platform_state).unwrap_or_default();
        log_action(&format!(
            "worker: platform sampled in {} us",
            started.elapsed().as_micros()
        ));
        let started = Instant::now();
        let quick_toggles = dchu.map(Dchu::read_quick_toggles).unwrap_or_default();
        log_action(&format!(
            "worker: toggles sampled in {} us",
            started.elapsed().as_micros()
        ));
        let started = Instant::now();
        let fan_curve = dchu.and_then(|dchu| dchu.read_fan_curve().ok());
        log_action(&format!(
            "worker: fan curve sampled in {} us",
            started.elapsed().as_micros()
        ));
        // Extended ACPI commands return a full response frame; the protocol layer
        // always provides the full 256-byte vendor response buffer.
        let started = Instant::now();
        let rpm_fan_table = dchu.and_then(|dchu| dchu.read_rpm_fan_table().ok());
        log_action(&format!(
            "worker: RPM fan table sampled in {} us",
            started.elapsed().as_micros()
        ));
        let started = Instant::now();
        let gpu_switch = dchu.and_then(|dchu| dchu.read_gpu_switch().ok());
        log_action(&format!(
            "worker: gpu switch sampled in {} us",
            started.elapsed().as_micros()
        ));
        let started = Instant::now();
        let capabilities = dchu.and_then(|dchu| dchu.read_capabilities().ok());
        log_action(&format!(
            "worker: capabilities sampled in {} us",
            started.elapsed().as_micros()
        ));
        let rgb_keyboard = dchu.and_then(|dchu| dchu.read_rgb_keyboard_state().ok());
        let camera_enabled = dchu.and_then(Dchu::read_camera_enabled);
        let touchpad_enabled = dchu.and_then(Dchu::read_touchpad_enabled);
        let airplane_enabled = system_controls::airplane_mode_enabled().ok();
        let brightness = system_controls::screen_brightness().ok();
        let blue_light = dchu.and_then(|dchu| dchu.raw_read_settings(1, 34, 1).1.first().copied());
        let audio = system_controls::audio_state().ok();
        let anti_dust_schedule = dchu.and_then(|dchu| dchu.read_anti_dust_schedule().ok());
        (
            platform,
            quick_toggles,
            fan_curve,
            rpm_fan_table,
            gpu_switch,
            capabilities,
            rgb_keyboard,
            camera_enabled,
            touchpad_enabled,
            airplane_enabled,
            brightness,
            blue_light,
            audio,
            anti_dust_schedule,
        )
    } else {
        (
            PlatformState::default(),
            QuickToggleState::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    };
    let extended_api = dchu.is_some_and(Dchu::has_extended_api);
    let display_care = extended_reads
        .then(system_controls::display_care_state)
        .and_then(Result::ok);
    let flexi_ready = flexi_recovery_ready();
    let flexi_charger = if flexi_ready {
        dchu.and_then(|dchu| dchu.read_flexi_charger().ok())
    } else {
        None
    };
    let transport_ready = dchu.is_some();
    let transport_label = if transport_ready {
        tr("DCHU connected", "DCHU 已连接").to_owned()
    } else {
        tr("DCHU unavailable", "DCHU 不可用").to_owned()
    };
    let (transport_detail, transport_device_path) = if let Some(dchu) = dchu {
        (shorten_path(dchu.path()), dchu.path().to_owned())
    } else {
        (
            tr(
                "Signed AcpiBridge device not found",
                "未找到签名 AcpiBridge 设备",
            )
            .to_owned(),
            tr("Not located", "未找到").to_owned(),
        )
    };
    push_update(
        weak,
        UiUpdate {
            system,
            thermal,
            platform,
            quick_toggles,
            fan_curve,
            rpm_fan_table,
            gpu_switch,
            flexi_charger,
            capabilities,
            rgb_keyboard,
            camera_enabled,
            touchpad_enabled,
            airplane_enabled,
            brightness,
            blue_light,
            audio,
            display_care,
            anti_dust_schedule,
            flexi_ready,
            extended_api,
            transport_ready,
            transport_label,
            transport_detail,
            transport_device_path,
            busy,
            status,
            sync_selection,
            extended_reads,
        },
    );
}

fn push_update(weak: &Weak<MainWindow>, update: UiUpdate) {
    let weak = weak.clone();
    log_action("worker: dispatching UI update");
    if let Err(error) = slint::invoke_from_event_loop(move || {
        log_action("ui: applying update");
        if let Some(ui) = weak.upgrade() {
            apply_update(&ui, update);
            log_action("ui: update applied");
        } else {
            log_action("ui: weak handle no longer upgrades");
        }
    }) {
        log_action(&format!("worker: UI dispatch failed: {error}"));
    }
}
