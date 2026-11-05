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
mod service_handle;
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

#[cfg(debug_assertions)]
fn write_snapshot_ppm(
    path: &std::path::Path,
    buffer: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut file = std::fs::File::create(path)?;
    write!(file, "P6\n{} {}\n255\n", buffer.width(), buffer.height())?;
    for pixel in buffer.as_slice() {
        file.write_all(&[pixel.r, pixel.g, pixel.b])?;
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn run_snapshot(page: usize, path: std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    if let Err(error) = slint::select_bundled_translation("zh-CN") {
        log_action(&format!("snapshot translation unavailable: {error}"));
    }
    ui.set_language_zh(true);
    ui.set_selected_page(page.min(10) as i32);
    ui.show()?;
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(700), move || {
        if let Some(ui) = weak.upgrade() {
            match ui.window().take_snapshot() {
                Ok(buffer) => {
                    if let Err(error) = write_snapshot_ppm(&path, &buffer) {
                        eprintln!("snapshot write failed: {error}");
                    }
                }
                Err(error) => eprintln!("snapshot capture failed: {error}"),
            }
        }
        let _ = slint::quit_event_loop();
    });
    slint::run_event_loop()?;
    Ok(())
}

#[cfg(debug_assertions)]
fn schedule_transition_capture(
    weak: slint::Weak<MainWindow>,
    directory: std::path::PathBuf,
    step: usize,
    start: std::time::Instant,
) {
    const DELAYS: [u64; 4] = [0, 60, 120, 260];
    slint::Timer::single_shot(Duration::from_millis(DELAYS[step]), {
        let weak = weak.clone();
        let directory = directory.clone();
        move || {
            eprintln!(
                "transition snapshot step={step} elapsed_ms={}",
                start.elapsed().as_millis()
            );
            if let Some(ui) = weak.upgrade() {
                let path = directory.join(format!("frame-{step}.ppm"));
                match ui.window().take_snapshot() {
                    Ok(buffer) => {
                        if let Err(error) = write_snapshot_ppm(&path, &buffer) {
                            eprintln!("transition snapshot write failed: {error}");
                        }
                    }
                    Err(error) => eprintln!("transition snapshot failed: {error}"),
                }
            }
            if step + 1 < DELAYS.len() {
                schedule_transition_capture(weak, directory, step + 1, start);
            } else {
                let _ = slint::quit_event_loop();
            }
        }
    });
}

#[cfg(debug_assertions)]
fn run_transition_snapshot(
    directory: std::path::PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    let ui = MainWindow::new()?;
    let _ = slint::select_bundled_translation("zh-CN");
    ui.set_language_zh(true);
    ui.set_selected_page(0);
    ui.show()?;
    let (sender, receiver) = mpsc::channel::<WorkerCommand>();
    let engine = FlexiKeyEngine::start();
    let flexikey_state = wire_flexikey(&ui, engine);
    wire_ui(&ui, sender, flexikey_state);
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(700), move || {
        if let Some(ui) = weak.upgrade() {
            ui.invoke_select_page(1);
            schedule_transition_capture(
                ui.as_weak(),
                directory.clone(),
                0,
                std::time::Instant::now(),
            );
        }
        drop(receiver);
    });
    slint::run_event_loop()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(debug_assertions)]
    if let Some(directory) = std::env::args().nth(1).and_then(|arg| {
        arg.strip_prefix("--transition-snapshot=")
            .map(str::to_owned)
    }) {
        return run_transition_snapshot(std::path::PathBuf::from(directory));
    }

    #[cfg(debug_assertions)]
    if let Some(page) = std::env::args()
        .nth(1)
        .and_then(|arg| arg.strip_prefix("--snapshot=").map(str::to_owned))
    {
        let path = std::env::args()
            .nth(2)
            .map(std::path::PathBuf::from)
            .ok_or("snapshot mode needs an output path")?;
        let page = page.parse::<usize>()?;
        return run_snapshot(page, path);
    }

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
