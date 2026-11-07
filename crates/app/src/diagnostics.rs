use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::MainWindow;
use crate::localization::tr;
use slint::ComponentHandle;

pub(crate) fn install_frame_diagnostics(ui: &MainWindow) {
    let path = std::env::var_os("NEO_CONTROL_FRAME_LOG").map(std::path::PathBuf::from);
    ui.set_renderer_ready(false);
    ui.set_renderer_text(tr("Detecting", "检测中").into());

    let frames = Arc::new(AtomicU64::new(0));
    let ready = Arc::new(AtomicBool::new(false));
    let renderer = Arc::new(std::sync::Mutex::new("unknown".to_owned()));
    let frames_for_notifier = Arc::clone(&frames);
    let ready_for_notifier = Arc::clone(&ready);
    let renderer_for_notifier = Arc::clone(&renderer);
    let ui_for_notifier = ui.as_weak();
    let path_for_notifier = path.clone();
    if ui
        .window()
        .set_rendering_notifier(move |state, api| {
            if !ready_for_notifier.swap(true, Ordering::Relaxed) {
                let report = format!("{api:?}");
                if let Ok(mut renderer) = renderer_for_notifier.lock() {
                    *renderer = report.clone();
                }
                if let Some(ui) = ui_for_notifier.upgrade() {
                    ui.set_renderer_text(report.clone().into());
                    ui.set_renderer_ready(true);
                }
                if let Some(path) = path_for_notifier.as_ref() {
                    let _ = std::fs::write(path, format!("renderer={report}\n"));
                }
            }
            if matches!(state, slint::RenderingState::BeforeRendering) {
                frames_for_notifier.fetch_add(1, Ordering::Relaxed);
            }
        })
        .is_err()
    {
        ui.set_renderer_text(tr("Unavailable", "不可用").into());
        return;
    }

    // A backend report must not remain at its placeholder forever if a
    // platform fails to deliver a rendering callback. A later first frame
    // still replaces this message with the detected API.
    let ui_for_timeout = ui.as_weak();
    let ready_for_timeout = Arc::clone(&ready);
    slint::Timer::single_shot(Duration::from_millis(1_500), move || {
        if ready_for_timeout.load(Ordering::Relaxed) {
            return;
        }
        if let Some(ui) = ui_for_timeout.upgrade() {
            ui.set_renderer_ready(false);
            ui.set_renderer_text(
                tr("Renderer initialization timed out", "渲染器初始化超时").into(),
            );
        }
    });

    let Some(path) = path else {
        return;
    };
    thread::spawn(move || {
        let mut interval = 0_u64;
        loop {
            thread::sleep(Duration::from_secs(2));
            interval += 1;
            let frame_count = frames.swap(0, Ordering::Relaxed);
            let renderer = renderer
                .lock()
                .map(|value| value.clone())
                .unwrap_or_else(|_| "unknown".to_owned());
            use std::io::Write as _;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = writeln!(
                    file,
                    "tick={interval} frames={frame_count} renderer={renderer}"
                );
            }
        }
    });
}

pub(crate) fn log_action(message: &str) {
    let Some(path) = std::env::var_os("NEO_CONTROL_ACTION_LOG") else {
        return;
    };
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}
