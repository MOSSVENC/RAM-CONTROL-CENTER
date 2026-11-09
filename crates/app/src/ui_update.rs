use neo_control_protocol::{
    AntiDustSchedule, FanCurveState, FanMode, FlexiChargerState, GpuSwitchMode, GpuSwitchState,
    PlatformCapabilities, PlatformState, PowerMode, QuickToggleState, RgbKeyboardState,
    RpmFanTable, ThermalSample,
};

use crate::MainWindow;
use crate::driver_manager::DriverHealth;
use crate::fan_curve_ui::{set_fan_curve_properties, set_toggle_properties};
use crate::formatting::{
    format_battery_capacity, format_battery_time, format_bytes, format_clock, format_power_rate,
    format_temperature_celsius, format_threshold_ranges, toggle_value_text,
};
use crate::localization::{localized_fan_label, localized_gpu_label, localized_power_label, tr};
use crate::system_controls::{AudioState, DisplayCareState};
use crate::telemetry::SystemSnapshot;

#[derive(Clone)]
pub(crate) struct UiUpdate {
    pub(crate) system: SystemSnapshot,
    pub(crate) thermal: Option<ThermalSample>,
    pub(crate) platform: PlatformState,
    pub(crate) quick_toggles: QuickToggleState,
    pub(crate) fan_curve: Option<FanCurveState>,
    pub(crate) rpm_fan_table: Option<RpmFanTable>,
    pub(crate) gpu_switch: Option<GpuSwitchState>,
    pub(crate) flexi_charger: Option<FlexiChargerState>,
    pub(crate) capabilities: Option<PlatformCapabilities>,
    pub(crate) rgb_keyboard: Option<RgbKeyboardState>,
    pub(crate) camera_enabled: Option<bool>,
    pub(crate) touchpad_enabled: Option<bool>,
    pub(crate) airplane_enabled: Option<bool>,
    pub(crate) brightness: Option<u8>,
    pub(crate) blue_light: Option<u8>,
    pub(crate) audio: Option<AudioState>,
    pub(crate) display_care: Option<DisplayCareState>,
    pub(crate) anti_dust_schedule: Option<AntiDustSchedule>,
    pub(crate) flexi_ready: bool,
    pub(crate) extended_api: bool,
    pub(crate) transport_ready: bool,
    pub(crate) transport_label: String,
    pub(crate) transport_detail: String,
    pub(crate) transport_device_path: String,
    pub(crate) driver_health: DriverHealth,
    pub(crate) busy: bool,
    pub(crate) status: Option<(String, i32)>,
    pub(crate) sync_selection: bool,
    pub(crate) extended_reads: bool,
}

impl Default for UiUpdate {
    fn default() -> Self {
        Self {
            system: SystemSnapshot::default(),
            thermal: None,
            platform: PlatformState::default(),
            quick_toggles: QuickToggleState::default(),
            fan_curve: None,
            rpm_fan_table: None,
            gpu_switch: None,
            flexi_charger: None,
            capabilities: None,
            rgb_keyboard: None,
            camera_enabled: None,
            touchpad_enabled: None,
            airplane_enabled: None,
            brightness: None,
            blue_light: None,
            audio: None,
            display_care: None,
            anti_dust_schedule: None,
            flexi_ready: false,
            extended_api: false,
            transport_ready: false,
            transport_label: tr("Connecting to DCHU").to_owned(),
            transport_detail: tr("Opening the signed AcpiBridge device").to_owned(),
            transport_device_path: tr("Not located").to_owned(),
            driver_health: DriverHealth::default(),
            busy: false,
            status: None,
            sync_selection: false,
            extended_reads: true,
        }
    }
}
pub(crate) fn apply_driver_health(ui: &MainWindow, driver: &DriverHealth) {
    ui.set_driver_core_ready(driver.core_ready);
    ui.set_driver_core_installed(driver.core_installed);
    ui.set_driver_keyboard_ready(driver.keyboard_ready);
    ui.set_driver_keyboard_installed(driver.keyboard_installed);
    ui.set_driver_payload_ready(driver.payload_ready);
    ui.set_driver_keyboard_payload_ready(driver.keyboard_payload_ready);
    let driver_summary = if !driver.required_missing() {
        tr("Required driver ready")
    } else if !driver.payload_ready {
        tr("Driver package missing")
    } else if driver.core_installed {
        tr("Driver installed but not responding")
    } else {
        tr("Required driver missing")
    };
    let driver_detail = if driver.core_ready {
        if driver.keyboard_ready {
            tr("AcpiBridge is online. The optional keyboard filter is also available.")
        } else {
            tr("AcpiBridge is online. The current release does not require HKKbdFltr.")
        }
    } else if driver.payload_ready {
        tr("The signed OEM driver package is available next to the application.")
    } else {
        tr("Put the signed AcpiBridge package in the drivers folder.")
    };
    ui.set_driver_summary(driver_summary.into());
    ui.set_driver_detail(driver_detail.into());
    ui.set_driver_payload_path(
        if driver.payload_path.as_os_str().is_empty() {
            tr("Not located").to_owned()
        } else {
            driver.payload_path.display().to_string()
        }
        .into(),
    );
}

pub(crate) fn apply_update(ui: &MainWindow, update: UiUpdate) {
    let system = &update.system;
    ui.set_transport_ready(update.transport_ready);
    ui.set_transport_label(update.transport_label.into());
    ui.set_transport_detail(update.transport_detail.into());
    ui.set_transport_device_path(update.transport_device_path.into());
    apply_driver_health(ui, &update.driver_health);
    ui.set_busy(update.busy);
    if let Some(capabilities) = update.capabilities {
        ui.set_rgb_available(capabilities.keyboard_type != 0);
        ui.set_power_quiet_supported(capabilities.quiet);
        ui.set_power_saving_supported(capabilities.power_saving);
        ui.set_power_performance_supported(capabilities.performance);
        ui.set_power_entertainment_supported(capabilities.entertainment);
        ui.set_rpm_fan_control(capabilities.rpm_fan_control);
        ui.set_display_care_panel_oled(capabilities.oled_panel);
    }
    if let Some(care) = update.display_care {
        ui.set_display_care_available(true);
        ui.set_taskbar_auto_hide(care.taskbar_auto_hide);
        ui.set_taskbar_transparency(care.taskbar_transparency);
        ui.set_display_dark_mode(care.dark_mode);
        ui.set_monitor_ac_minutes(care.monitor_ac_minutes as i32);
        ui.set_monitor_dc_minutes(care.monitor_dc_minutes as i32);
        ui.set_screen_saver_enabled(care.screen_saver_enabled);
        ui.set_screen_saver_type(i32::from(care.screen_saver_type));
        ui.set_screen_saver_minutes(care.screen_saver_minutes as i32);
        if update.sync_selection {
            ui.set_monitor_ac_requested(care.monitor_ac_minutes as i32);
            ui.set_monitor_dc_requested(care.monitor_dc_minutes as i32);
            ui.set_screen_saver_requested_enabled(care.screen_saver_enabled);
            ui.set_screen_saver_requested_type(i32::from(care.screen_saver_type));
            ui.set_screen_saver_requested_minutes(care.screen_saver_minutes as i32);
        }
    } else if update.extended_reads {
        ui.set_display_care_available(false);
    }
    if update.extended_reads {
        // Anti-dust exists only in the extended snapshot. Never let the 1 Hz
        // light snapshot clear a previously valid state: doing so made the
        // schedule toggles alternate between "Initialize" and "Enabled".
        if let Some(running) = update.platform.anti_dust_running {
            ui.set_anti_dust_running(running);
        }
        if let Some(schedule) = update.anti_dust_schedule {
            ui.set_anti_dust_supported(true);
            if update.sync_selection {
                ui.set_anti_dust_enabled(schedule.enabled);
                ui.set_anti_dust_resume(schedule.resume);
                ui.set_anti_dust_day(if schedule.enabled {
                    i32::from(schedule.day.min(6))
                } else {
                    6
                });
                ui.set_anti_dust_hour(if schedule.enabled {
                    i32::from(schedule.hour.min(23))
                } else {
                    0
                });
            }
        }
    }
    if let Some(table) = update.rpm_fan_table {
        let cpu = table.channels[0]
            .levels
            .iter()
            .map(|level| format!("{}:{}", level.temp_up_c, level.rpm_up))
            .collect::<Vec<_>>()
            .join("  ");
        let gpu = table.channels[1]
            .levels
            .iter()
            .map(|level| format!("{}:{}", level.temp_up_c, level.rpm_up))
            .collect::<Vec<_>>()
            .join("  ");
        ui.set_rpm_curve_text(format!("CPU  {cpu}\nGPU  {gpu}").into());
        ui.set_rpm_editable(table.channels[0].present || table.channels[1].present);
        if update.sync_selection {
            for (index, level) in table.channels[0].levels.iter().enumerate() {
                let temp = i32::from(level.temp_up_c);
                let rpm = i32::from(level.rpm_up);
                match index {
                    0 => {
                        ui.set_rpm_cpu_temp_0(temp);
                        ui.set_rpm_cpu_rpm_0(rpm);
                    }
                    1 => {
                        ui.set_rpm_cpu_temp_1(temp);
                        ui.set_rpm_cpu_rpm_1(rpm);
                    }
                    2 => {
                        ui.set_rpm_cpu_temp_2(temp);
                        ui.set_rpm_cpu_rpm_2(rpm);
                    }
                    3 => {
                        ui.set_rpm_cpu_temp_3(temp);
                        ui.set_rpm_cpu_rpm_3(rpm);
                    }
                    4 => {
                        ui.set_rpm_cpu_temp_4(temp);
                        ui.set_rpm_cpu_rpm_4(rpm);
                    }
                    5 => {
                        ui.set_rpm_cpu_temp_5(temp);
                        ui.set_rpm_cpu_rpm_5(rpm);
                    }
                    6 => {
                        ui.set_rpm_cpu_temp_6(temp);
                        ui.set_rpm_cpu_rpm_6(rpm);
                    }
                    7 => {
                        ui.set_rpm_cpu_temp_7(temp);
                        ui.set_rpm_cpu_rpm_7(rpm);
                    }
                    8 => {
                        ui.set_rpm_cpu_temp_8(temp);
                        ui.set_rpm_cpu_rpm_8(rpm);
                    }
                    9 => {
                        ui.set_rpm_cpu_temp_9(temp);
                        ui.set_rpm_cpu_rpm_9(rpm);
                    }
                    _ => {}
                }
            }
            for (index, level) in table.channels[1].levels.iter().enumerate() {
                let temp = i32::from(level.temp_up_c);
                let rpm = i32::from(level.rpm_up);
                match index {
                    0 => {
                        ui.set_rpm_gpu_temp_0(temp);
                        ui.set_rpm_gpu_rpm_0(rpm);
                    }
                    1 => {
                        ui.set_rpm_gpu_temp_1(temp);
                        ui.set_rpm_gpu_rpm_1(rpm);
                    }
                    2 => {
                        ui.set_rpm_gpu_temp_2(temp);
                        ui.set_rpm_gpu_rpm_2(rpm);
                    }
                    3 => {
                        ui.set_rpm_gpu_temp_3(temp);
                        ui.set_rpm_gpu_rpm_3(rpm);
                    }
                    4 => {
                        ui.set_rpm_gpu_temp_4(temp);
                        ui.set_rpm_gpu_rpm_4(rpm);
                    }
                    5 => {
                        ui.set_rpm_gpu_temp_5(temp);
                        ui.set_rpm_gpu_rpm_5(rpm);
                    }
                    6 => {
                        ui.set_rpm_gpu_temp_6(temp);
                        ui.set_rpm_gpu_rpm_6(rpm);
                    }
                    7 => {
                        ui.set_rpm_gpu_temp_7(temp);
                        ui.set_rpm_gpu_rpm_7(rpm);
                    }
                    8 => {
                        ui.set_rpm_gpu_temp_8(temp);
                        ui.set_rpm_gpu_rpm_8(rpm);
                    }
                    9 => {
                        ui.set_rpm_gpu_temp_9(temp);
                        ui.set_rpm_gpu_rpm_9(rpm);
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(keyboard) = update.rgb_keyboard {
        // The EC keeps the last nonzero brightness while the keyboard is off.
        // The segmented control represents the effective state, so expose OFF
        // as 0 and restore the stored level only when the backlight is on.
        let display_brightness = if keyboard.enabled {
            keyboard.brightness
        } else {
            0
        };
        ui.set_rgb_enabled(keyboard.enabled);
        ui.set_rgb_brightness(i32::from(display_brightness));
        ui.set_rgb_effect(i32::from(keyboard.effect));
        ui.set_rgb_red(i32::from(keyboard.color.red));
        ui.set_rgb_green(i32::from(keyboard.color.green));
        ui.set_rgb_blue(i32::from(keyboard.color.blue));
        ui.set_rgb_boot_effect(keyboard.boot_effect);
        ui.set_rgb_sleep_enabled(keyboard.sleep_enabled);
        ui.set_rgb_sleep_hours((keyboard.sleep_seconds / 3600) as i32);
        ui.set_rgb_sleep_minutes(((keyboard.sleep_seconds / 60) % 60) as i32);
        ui.set_rgb_sleep_seconds((keyboard.sleep_seconds % 60) as i32);
        if update.sync_selection {
            ui.set_rgb_requested_red(i32::from(keyboard.color.red));
            ui.set_rgb_requested_green(i32::from(keyboard.color.green));
            ui.set_rgb_requested_blue(i32::from(keyboard.color.blue));
            ui.set_rgb_requested_brightness(i32::from(display_brightness));
            ui.set_rgb_requested_effect(i32::from(keyboard.effect));
            ui.set_rgb_requested_boot_effect(keyboard.boot_effect);
            ui.set_rgb_requested_sleep_enabled(keyboard.sleep_enabled);
            ui.set_rgb_requested_hours((keyboard.sleep_seconds / 3600) as i32);
            ui.set_rgb_requested_minutes(((keyboard.sleep_seconds / 60) % 60) as i32);
            ui.set_rgb_requested_seconds((keyboard.sleep_seconds % 60) as i32);
        }
    }
    if let Some(enabled) = update.camera_enabled {
        ui.set_camera_enabled(enabled);
        ui.set_camera_known(true);
    }
    if let Some(enabled) = update.touchpad_enabled {
        ui.set_touchpad_enabled(enabled);
        ui.set_touchpad_known(true);
    }
    if let Some(enabled) = update.airplane_enabled {
        ui.set_airplane_enabled(enabled);
        ui.set_airplane_known(true);
    }
    if let Some(brightness) = update.brightness {
        ui.set_display_brightness(i32::from(brightness));
        if update.sync_selection {
            ui.set_display_brightness_requested(i32::from(brightness));
        }
    }
    if let Some(blue_light) = update.blue_light {
        ui.set_blue_light_level(i32::from(blue_light));
        if update.sync_selection {
            ui.set_blue_light_requested(i32::from(blue_light));
        }
    }
    if let Some(audio) = update.audio {
        ui.set_audio_volume(i32::from(audio.volume_percent));
        ui.set_audio_muted(audio.muted);
        ui.set_audio_known(true);
        if update.sync_selection {
            ui.set_audio_volume_requested(i32::from(audio.volume_percent));
        }
    }
    if update.extended_reads {
        ui.set_toggle_fn_text(toggle_value_text(update.quick_toggles.fn_lock).into());
        ui.set_toggle_win_text(toggle_value_text(update.quick_toggles.win_lock).into());
        ui.set_toggle_swap_text(toggle_value_text(update.quick_toggles.swap_win_fn).into());
        ui.set_toggle_hotkeys_text(
            toggle_value_text(update.quick_toggles.hotkeys_control_center).into(),
        );
        ui.set_toggle_copilot_text(toggle_value_text(update.quick_toggles.copilot).into());
        set_toggle_properties(ui, &update.quick_toggles);
        set_fan_curve_properties(ui, update.fan_curve.as_ref(), update.sync_selection);
        ui.set_extended_api_text(
            if update.extended_api {
                tr("Available")
            } else {
                tr("Extended transport")
            }
            .into(),
        );
        ui.set_gpu_switch_text(
            update
                .gpu_switch
                .map(|gpu| {
                    format!(
                        "{} · {} 0x{:02X}",
                        localized_gpu_label(gpu.current_mode),
                        tr("mask"),
                        gpu.supported_mask
                    )
                })
                .unwrap_or_else(|| tr("Unavailable").to_owned())
                .into(),
        );
        if let Some(gpu) = update.gpu_switch {
            ui.set_gpu_current_mode(i32::from(gpu.current_mode));
            ui.set_gpu_igpu_supported(gpu.supports(GpuSwitchMode::Integrated));
            ui.set_gpu_dgpu_supported(gpu.supports(GpuSwitchMode::Discrete));
            ui.set_gpu_mshybrid_supported(gpu.supports(GpuSwitchMode::MsHybrid));
            ui.set_gpu_dynamic_supported(gpu.supports(GpuSwitchMode::Dynamic));
        }
        ui.set_flexi_ready(update.flexi_ready && update.flexi_charger.is_some());
        if let Some(charger) = &update.flexi_charger {
            ui.set_flexi_mode(i32::from(charger.mode));
            if update.sync_selection {
                ui.set_requested_flexi_mode(i32::from(charger.mode));
                ui.set_flexi_start_choice(i32::from(charger.start_threshold));
                ui.set_flexi_stop_choice(i32::from(charger.stop_threshold));
            }
            ui.set_flexi_mode_text(
                match charger.mode {
                    0 => tr("Charge to 100%").to_owned(),
                    1 => tr("Preset · 70–80%").to_owned(),
                    2 => tr("Custom thresholds").to_owned(),
                    _ => format!("{} {}", tr("Mode"), charger.mode),
                }
                .into(),
            );
            ui.set_flexi_start_text(format!("{}%", charger.start_threshold).into());
            ui.set_flexi_stop_text(format!("{}%", charger.stop_threshold).into());
            ui.set_flexi_ranges_text(format_threshold_ranges(&charger.threshold_choices).into());
        } else {
            ui.set_flexi_mode_text(
                if update.flexi_ready {
                    tr("Charge channel ready")
                } else {
                    tr("Restart to refresh the charge channel")
                }
                .into(),
            );
            ui.set_flexi_start_text("--".into());
            ui.set_flexi_stop_text("--".into());
            ui.set_flexi_ranges_text("--".into());
        }
    }

    ui.set_cpu_load(system.cpu_load_percent.round().clamp(0.0, 100.0) as i32);
    ui.set_memory_load(system.memory_percent() as i32);
    ui.set_disk_load(system.disk_percent() as i32);
    ui.set_cpu_clock(format_clock(system.cpu_clock_mhz).into());
    ui.set_cpu_clock_max(format_clock(system.cpu_max_clock_mhz).into());
    ui.set_memory_text(
        format!(
            "{} / {}",
            format_bytes(system.memory_used_bytes),
            format_bytes(system.memory_total_bytes)
        )
        .into(),
    );
    ui.set_disk_text(
        format!(
            "{} / {}",
            format_bytes(system.disk_used_bytes),
            format_bytes(system.disk_total_bytes)
        )
        .into(),
    );

    if let Some(gpu) = &system.gpu {
        ui.set_gpu_load(gpu.utilization_percent as i32);
        ui.set_gpu_memory_load(if gpu.memory_total_bytes == 0 {
            0
        } else {
            ((gpu.memory_used_bytes as f64 / gpu.memory_total_bytes as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as i32
        });
        ui.set_gpu_clock(format_clock(gpu.clock_mhz).into());
        ui.set_gpu_clock_max(format_clock(gpu.max_clock_mhz).into());
        ui.set_gpu_memory_text(
            format!(
                "{} / {}",
                format_bytes(gpu.memory_used_bytes),
                format_bytes(gpu.memory_total_bytes)
            )
            .into(),
        );
    } else {
        ui.set_gpu_load(0);
        ui.set_gpu_memory_load(0);
        ui.set_gpu_clock(tr("NVML unavailable").into());
        ui.set_gpu_clock_max("--".into());
        ui.set_gpu_memory_text("--".into());
    }

    if let Some(battery) = &system.battery {
        ui.set_battery_present(true);
        ui.set_battery_percent(i32::from(battery.percent.unwrap_or(0)));
        ui.set_battery_status_text(
            if battery.charging {
                tr("Charging")
            } else if battery.ac_online {
                tr("Connected · not charging")
            } else {
                tr("Discharging")
            }
            .into(),
        );
        ui.set_battery_ac_text(
            if battery.ac_online {
                tr("AC online")
            } else {
                tr("Battery power")
            }
            .into(),
        );
        ui.set_battery_time_text(
            if battery.ac_online && !battery.charging {
                tr("AC connected").to_owned()
            } else {
                format_battery_time(battery.seconds_remaining)
            }
            .into(),
        );
        ui.set_battery_capacity_text(
            format_battery_capacity(battery.remaining_mwh, battery.max_capacity_mwh).into(),
        );
        ui.set_battery_rate_text(format_power_rate(battery.rate_mw).into());
    } else {
        ui.set_battery_present(false);
        ui.set_battery_percent(0);
        ui.set_battery_status_text(tr("No battery detected").into());
        ui.set_battery_ac_text("--".into());
        ui.set_battery_time_text("--".into());
        ui.set_battery_capacity_text("--".into());
        ui.set_battery_rate_text("--".into());
    }

    if let Some(thermal) = update.thermal {
        ui.set_cpu_temp(format_temperature_celsius(u32::from(thermal.cpu.temperature_c)).into());
        ui.set_gpu_temp(format_temperature_celsius(u32::from(thermal.gpu.temperature_c)).into());
        ui.set_cpu_rpm(thermal.cpu.rpm.to_string().into());
        ui.set_gpu_rpm(thermal.gpu.rpm.to_string().into());
        ui.set_cpu_duty(thermal.cpu.duty_percent.to_string().into());
        ui.set_gpu_duty(thermal.gpu.duty_percent.to_string().into());
        let fan3_absent = thermal.fan3.rpm == 0 && thermal.fan3.temperature_c == 0;
        let fan4_absent = thermal.fan4.rpm == 0 && thermal.fan4.temperature_c == 0;
        ui.set_fan3_rpm(
            if fan3_absent {
                "--".to_owned()
            } else {
                thermal.fan3.rpm.to_string()
            }
            .into(),
        );
        ui.set_fan4_rpm(
            if fan4_absent {
                "--".to_owned()
            } else {
                thermal.fan4.rpm.to_string()
            }
            .into(),
        );
        ui.set_fan3_duty(
            if fan3_absent {
                "--".to_owned()
            } else {
                thermal.fan3.duty_percent.to_string()
            }
            .into(),
        );
        ui.set_fan4_duty(
            if fan4_absent {
                "--".to_owned()
            } else {
                thermal.fan4.duty_percent.to_string()
            }
            .into(),
        );
        ui.set_fan3_temp(
            if fan3_absent {
                "--".to_owned()
            } else {
                format_temperature_celsius(u32::from(thermal.fan3.temperature_c))
            }
            .into(),
        );
        ui.set_fan4_temp(
            if fan4_absent {
                "--".to_owned()
            } else {
                format_temperature_celsius(u32::from(thermal.fan4.temperature_c))
            }
            .into(),
        );
    } else {
        ui.set_cpu_temp("--°C".into());
        ui.set_gpu_temp(
            system
                .gpu
                .as_ref()
                .map(|gpu| format_temperature_celsius(gpu.temperature_c))
                .unwrap_or_else(|| "--°C".to_owned())
                .into(),
        );
        ui.set_cpu_rpm("--".into());
        ui.set_gpu_rpm("--".into());
        ui.set_fan3_rpm("--".into());
        ui.set_fan4_rpm("--".into());
        ui.set_cpu_duty("--".into());
        ui.set_gpu_duty("--".into());
        ui.set_fan3_duty("--".into());
        ui.set_fan4_duty("--".into());
        ui.set_fan3_temp("--".into());
        ui.set_fan4_temp("--".into());
    }

    if update.extended_reads {
        let power = update
            .platform
            .power_mode
            .and_then(PowerMode::from_u8)
            .unwrap_or(PowerMode::Performance);
        let fan = update
            .platform
            .fan_mode
            .and_then(FanMode::from_u8)
            .unwrap_or(FanMode::Auto);
        ui.set_power_mode(power.as_u8() as i32);
        ui.set_power_label(localized_power_label(power).into());
        ui.set_low_refresh_supported(update.platform.low_refresh_rate_supported == Some(true));
        ui.set_low_refresh_eligible(matches!(
            power,
            PowerMode::Quiet | PowerMode::PowerSaving | PowerMode::QuietPlus
        ));
        ui.set_low_refresh_enabled(update.platform.low_refresh_rate_enabled.unwrap_or(false));
        ui.set_low_refresh_known(update.platform.low_refresh_rate_enabled.is_some());
        ui.set_fan_mode(fan.as_u8() as i32);
        ui.set_fan_label(localized_fan_label(fan).into());
        let fan_offset = i32::from(update.platform.fan_offset.unwrap_or(0));
        ui.set_fan_offset_current(fan_offset);
        ui.set_slow_fan_active(update.platform.slow_fan.unwrap_or(false));
        ui.set_turbo_fan_active(update.platform.turbo_fan.unwrap_or(false));
        if let Some(capabilities) = update.capabilities {
            ui.set_slow_fan_supported(capabilities.slow_fan);
            ui.set_turbo_fan_supported(capabilities.turbo_fan);
        }
        ui.set_turbo_fan_eligible(power == PowerMode::Performance);
        if update.sync_selection {
            ui.set_requested_power_mode(power.as_u8() as i32);
            ui.set_requested_fan_mode(fan.as_u8() as i32);
            ui.set_fan_offset_requested(fan_offset);
        }
        ui.set_capabilities_text(
            update
                .platform
                .capabilities
                .map(|value| format!("0x{value:04X}"))
                .unwrap_or_else(|| "--".to_owned())
                .into(),
        );
        ui.set_platform_text(
            update
                .platform
                .platform_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "--".to_owned())
                .into(),
        );
    }

    if let Some((message, tone)) = update.status {
        ui.set_status_message(message.into());
        ui.set_status_tone(tone);
    }
}
