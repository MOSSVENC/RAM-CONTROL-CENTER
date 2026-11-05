use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::MainWindow;
use slint::ComponentHandle;

pub(crate) fn install_frame_diagnostics(ui: &MainWindow) {
    let Some(path) = std::env::var_os("NEO_CONTROL_FRAME_LOG") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let frames = Arc::new(AtomicU64::new(0));
    let renderer = Arc::new(std::sync::Mutex::new("unknown".to_owned()));

    let frames_for_notifier = Arc::clone(&frames);
    let renderer_for_notifier = Arc::clone(&renderer);
    let ui_for_notifier = ui.as_weak();
    if ui
        .window()
        .set_rendering_notifier(move |state, api| {
            if matches!(state, slint::RenderingState::RenderingSetup) {
                let report = format!("{api:?}");
                if let Ok(mut renderer) = renderer_for_notifier.lock() {
                    *renderer = report.clone();
                }
                if let Some(ui) = ui_for_notifier.upgrade() {
                    ui.set_renderer_text(report.into());
                }
            }
            if matches!(state, slint::RenderingState::BeforeRendering) {
                frames_for_notifier.fetch_add(1, Ordering::Relaxed);
            }
        })
        .is_err()
    {
        let _ = std::fs::write(&path, "renderer_notifier=unsupported\n");
        return;
    }

    let _ = std::fs::write(&path, "renderer=initializing\n");
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
