use std::time::Instant;

use neo_control_protocol::{Dchu, PlatformState, QuickToggleState, ThermalSample};
use slint::Weak;

use crate::MainWindow;
use crate::diagnostics::log_action;
use crate::driver_manager::DriverHealth;
use crate::formatting::shorten_path;
use crate::localization::tr;
use crate::runtime_state::flexi_recovery_ready;
use crate::system_controls;
use crate::telemetry::SystemSampler;
use crate::ui_update::{UiUpdate, apply_update};

pub(super) struct SnapshotInput<'a> {
    pub(super) status: Option<(String, i32)>,
    pub(super) thermal: Option<ThermalSample>,
    pub(super) driver_health: &'a DriverHealth,
}

pub(super) fn push_snapshot(
    weak: &Weak<MainWindow>,
    sampler: &mut SystemSampler,
    dchu: Option<&Dchu>,
    input: SnapshotInput<'_>,
    sync_selection: bool,
    extended_reads: bool,
) {
    let SnapshotInput {
        status,
        thermal,
        driver_health,
    } = input;
    log_action(if extended_reads {
        "worker: snapshot begin full"
    } else {
        "worker: snapshot begin light"
    });
    let system = sampler.sample();
    log_action(&format!(
        "worker: system sampled cpu={:.0}% clock={} MHz max={} MHz",
        system.cpu_load_percent, system.cpu_clock_mhz, system.cpu_max_clock_mhz
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
            driver_health: driver_health.clone(),
            busy: false,
            status,
            sync_selection,
            extended_reads,
        },
    );
}

pub(super) fn push_update(weak: &Weak<MainWindow>, update: UiUpdate) {
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
