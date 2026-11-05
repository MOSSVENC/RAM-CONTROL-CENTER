#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod boot_agent;
mod boot_service;
mod diagnostics;
mod display_care;
mod fan_curve_ui;
mod flexikey_engine;
mod flexikey_ui;
mod formatting;
mod input;
mod localization;
mod runtime_state;
mod smoke;
mod system_controls;
mod telemetry;
mod tray;
mod ui_callbacks;
mod ui_update;
mod worker;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use slint::{CloseRequestResponse, ComponentHandle};

use diagnostics::{install_frame_diagnostics, log_action};
use flexikey_engine::FlexiKeyEngine;
use flexikey_ui::wire_flexikey;
use smoke::{display_care_smoke, io_smoke, system_smoke};
use ui_callbacks::wire_ui;
use worker::{WorkerCommand, spawn_worker};

slint::include_modules!();

fn schedule_tray_start(
    weak: slint::Weak<MainWindow>,
    tray_slot: Rc<RefCell<Option<tray::Tray>>>,
    start_background: bool,
    attempt: u32,
) {
    const MAX_ATTEMPTS: u32 = 20;
    let delay_ms = if attempt == 0 { 150 } else { 250 };
    slint::Timer::single_shot(Duration::from_millis(delay_ms), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if tray_slot.borrow().is_some() {
            return;
        }
        let Some(main_window) = tray::main_window_handle(&ui) else {
            log_action(&format!(
                "tray unavailable: native window handle was not available (attempt {})",
                attempt + 1
            ));
            if attempt + 1 < MAX_ATTEMPTS {
                schedule_tray_start(ui.as_weak(), tray_slot, start_background, attempt + 1);
            }
            return;
        };
        match tray::Tray::start(ui.as_weak(), main_window) {
            Ok(tray) => {
                *tray_slot.borrow_mut() = Some(tray);
                ui.window().on_close_requested(move || {
                    tray::hide_main_window(main_window);
                    CloseRequestResponse::KeepWindowShown
                });
                if start_background {
                    tray::begin_background_hide(main_window);
                }
            }
            Err(error) => {
                // `Tray::start` owns a background thread after this point. Do
                // not retry an uncertain start and risk installing a duplicate
                // tray icon; only a missing top-level handle is retried above.
                log_action(&format!(
                    "tray unavailable: {error} (attempt {})",
                    attempt + 1
                ));
            }
        }
    });
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("--boot-agent") => std::process::exit(boot_agent::run()),
        Some("--boot-service") => std::process::exit(boot_service::run_dispatcher()),
        Some("--install-boot-service") => std::process::exit(boot_service::install()),
        Some("--uninstall-boot-service") => std::process::exit(boot_service::uninstall()),
        Some("--install-startup") => {
            let executable = match std::env::current_exe() {
                Ok(executable) => executable,
                Err(error) => {
                    eprintln!("could not resolve the current executable: {error}");
                    std::process::exit(1);
                }
            };
            match tray::install_startup(&executable) {
                Ok(()) => std::process::exit(0),
                Err(error) => {
                    eprintln!("could not install startup entry: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some("--uninstall-startup") => match tray::uninstall_startup() {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("could not remove startup entry: {error}");
                std::process::exit(1);
            }
        },
        Some("--system-smoke") => std::process::exit(system_smoke()),
        Some("--io-smoke") => std::process::exit(io_smoke()),
        Some("--display-care-smoke") => std::process::exit(display_care_smoke()),
        _ => {}
    }

    let start_background = std::env::args().any(|argument| argument == "--background");
    log_action(&format!("start_background={start_background}"));
    let _single_instance = match tray::SingleInstance::acquire() {
        Ok(Some(instance)) => Some(instance),
        Ok(None) => return Ok(()),
        Err(error) => {
            log_action(&format!("single-instance guard unavailable: {error}"));
            None
        }
    };

    let ui = MainWindow::new()?;
    if let Err(error) = slint::select_bundled_translation("zh-CN") {
        log_action(&format!("Chinese translation unavailable: {error}"));
    }
    ui.set_language_zh(true);
    ui.set_startup_enabled(tray::startup_enabled());
    let (sender, receiver) = mpsc::channel::<WorkerCommand>();
    let weak = ui.as_weak();

    let flexikey_engine = FlexiKeyEngine::start();
    let flexikey_state = wire_flexikey(&ui, flexikey_engine);
    wire_ui(&ui, sender.clone(), flexikey_state);
    install_frame_diagnostics(&ui);
    spawn_worker(receiver, weak);

    let tray_slot = Rc::new(RefCell::new(None::<tray::Tray>));
    schedule_tray_start(ui.as_weak(), Rc::clone(&tray_slot), start_background, 0);

    ui.run()?;
    let tray = tray_slot.borrow_mut().take();
    if let Some(tray) = tray {
        tray.shutdown();
    }
    Ok(())
}
