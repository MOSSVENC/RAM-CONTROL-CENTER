use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use neo_control_protocol::{
    CustomFanCurve, Dchu, FanCurveNodes, FanMode, FlexiChargerMode, PowerMode, QuickToggle,
};

fn main() -> ExitCode {
    let dchu = match Dchu::open() {
        Ok(dchu) => dchu,
        Err(error) => {
            eprintln!("transport: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("transport: {}", dchu.path());
    let (thermal_status, thermal_raw) = dchu.raw_read_buffer(12);
    println!("thermal_status: {thermal_status}");
    println!("thermal_raw[0..48]: {}", hex_bytes(&thermal_raw[..48]));

    let (feature_status, features) = dchu.raw_read_settings(7, 0, 256);
    println!("feature_status: {feature_status}");
    println!("feature[0..64]: {}", hex_bytes(&features[..64]));
    let (low_refresh_support_status, low_refresh_support) = dchu.raw_read_settings(1, 27, 1);
    println!(
        "low_refresh_support_status: {low_refresh_support_status}, value: {}",
        low_refresh_support.first().copied().unwrap_or_default()
    );
    let (low_refresh_status, low_refresh) = dchu.raw_read_settings(1, 28, 1);
    println!(
        "low_refresh_status: {low_refresh_status}, value: {}",
        low_refresh.first().copied().unwrap_or_default()
    );
    let (gpu13_status, gpu13) = dchu.raw_read_buffer(13);
    println!("gpu13_status: {gpu13_status}");
    println!("gpu13[0..64]: {}", hex_bytes(&gpu13[..64]));
    let (identity_status, identity) = dchu.raw_read_buffer(17);
    println!("identity_status: {identity_status}");
    println!("identity[0..32]: {}", hex_bytes(&identity[..32]));
    for (page, offset, length) in [(1, 49, 1), (1, 55, 1), (1, 61, 1)] {
        let (status, bytes) = dchu.raw_read_settings(page, offset, length);
        println!(
            "settings[{page},{offset},{length}]: status={status} bytes={}",
            hex_bytes(&bytes)
        );
    }

    for command in [16, 70] {
        let (status, value) = dchu.raw_read_integer(command);
        println!("integer[{command}]: status={status} value={value}");
    }
    for (page, offset, length) in [
        (1, 1, 1),
        (1, 16, 1),
        (1, 36, 1),
        (1, 37, 1),
        (1, 38, 1),
        (1, 43, 1),
        (1, 44, 1),
        (4, 5, 1),
        (4, 7, 1),
    ] {
        let (status, bytes) = dchu.raw_read_settings(page, offset, length);
        println!(
            "settings[{page},{offset},{length}]: status={status} bytes={}",
            hex_bytes(&bytes)
        );
    }

    match dchu.read_capabilities() {
        Ok(caps) => println!("capabilities_decoded: {caps:?}"),
        Err(error) => println!("capabilities_error: {error}"),
    }
    match dchu.read_rgb_keyboard_state() {
        Ok(state) => println!("rgb_keyboard: {state:?}"),
        Err(error) => println!("rgb_keyboard_error: {error}"),
    }
    println!("camera_enabled: {:?}", dchu.read_camera_enabled());
    match dchu.read_rpm_fan_table() {
        Ok(table) => {
            for (index, channel) in table.channels.iter().enumerate() {
                println!(
                    "rpm_fan[{index}]: present={} id=0x{:02X} max={} turbo={} levels={:?}",
                    channel.present,
                    channel.id,
                    channel.max_rpm,
                    channel.max_turbo_rpm,
                    channel.levels
                );
            }
        }
        Err(error) => println!("rpm_fan_error: {error}"),
    }
    println!("touchpad_enabled: {:?}", dchu.read_touchpad_enabled());

    match dchu.read_thermal() {
        Ok(sample) => {
            print_channel(
                "cpu",
                sample.cpu.rpm,
                sample.cpu.duty_percent,
                sample.cpu.temperature_c,
            );
            print_channel(
                "gpu",
                sample.gpu.rpm,
                sample.gpu.duty_percent,
                sample.gpu.temperature_c,
            );
            print_channel(
                "fan3",
                sample.fan3.rpm,
                sample.fan3.duty_percent,
                sample.fan3.temperature_c,
            );
            print_channel(
                "fan4",
                sample.fan4.rpm,
                sample.fan4.duty_percent,
                sample.fan4.temperature_c,
            );
        }
        Err(error) => {
            eprintln!("telemetry: {error}");
            return ExitCode::FAILURE;
        }
    }

    let state = dchu.read_platform_state();
    println!("power: {:?}", state.power_mode.and_then(PowerMode::from_u8));
    println!("fan: {:?}", state.fan_mode.and_then(FanMode::from_u8));
    println!("fan_offset: {:?}", state.fan_offset);
    println!("power_policy: {:?}", state.power_policy);
    println!("capabilities: {:?}", state.capabilities);
    println!("platform_id: {:?}", state.platform_id);
    let (fan_curve_status, fan_curve_raw) = dchu.raw_read_buffer(13);
    println!("fan_curve_raw_status: {fan_curve_status}");
    println!("fan_curve_raw[0..64]: {}", hex_bytes(&fan_curve_raw[..64]));
    match dchu.read_fan_curve() {
        Ok(curve) => {
            println!(
                "fan_curve: count={} initial_mode={}",
                curve.fan_count, curve.initial_mode
            );
            for (index, channel) in curve.channels.iter().enumerate() {
                println!(
                    "fan_curve[{index}]: {:?}",
                    channel
                        .points
                        .map(|point| (point.temp_c, point.duty_percent()))
                );
            }
            if std::env::args().any(|argument| argument == "--curve-smoke") {
                let previous_curve = curve;
                let previous_mode = state.fan_mode.and_then(FanMode::from_u8);
                let mut nodes = [None; 3];
                if !previous_curve.channels[0].is_unused() {
                    nodes[0] = Some(FanCurveNodes {
                        t2: 60,
                        d2: 40,
                        t3: 80,
                        d3: 60,
                    });
                }
                if !previous_curve.channels[1].is_unused() {
                    nodes[1] = Some(FanCurveNodes {
                        t2: 60,
                        d2: 35,
                        t3: 80,
                        d3: 55,
                    });
                }
                if nodes.iter().all(Option::is_none) {
                    eprintln!("curve-smoke: no populated fan channels; no write attempted");
                    return ExitCode::FAILURE;
                }
                println!("curve-smoke: applying a safe higher-duty Custom curve");
                if let Err(error) = dchu.set_custom_fan_curve(CustomFanCurve { channels: nodes }) {
                    eprintln!("curve-smoke: write failed: {error}");
                    return ExitCode::FAILURE;
                }
                match dchu.read_fan_curve() {
                    Ok(readback) => {
                        println!(
                            "curve-smoke: runtime readback {:?}",
                            readback.channels[0].points
                        );
                    }
                    Err(error) => {
                        eprintln!("curve-smoke: readback failed: {error}");
                        return ExitCode::FAILURE;
                    }
                }
                if let Err(error) = dchu.write_fan_curve_table(&previous_curve) {
                    eprintln!("curve-smoke: could not restore previous curve table: {error}");
                    return ExitCode::FAILURE;
                }
                if let Some(previous_mode) = previous_mode {
                    if let Err(error) = dchu.set_fan_mode(previous_mode) {
                        eprintln!("curve-smoke: could not restore fan mode: {error}");
                        return ExitCode::FAILURE;
                    }
                    println!("curve-smoke: restored fan mode {:?}", previous_mode);
                }
                let restored = match dchu.read_fan_curve() {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("curve-smoke: final readback failed: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                println!(
                    "curve-smoke: restored table {:?}",
                    restored.channels[0].points
                );
                println!("curve-smoke: block-14 write and block-13 readback succeeded");
            }
        }
        Err(error) => println!("fan_curve_error: {error}"),
    }
    let toggles = dchu.read_quick_toggles();
    println!("quick_toggles: {toggles:?}");
    println!("extended_api: {}", dchu.has_extended_api());
    if dchu.has_extended_api() {
        match dchu.read_gpu_switch() {
            Ok(gpu) => {
                println!("gpu_switch: {gpu:?}");
            }
            Err(error) => println!("gpu_switch_error: {error}"),
        }
        let run_flexi =
            std::env::args().any(|argument| argument == "--flexi" || argument == "--flexi-smoke");
        if run_flexi {
            match dchu.read_flexi_charger() {
                Ok(charger) => println!("flexicharger: {charger:?}"),
                Err(error) => println!("flexicharger_error: {error}"),
            }
        }
        if std::env::args().any(|argument| argument == "--flexi-smoke") {
            let charger = match dchu.read_flexi_charger() {
                Ok(charger) => charger,
                Err(error) => {
                    eprintln!("flexi-smoke: could not read current state: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let Some(mode) = FlexiChargerMode::from_u8(charger.mode) else {
                eprintln!(
                    "flexi-smoke: current mode {} is not recognized; no write attempted",
                    charger.mode
                );
                return ExitCode::FAILURE;
            };
            println!(
                "flexi-smoke: re-applying mode {mode:?} at {}–{}%",
                charger.start_threshold, charger.stop_threshold
            );
            if let Err(error) =
                dchu.set_flexi_charger(mode, charger.start_threshold, charger.stop_threshold)
            {
                eprintln!("flexi-smoke: write failed: {error}");
                return ExitCode::FAILURE;
            }
            println!("flexi-smoke: FlexiCharger values read back successfully");
        }
    }

    if std::env::args().any(|argument| argument == "--turbo-smoke") {
        let previous = dchu.read_platform_state();
        let Some(previous_power) = previous.power_mode.and_then(PowerMode::from_u8) else {
            eprintln!("turbo-smoke: current power mode unavailable");
            return ExitCode::FAILURE;
        };
        println!("turbo-smoke: entering Performance and cycling Turbo fan");
        if let Err(error) = dchu.set_power_mode(PowerMode::Performance) {
            eprintln!("turbo-smoke: Performance failed: {error}");
            return ExitCode::FAILURE;
        }
        if let Err(error) = dchu.set_turbo_fan(true) {
            eprintln!("turbo-smoke: enable failed: {error}");
            let _ = dchu.set_power_mode(previous_power);
            return ExitCode::FAILURE;
        }
        println!(
            "turbo-smoke: enabled state {:?}",
            dchu.read_platform_state().turbo_fan
        );
        if let Err(error) = dchu.set_turbo_fan(false) {
            eprintln!("turbo-smoke: disable failed: {error}");
            let _ = dchu.set_power_mode(previous_power);
            return ExitCode::FAILURE;
        }
        println!(
            "turbo-smoke: disabled state {:?}",
            dchu.read_platform_state().turbo_fan
        );
        if let Err(error) = dchu.set_power_mode(previous_power) {
            eprintln!("turbo-smoke: restore failed: {error}");
            return ExitCode::FAILURE;
        }
        println!(
            "turbo-smoke: restored {:?}",
            dchu.read_platform_state().power_mode
        );
    }

    if std::env::args().any(|argument| argument == "--rgb-color-smoke") {
        let state = match dchu.read_rgb_keyboard_state() {
            Ok(state) => state,
            Err(error) => {
                eprintln!("rgb-color-smoke: could not read current state: {error}");
                return ExitCode::FAILURE;
            }
        };
        println!("rgb-color-smoke: reapplying current color and cycling effects");
        for action in [
            ("color", 0_u8),
            ("breathe", 2_u8),
            ("cycle", 3_u8),
            ("static", 8_u8),
        ] {
            let result = if action.1 == 0 {
                dchu.set_rgb_keyboard_color(state.color)
            } else {
                dchu.set_rgb_keyboard_effect(action.1)
            };
            if let Err(error) = result {
                eprintln!("rgb-color-smoke: {} failed: {error}", action.0);
                return ExitCode::FAILURE;
            }
            println!("rgb-color-smoke: {} applied", action.0);
        }
        match dchu.read_rgb_keyboard_state() {
            Ok(actual) => println!("rgb-color-smoke: final state {actual:?}"),
            Err(error) => {
                eprintln!("rgb-color-smoke: final read failed: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    if std::env::args().any(|argument| argument == "--rpm-write-smoke") {
        let table = match dchu.read_rpm_fan_table() {
            Ok(table) => table,
            Err(error) => {
                eprintln!("rpm-write-smoke: could not read current table: {error}");
                return ExitCode::FAILURE;
            }
        };
        println!("rpm-write-smoke: re-applying current ten-point tables");
        if let Err(error) = dchu.write_rpm_fan_table(&table) {
            eprintln!("rpm-write-smoke: write failed: {error}");
            return ExitCode::FAILURE;
        }
        println!("rpm-write-smoke: ten-point tables read back successfully");
    }

    if std::env::args().any(|argument| argument == "--rgb-settings-smoke") {
        let state = match dchu.read_rgb_keyboard_state() {
            Ok(state) => state,
            Err(error) => {
                eprintln!("rgb-settings-smoke: could not read current state: {error}");
                return ExitCode::FAILURE;
            }
        };
        println!("rgb-settings-smoke: re-applying brightness, sleep timer, and boot effect");
        if let Err(error) = dchu.set_rgb_keyboard_brightness(state.brightness) {
            eprintln!("rgb-settings-smoke: brightness failed: {error}");
            return ExitCode::FAILURE;
        }
        if let Err(error) =
            dchu.set_rgb_keyboard_sleep_timer(state.sleep_enabled, state.sleep_seconds)
        {
            eprintln!("rgb-settings-smoke: sleep timer failed: {error}");
            return ExitCode::FAILURE;
        }
        if let Err(error) = dchu.set_rgb_keyboard_boot_effect(state.boot_effect) {
            eprintln!("rgb-settings-smoke: boot effect failed: {error}");
            return ExitCode::FAILURE;
        }
        println!("rgb-settings-smoke: settings read back successfully");
    }

    if std::env::args().any(|argument| argument == "--toggle-smoke") {
        let Some(current) = toggles.value(QuickToggle::Copilot) else {
            eprintln!("toggle-smoke: Copilot toggle state is unavailable; no write attempted");
            return ExitCode::FAILURE;
        };
        println!("toggle-smoke: re-applying current Copilot toggle value {current}");
        if let Err(error) = dchu.set_quick_toggle(QuickToggle::Copilot, current) {
            eprintln!("toggle-smoke: write failed: {error}");
            return ExitCode::FAILURE;
        }
        println!("toggle-smoke: quick-toggle values read back successfully");
    }

    if std::env::args().any(|argument| argument == "--low-refresh-smoke") {
        let Some(current) = state.low_refresh_rate_enabled else {
            eprintln!("low-refresh-smoke: state is unavailable; no write attempted");
            return ExitCode::FAILURE;
        };
        println!("low-refresh-smoke: re-applying current value {current}");
        if let Err(error) = dchu.set_low_refresh_rate(current) {
            eprintln!("low-refresh-smoke: write failed: {error}");
            return ExitCode::FAILURE;
        }
        println!("low-refresh-smoke: setting read back successfully");
    }

    if let Some(encoded) = std::env::args()
        .find_map(|argument| argument.strip_prefix("--rgb-set-color=").map(str::to_owned))
    {
        let channels = encoded
            .split(',')
            .map(str::trim)
            .map(str::parse::<u8>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(channels) = channels else {
            eprintln!("rgb-set-color: expected R,G,B in 0..255");
            return ExitCode::FAILURE;
        };
        if channels.len() != 3 {
            eprintln!("rgb-set-color: expected exactly three channels");
            return ExitCode::FAILURE;
        }
        let color = neo_control_protocol::RgbColor {
            red: channels[0],
            green: channels[1],
            blue: channels[2],
        };
        return match dchu.set_rgb_keyboard_color(color) {
            Ok(()) => {
                println!("rgb-set-color: applied {color:?}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("rgb-set-color: failed: {error}");
                ExitCode::FAILURE
            }
        };
    }

    if std::env::args().any(|argument| argument == "--anti-dust-stability") {
        return anti_dust_stability(&dchu);
    }

    if std::env::args().any(|argument| argument == "--rgb-sleep-stability-smoke") {
        return rgb_sleep_stability_smoke(&dchu);
    }

    if std::env::args().any(|argument| argument == "--write-smoke") {
        return write_smoke(&dchu, state);
    }
    ExitCode::SUCCESS
}

fn anti_dust_stability(dchu: &Dchu) -> ExitCode {
    println!("anti-dust-stability: sampling schedule and run byte for 20 seconds");
    let mut last_schedule = None;
    let mut schedule_changes = 0_u32;
    let mut errors = 0_u32;
    for index in 0..20 {
        match dchu.read_anti_dust_schedule() {
            Ok(schedule) => {
                let (_, running_raw) = dchu.raw_read_settings(4, 80, 1);
                let running = running_raw.first().copied().unwrap_or_default();
                println!(
                    "anti-dust-stability[{index:02}]: schedule={schedule:?} running_byte={running}"
                );
                if last_schedule.is_some_and(|previous| previous != schedule) {
                    schedule_changes += 1;
                }
                last_schedule = Some(schedule);
            }
            Err(error) => {
                errors += 1;
                eprintln!("anti-dust-stability[{index:02}]: read failed: {error}");
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
    println!("anti-dust-stability: errors={errors} schedule_changes={schedule_changes}");
    if errors == 0 && schedule_changes == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn rgb_sleep_stability_smoke(dchu: &Dchu) -> ExitCode {
    let previous = match dchu.read_rgb_keyboard_state() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("rgb-sleep-stability-smoke: initial read failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("rgb-sleep-stability-smoke: previous={previous:?}");
    if let Err(error) = dchu.set_rgb_keyboard_brightness(0) {
        eprintln!("rgb-sleep-stability-smoke: disabling backlight failed: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = dchu.set_rgb_keyboard_sleep_timer(false, previous.sleep_seconds) {
        eprintln!("rgb-sleep-stability-smoke: disabling sleep failed: {error}");
        let _ = dchu.set_rgb_keyboard_brightness(if previous.enabled {
            previous.brightness.max(1)
        } else {
            0
        });
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    for index in 0..12 {
        match dchu.read_rgb_keyboard_state() {
            Ok(state) => {
                println!(
                    "rgb-sleep-stability-smoke[{index:02}]: enabled={} sleep_enabled={} seconds={}",
                    state.enabled, state.sleep_enabled, state.sleep_seconds
                );
                if state.sleep_enabled || state.enabled {
                    eprintln!("rgb-sleep-stability-smoke: state changed unexpectedly");
                    failed = true;
                }
            }
            Err(error) => {
                eprintln!("rgb-sleep-stability-smoke[{index:02}]: read failed: {error}");
                failed = true;
            }
        }
        thread::sleep(Duration::from_secs(1));
    }

    let sleep_restore =
        dchu.set_rgb_keyboard_sleep_timer(previous.sleep_enabled, previous.sleep_seconds);
    let brightness_restore = dchu.set_rgb_keyboard_brightness(if previous.enabled {
        previous.brightness.max(1)
    } else {
        0
    });
    if let Err(error) = sleep_restore {
        eprintln!("rgb-sleep-stability-smoke: sleep restore failed: {error}");
        failed = true;
    }
    if let Err(error) = brightness_restore {
        eprintln!("rgb-sleep-stability-smoke: brightness restore failed: {error}");
        failed = true;
    }
    match dchu.read_rgb_keyboard_state() {
        Ok(restored) => {
            println!("rgb-sleep-stability-smoke: restored={restored:?}");
            if restored.sleep_enabled != previous.sleep_enabled
                || restored.sleep_seconds != previous.sleep_seconds
                || restored.enabled != previous.enabled
                || restored.brightness != previous.brightness
            {
                eprintln!("rgb-sleep-stability-smoke: restore verification failed");
                failed = true;
            }
        }
        Err(error) => {
            eprintln!("rgb-sleep-stability-smoke: restore read failed: {error}");
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn write_smoke(dchu: &Dchu, state: neo_control_protocol::PlatformState) -> ExitCode {
    let Some(power) = state.power_mode.and_then(PowerMode::from_u8) else {
        eprintln!("write-smoke: current power mode is not recognized; no write attempted");
        return ExitCode::FAILURE;
    };
    println!("write-smoke: re-applying current power mode {power:?}");
    if let Err(error) = dchu.set_power_mode(power) {
        eprintln!("write-smoke: power write failed: {error}");
        return ExitCode::FAILURE;
    }

    let Some(fan_value) = state.fan_mode else {
        eprintln!("write-smoke: current fan mode is unavailable; no write attempted");
        return ExitCode::FAILURE;
    };
    let Some(fan) = FanMode::from_u8(fan_value) else {
        eprintln!(
            "write-smoke: fan mode {fan_value} is outside the safe Auto/Maximum slice; no write attempted"
        );
        return ExitCode::FAILURE;
    };
    println!("write-smoke: re-applying current fan mode {fan:?}");
    if let Err(error) = dchu.set_fan_mode(fan) {
        eprintln!("write-smoke: fan write failed: {error}");
        return ExitCode::FAILURE;
    }
    println!("write-smoke: power and fan values read back successfully");
    ExitCode::SUCCESS
}

fn print_channel(name: &str, rpm: u32, duty: u8, temp: u8) {
    println!("{name}: rpm={rpm} duty={duty}% temp={temp}C");
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
