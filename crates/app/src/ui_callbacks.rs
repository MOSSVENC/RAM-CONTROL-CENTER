use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use neo_control_protocol::{
    AntiDustSchedule, CustomFanCurve, FanMode, GpuSwitchMode, PowerMode, RgbColor,
};
use slint::ComponentHandle;

use crate::diagnostics::log_action;
use crate::fan_curve_ui::{curve_nodes, quick_toggle_from_index};
use crate::flexikey_ui::{FlexiKeyViewState, render_flexikey};
use crate::localization::{
    is_zh, localized_fan_label, localized_power_label, localized_toggle_label, set_zh, tr,
};
use crate::tray;
use crate::worker::WorkerCommand;
use crate::{MainWindow, Palette};

static PAGE_EPOCH: AtomicU64 = AtomicU64::new(0);
static THEME_EPOCH: AtomicU64 = AtomicU64::new(0);

const PAGE_FADE_MS: u64 = 180;
const PAGE_SETTLE_MS: u64 = 16;
const PAGE_SLIDE_PX: f32 = 14.0;
const THEME_COVER_MS: u64 = 190;
const THEME_HOLD_MS: u64 = 24;

pub(crate) fn wire_ui(
    ui: &MainWindow,
    sender: Sender<WorkerCommand>,
    flexikey_state: Rc<RefCell<FlexiKeyViewState>>,
) {
    let weak = ui.as_weak();
    ui.on_select_page(move |page| {
        let page = page.clamp(0, 10);
        log_action(&format!("select-page {page}"));
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_selected_page() == page {
            return;
        }
        let epoch = PAGE_EPOCH.fetch_add(1, Ordering::Relaxed) + 1;
        ui.set_page_opacity(0.0);
        ui.set_page_shift(-PAGE_SLIDE_PX);
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(PAGE_FADE_MS), move || {
            if PAGE_EPOCH.load(Ordering::Relaxed) != epoch {
                return;
            }
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.set_selected_page(page);
            ui.set_page_shift(PAGE_SLIDE_PX);
            let weak_for_fade_in = ui.as_weak();
            slint::Timer::single_shot(Duration::from_millis(PAGE_SETTLE_MS), move || {
                if PAGE_EPOCH.load(Ordering::Relaxed) == epoch
                    && let Some(ui) = weak_for_fade_in.upgrade()
                {
                    ui.set_page_opacity(1.0);
                    ui.set_page_shift(0.0);
                }
            });
        });
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_set_language(move |zh| {
        if is_zh() == zh {
            return;
        }
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let epoch = PAGE_EPOCH.fetch_add(1, Ordering::Relaxed) + 1;
        ui.set_page_opacity(0.0);
        ui.set_page_shift(-PAGE_SLIDE_PX);
        let weak_for_fade_out = ui.as_weak();
        let flexikey_state_for_fade = Rc::clone(&flexikey_state);
        let tx = tx.clone();
        slint::Timer::single_shot(Duration::from_millis(PAGE_FADE_MS), move || {
            if PAGE_EPOCH.load(Ordering::Relaxed) != epoch {
                return;
            }
            set_zh(zh);
            let locale = if zh { "zh-CN" } else { "en" };
            if let Err(error) = slint::select_bundled_translation(locale) {
                log_action(&format!("translation switch to {locale} failed: {error}"));
            }
            let Some(ui) = weak_for_fade_out.upgrade() else {
                return;
            };
            ui.set_language_zh(zh);
            ui.set_page_shift(PAGE_SLIDE_PX);
            render_flexikey(&ui, &flexikey_state_for_fade.borrow());
            let weak_for_fade_in = ui.as_weak();
            slint::Timer::single_shot(Duration::from_millis(PAGE_SETTLE_MS), move || {
                if PAGE_EPOCH.load(Ordering::Relaxed) == epoch
                    && let Some(ui) = weak_for_fade_in.upgrade()
                {
                    ui.set_page_opacity(1.0);
                    ui.set_page_shift(0.0);
                }
            });
            let _ = tx.send(WorkerCommand::Refresh);
        });
    });

    let weak = ui.as_weak();
    ui.on_set_theme(move |dark| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.global::<Palette>().get_dark() == dark {
            return;
        }
        let epoch = THEME_EPOCH.fetch_add(1, Ordering::Relaxed) + 1;
        ui.set_theme_flash(1.0);
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(THEME_COVER_MS), move || {
            if THEME_EPOCH.load(Ordering::Relaxed) != epoch {
                return;
            }
            if let Some(ui) = weak.upgrade() {
                ui.global::<Palette>().set_dark(dark);
            }
            slint::Timer::single_shot(Duration::from_millis(THEME_HOLD_MS), move || {
                if THEME_EPOCH.load(Ordering::Relaxed) == epoch
                    && let Some(ui) = weak.upgrade()
                {
                    ui.set_theme_flash(0.0);
                }
            });
        });
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_refresh_now(move || {
        log_action("refresh");
        if let Some(ui) = weak.upgrade() {
            ui.set_status_message(
                tr(
                    "Reading the signed DCHU transport…",
                    "正在读取签名 DCHU 通道…",
                )
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::Refresh);
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_power(move |mode| {
        log_action(&format!("apply-power callback {mode}"));
        let Ok(raw_mode) = u8::try_from(mode) else {
            return;
        };
        let Some(mode) = PowerMode::from_u8(raw_mode) else {
            return;
        };
        if mode == PowerMode::Quiet {
            if let Some(ui) = weak.upgrade() {
                ui.set_quiet_dialog_open(true);
            }
            return;
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                format!(
                    "{} {}",
                    tr("Applying power profile:", "正在应用电源配置："),
                    localized_power_label(mode)
                )
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyPower(mode));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_fan(move |mode| {
        log_action(&format!("apply-fan callback {mode}"));
        let Ok(raw_mode) = u8::try_from(mode) else {
            return;
        };
        let Some(mode) = FanMode::from_u8(raw_mode) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                format!(
                    "{} {}",
                    tr("Applying fan policy:", "正在应用风扇策略："),
                    localized_fan_label(mode)
                )
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyFan(mode));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_set_quick_toggle(move |toggle, enabled| {
        log_action(&format!("set-quick-toggle callback {toggle} -> {enabled}"));
        let Some(toggle) = quick_toggle_from_index(toggle) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                format!(
                    "{} {}",
                    tr("Applying:", "正在应用："),
                    localized_toggle_label(toggle)
                )
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyQuickToggle(toggle, enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_low_refresh(move |enabled| {
        log_action(&format!("apply-low-refresh callback -> {enabled}"));
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                if enabled {
                    tr(
                        "Switching the panel to its lowest refresh rate…",
                        "正在将屏幕切换到最低刷新率…",
                    )
                } else {
                    tr("Restoring the panel refresh rate…", "正在恢复屏幕刷新率…")
                }
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyLowRefresh(enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_fan_curve(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let curve = CustomFanCurve {
            channels: [
                curve_nodes(
                    ui.get_curve_cpu_t2(),
                    ui.get_curve_cpu_d2(),
                    ui.get_curve_cpu_t3(),
                    ui.get_curve_cpu_d3(),
                ),
                curve_nodes(
                    ui.get_curve_gpu_t2(),
                    ui.get_curve_gpu_d2(),
                    ui.get_curve_gpu_t3(),
                    ui.get_curve_gpu_d3(),
                ),
                None,
            ],
        };
        log_action("apply-fan-curve callback");
        ui.set_busy(true);
        ui.set_status_message(
            tr(
                "Writing and verifying the custom fan curve…",
                "正在写入并校验自定义风扇曲线…",
            )
            .into(),
        );
        ui.set_status_tone(0);
        let _ = tx.send(WorkerCommand::ApplyFanCurve(curve));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_quiet(move |disable_dgpu| {
        if let Some(ui) = weak.upgrade() {
            ui.set_quiet_dialog_open(false);
            ui.set_busy(true);
            ui.set_status_message(
                tr(
                    "Applying Quiet mode and staging the GPU choice…",
                    "正在应用静音模式并暂存显卡策略…",
                )
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyQuiet(disable_dgpu));
    });

    let weak = ui.as_weak();
    ui.on_cancel_quiet(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_quiet_dialog_open(false);
        }
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_gpu_mode(move |mode| {
        let Some(mode) = u8::try_from(mode).ok().and_then(GpuSwitchMode::from_u8) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr(
                    "Staging the GPU mode for the next boot…",
                    "正在为下次启动暂存显卡模式…",
                )
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyGpuSwitch(mode));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_flexi_charger(move |mode, start, stop| {
        let Ok(mode) = u8::try_from(mode) else {
            return;
        };
        let Ok(start) = u8::try_from(start) else {
            return;
        };
        let Ok(stop) = u8::try_from(stop) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying charge policy…", "正在应用充电策略…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyFlexiCharger(mode, start, stop));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_slow_fan(move |enabled| {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr("Applying Noiseless fan mode…", "正在应用静音风扇模式…").into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplySlowFan(enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_turbo_fan(move |enabled| {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying Turbo fan…", "正在应用涡轮风扇…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyTurboFan(enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_rgb_brightness(move |level| {
        let Ok(level) = u8::try_from(level) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying keyboard brightness…", "正在应用键盘亮度…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyRgbBrightness(level));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_rgb_color(move |red, green, blue| {
        let (Ok(red), Ok(green), Ok(blue)) =
            (u8::try_from(red), u8::try_from(green), u8::try_from(blue))
        else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying keyboard color…", "正在应用键盘颜色…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyRgbColor(RgbColor { red, green, blue }));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_rgb_effect(move |effect| {
        let Ok(effect) = u8::try_from(effect) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying keyboard effect…", "正在应用键盘灯效…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyRgbEffect(effect));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_rgb_sleep_timer(move |enabled, hours, minutes, seconds| {
        let (Ok(hours), Ok(minutes), Ok(seconds)) = (
            u32::try_from(hours),
            u32::try_from(minutes),
            u32::try_from(seconds),
        ) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr("Applying keyboard sleep timer…", "正在应用键盘休眠计时…").into(),
            );
            ui.set_status_tone(0);
        }
        let total = hours * 3600 + minutes * 60 + seconds;
        let _ = tx.send(WorkerCommand::ApplyRgbSleepTimer(enabled, total));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_rgb_boot_effect(move |enabled| {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr("Applying keyboard boot effect…", "正在应用键盘开机灯效…").into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyRgbBootEffect(enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_camera(move |enabled| {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Updating camera state…", "正在更新摄像头状态…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyCamera(enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_touchpad(move |enabled| {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Updating touchpad state…", "正在更新触摸板状态…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyTouchpad(enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_send_lock_key(move |vk| {
        let Ok(vk) = u8::try_from(vk) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Toggling lock key…", "正在切换锁定键…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyLockKey(vk));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_rpm_curve(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let cpu = [
            (
                u8::try_from(ui.get_rpm_cpu_temp_0().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_0().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_1().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_1().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_2().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_2().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_3().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_3().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_4().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_4().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_5().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_5().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_6().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_6().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_7().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_7().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_8().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_8().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_cpu_temp_9().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_cpu_rpm_9().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
        ];
        let gpu = [
            (
                u8::try_from(ui.get_rpm_gpu_temp_0().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_0().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_1().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_1().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_2().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_2().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_3().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_3().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_4().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_4().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_5().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_5().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_6().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_6().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_7().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_7().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_8().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_8().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
            (
                u8::try_from(ui.get_rpm_gpu_temp_9().clamp(0, 100)).unwrap_or(0),
                u16::try_from((ui.get_rpm_gpu_rpm_9().clamp(0, 8000) / 100) * 100).unwrap_or(0),
            ),
        ];
        ui.set_busy(true);
        ui.set_status_message(tr("Applying ten-point fan curve…", "正在应用十点风扇曲线…").into());
        ui.set_status_tone(0);
        let _ = tx.send(WorkerCommand::ApplyRpmEdit { cpu, gpu });
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_fan_offset(move |percent| {
        let Ok(percent) = u8::try_from(percent) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying fan offset…", "正在应用风扇补偿…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyFanOffset(percent));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_run_anti_dust(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr("Starting the anti-dust routine…", "正在启动防尘清洁…").into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::RunAntiDust);
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_anti_dust_schedule(move |enabled, resume, day, hour| {
        let (Ok(day), Ok(hour)) = (u8::try_from(day), u8::try_from(hour)) else {
            return;
        };
        let schedule = AntiDustSchedule {
            enabled,
            resume,
            day,
            hour,
        };
        if schedule.validate().is_err() {
            return;
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr("Applying the anti-dust schedule…", "正在应用防尘计划…").into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyAntiDustSchedule(schedule));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_brightness(move |percent| {
        let Ok(percent) = u8::try_from(percent) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying display brightness…", "正在应用屏幕亮度…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyBrightness(percent));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_blue_light(move |level| {
        let Ok(level) = u8::try_from(level).map(|value: u8| value.min(10)) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying blue-light filter…", "正在应用蓝光滤镜…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyBlueLight(level));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_volume(move |percent| {
        let Ok(percent) = u8::try_from(percent) else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Applying volume…", "正在应用音量…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyVolume(percent));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_mute(move |muted| {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Updating mute state…", "正在更新静音状态…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyMute(muted));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_airplane(move |enabled| {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Updating airplane mode…", "正在更新飞行模式…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyAirplane(enabled));
    });

    let weak = ui.as_weak();
    ui.on_set_startup(move |enabled| {
        let result = if enabled {
            std::env::current_exe()
                .map_err(|error| format!("current executable unavailable: {error}"))
                .and_then(|executable| tray::install_startup(&executable))
        } else {
            tray::uninstall_startup()
        };
        if let Some(ui) = weak.upgrade() {
            match result {
                Ok(()) => {
                    ui.set_startup_enabled(enabled);
                    ui.set_status_message(
                        if enabled {
                            tr("Start with Windows enabled.", "已启用登录启动。")
                        } else {
                            tr("Start with Windows disabled.", "已关闭登录启动。")
                        }
                        .into(),
                    );
                    ui.set_status_tone(1);
                }
                Err(error) => {
                    ui.set_status_message(
                        format!("{}: {error}", tr("Startup setting failed", "启动设置失败")).into(),
                    );
                    ui.set_status_tone(2);
                }
            }
        }
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_set_display_toggle(move |kind, enabled| {
        let Ok(kind) = u8::try_from(kind) else {
            return;
        };
        if kind > 2 {
            return;
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr(
                    "Applying Windows display protection…",
                    "正在应用 Windows 显示保护…",
                )
                .into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyDisplayToggle(kind, enabled));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_monitor_timeouts(move |ac, dc| {
        let ac = u32::try_from(ac).unwrap_or(0);
        let dc = u32::try_from(dc).unwrap_or(0);
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(tr("Updating monitor timeouts…", "正在更新显示器超时…").into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyMonitorTimeouts(ac, dc));
    });

    let weak = ui.as_weak();
    let tx = sender.clone();
    ui.on_apply_screen_saver(move |enabled, screen_saver_type, minutes| {
        let screen_saver_type = u8::try_from(screen_saver_type).unwrap_or(0);
        let minutes = u32::try_from(minutes).unwrap_or(0);
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status_message(
                tr("Updating screen-saver state…", "正在更新屏幕保护程序…").into(),
            );
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ApplyScreenSaver(
            enabled,
            screen_saver_type,
            minutes,
        ));
    });

    let weak = ui.as_weak();
    let tx = sender;
    ui.on_clear_status(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_status_message("".into());
            ui.set_status_tone(0);
        }
        let _ = tx.send(WorkerCommand::ClearStatus);
    });
}
