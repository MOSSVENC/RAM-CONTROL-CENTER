use neo_control_protocol::Dchu;

use crate::input::send_touchpad_chord;
use crate::system_controls;

pub(crate) fn system_smoke() -> i32 {
    let mut ok = true;
    match system_controls::screen_brightness() {
        Ok(level) => match system_controls::set_screen_brightness(level) {
            Ok(()) => println!("brightness: {level}% reapplied"),
            Err(error) => {
                eprintln!("brightness: {error}");
                ok = false;
            }
        },
        Err(error) => {
            eprintln!("brightness read: {error}");
            ok = false;
        }
    }
    match system_controls::audio_state() {
        Ok(audio) => {
            if let Err(error) = system_controls::set_audio_volume(audio.volume_percent) {
                eprintln!("volume: {error}");
                ok = false;
            } else if let Err(error) = system_controls::set_audio_mute(audio.muted) {
                eprintln!("mute: {error}");
                ok = false;
            } else {
                println!(
                    "audio: {}% {} reapplied",
                    audio.volume_percent,
                    if audio.muted { "muted" } else { "unmuted" }
                );
            }
        }
        Err(error) => {
            eprintln!("audio read: {error}");
            ok = false;
        }
    }
    match system_controls::airplane_mode_enabled() {
        Ok(enabled) => println!("airplane: {enabled}"),
        Err(error) => {
            eprintln!("airplane read: {error}");
            ok = false;
        }
    }
    match Dchu::open().and_then(|dchu| {
        let level = dchu
            .raw_read_settings(1, 34, 1)
            .1
            .first()
            .copied()
            .unwrap_or(0);
        system_controls::set_blue_light_filter(level).map_err(|error| {
            neo_control_protocol::DchuError::ControlFailed {
                operation: "system smoke blue light",
                detail: error,
            }
        })?;
        println!("blue_light: {level} reapplied");
        Ok(())
    }) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("blue_light: {error}");
            ok = false;
        }
    }
    if ok { 0 } else { 1 }
}

pub(crate) fn io_smoke() -> i32 {
    let Ok(dchu) = Dchu::open() else {
        eprintln!("io-smoke: AcpiBridge unavailable");
        return 1;
    };
    let mut ok = true;

    if let Some(previous) = dchu.read_camera_enabled() {
        let target = !previous;
        match dchu.set_camera_enabled(target) {
            Ok(()) if dchu.read_camera_enabled() == Some(target) => {
                println!("camera: toggled to {target}");
                if let Err(error) = dchu.set_camera_enabled(previous) {
                    eprintln!("camera restore: {error}");
                    ok = false;
                } else {
                    println!("camera: restored to {previous}");
                }
            }
            Ok(()) => {
                eprintln!("camera: readback mismatch");
                let _ = dchu.set_camera_enabled(previous);
                ok = false;
            }
            Err(error) => {
                eprintln!("camera toggle: {error}");
                ok = false;
            }
        }
    }

    if let Some(previous) = dchu.read_touchpad_enabled() {
        let target = !previous;
        let result = send_touchpad_chord().and_then(|()| {
            dchu.set_touchpad_enabled(target)
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(()) if dchu.read_touchpad_enabled() == Some(target) => {
                println!("touchpad: toggled to {target}");
                let _ = send_touchpad_chord();
                if let Err(error) = dchu.set_touchpad_enabled(previous) {
                    eprintln!("touchpad restore: {error}");
                    ok = false;
                } else {
                    println!("touchpad: restored to {previous}");
                }
            }
            Ok(()) => {
                eprintln!("touchpad: readback mismatch");
                let _ = send_touchpad_chord();
                let _ = dchu.set_touchpad_enabled(previous);
                ok = false;
            }
            Err(error) => {
                eprintln!("touchpad toggle: {error}");
                ok = false;
            }
        }
    }

    match system_controls::airplane_mode_enabled() {
        Ok(previous) => {
            let target = !previous;
            match system_controls::set_airplane_mode(target) {
                Ok(()) if system_controls::airplane_mode_enabled() == Ok(target) => {
                    println!("airplane: toggled to {target}");
                    let _ = dchu.notify_airplane_mode(target);
                    if let Err(error) = system_controls::set_airplane_mode(previous) {
                        eprintln!("airplane restore: {error}");
                        ok = false;
                    } else if system_controls::airplane_mode_enabled() != Ok(previous) {
                        eprintln!("airplane: restore readback mismatch");
                        ok = false;
                    } else {
                        let _ = dchu.notify_airplane_mode(previous);
                        println!("airplane: restored to {previous}");
                    }
                }
                Ok(()) => {
                    eprintln!("airplane: readback mismatch");
                    let _ = system_controls::set_airplane_mode(previous);
                    ok = false;
                }
                Err(error) => {
                    eprintln!("airplane toggle: {error}");
                    ok = false;
                }
            }
        }
        Err(error) => {
            eprintln!("airplane read: {error}");
            ok = false;
        }
    }

    if ok { 0 } else { 1 }
}

pub(crate) fn display_care_smoke() -> i32 {
    let mut ok = true;
    let state = match system_controls::display_care_state() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("display-care read: {error}");
            return 1;
        }
    };
    println!(
        "display-care: taskbar_hide={} transparency={} dark={} monitor_ac={} monitor_dc={} saver_enabled={} saver_type={} saver_minutes={}",
        state.taskbar_auto_hide,
        state.taskbar_transparency,
        state.dark_mode,
        state.monitor_ac_minutes,
        state.monitor_dc_minutes,
        state.screen_saver_enabled,
        state.screen_saver_type,
        state.screen_saver_minutes,
    );

    for (name, result) in [
        (
            "taskbar_auto_hide",
            system_controls::set_taskbar_auto_hide(state.taskbar_auto_hide),
        ),
        (
            "taskbar_transparency",
            system_controls::set_taskbar_transparency(state.taskbar_transparency),
        ),
        ("dark_mode", system_controls::set_dark_mode(state.dark_mode)),
        (
            "monitor_timeouts",
            system_controls::set_monitor_timeouts(
                state.monitor_ac_minutes,
                state.monitor_dc_minutes,
            ),
        ),
        (
            "screen_saver",
            system_controls::set_screen_saver(
                state.screen_saver_enabled,
                state.screen_saver_type,
                state.screen_saver_minutes,
            ),
        ),
    ] {
        match result {
            Ok(()) => println!("{name}: same-value readback passed"),
            Err(error) => {
                eprintln!("{name}: {error}");
                ok = false;
            }
        }
    }

    match Dchu::open() {
        Ok(dchu) => {
            for offset in [45_i32, 46, 47, 48] {
                let Some(value) = dchu.read_setting_byte(0, offset) else {
                    eprintln!("dchu offset {offset}: value unavailable");
                    ok = false;
                    continue;
                };
                match dchu.write_setting_byte(0, offset, value) {
                    Ok(()) => {
                        println!("dchu offset {offset}: same-value {value} readback passed")
                    }
                    Err(error) => {
                        eprintln!("dchu offset {offset}: {error}");
                        ok = false;
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("display-care DCHU open: {error}");
            ok = false;
        }
    }

    if ok { 0 } else { 1 }
}
