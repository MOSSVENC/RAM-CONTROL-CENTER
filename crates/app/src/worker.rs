mod input;
mod io_controls;
mod power_fan;
mod rgb;
mod snapshot;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use neo_control_protocol::{
    AntiDustSchedule, CustomFanCurve, Dchu, FanMode, GpuSwitchMode, PowerMode, QuickToggle,
    RgbColor, ThermalSample,
};
use slint::Weak;

use crate::MainWindow;
use crate::diagnostics::log_action;
use crate::driver_manager::{self, DriverHealth};
use crate::localization::tr;
use crate::telemetry::SystemSampler;
use crate::ui_update::UiUpdate;

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
}

pub(super) struct CommandOutcome {
    status: Option<(String, i32)>,
    sync_selection: bool,
}

impl CommandOutcome {
    pub(super) fn success(message: impl Into<String>) -> Self {
        Self {
            status: Some((message.into(), 1)),
            sync_selection: true,
        }
    }

    pub(super) fn failure(message: impl ToString) -> Self {
        Self::with_tone(message.to_string(), 2)
    }

    pub(super) fn with_tone(message: String, tone: i32) -> Self {
        Self {
            status: Some((message, tone)),
            sync_selection: true,
        }
    }

    pub(super) fn from_result<E: ToString>(
        result: Result<(), E>,
        success: impl Into<String>,
    ) -> Self {
        match result {
            Ok(()) => Self::success(success),
            Err(error) => Self::failure(error),
        }
    }

    pub(super) fn missing_transport() -> Self {
        Self::failure(tr("Hardware transport is unavailable.", "硬件通道不可用。"))
            .without_selection_sync()
    }

    pub(super) fn without_selection_sync(mut self) -> Self {
        self.sync_selection = false;
        self
    }
}

struct WorkerRuntime {
    weak: Weak<MainWindow>,
    sampler: SystemSampler,
    dchu: Option<Dchu>,
    status: Option<(String, i32)>,
    next_retry: Instant,
    sync_selection_once: bool,
    tick: u64,
    last_thermal: Option<ThermalSample>,
    thermal_failures: u8,
    driver_health: DriverHealth,
}

impl WorkerRuntime {
    fn new(weak: Weak<MainWindow>) -> Self {
        log_action("worker: start");
        let sampler = SystemSampler::new();
        log_action("worker: sampler ready");
        log_action("worker: opening DCHU");
        let dchu = match Dchu::open() {
            Ok(dchu) => {
                log_action(&format!("worker: DCHU opened at {}", dchu.path()));
                Some(dchu)
            }
            Err(error) => {
                log_action(&format!("worker: DCHU open failed: {error}"));
                snapshot::push_update(
                    &weak,
                    UiUpdate {
                        transport_detail: tr("Signed codec not found", "未找到签名编解码库")
                            .to_owned(),
                        status: Some((error.to_string(), 2)),
                        ..UiUpdate::default()
                    },
                );
                None
            }
        };
        let driver_health = driver_manager::detect(dchu.is_some());
        Self {
            weak,
            sampler,
            dchu,
            status: None,
            next_retry: Instant::now() + Duration::from_secs(5),
            sync_selection_once: true,
            tick: 0,
            last_thermal: None,
            thermal_failures: 0,
            driver_health,
        }
    }

    fn run(mut self, receiver: mpsc::Receiver<WorkerCommand>) {
        loop {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(WorkerCommand::ApplyPower(mode)) => {
                    self.finish(power_fan::apply_power(self.dchu.as_ref(), mode));
                }
                Ok(WorkerCommand::ApplyFan(mode)) => {
                    self.finish(power_fan::apply_fan(self.dchu.as_ref(), mode));
                }
                Ok(WorkerCommand::ApplySlowFan(enabled)) => {
                    self.finish(power_fan::apply_slow_fan(self.dchu.as_ref(), enabled));
                }
                Ok(WorkerCommand::ApplyTurboFan(enabled)) => {
                    self.finish(power_fan::apply_turbo_fan(self.dchu.as_ref(), enabled));
                }
                Ok(WorkerCommand::ApplyFanOffset(percent)) => {
                    self.finish(power_fan::apply_fan_offset(self.dchu.as_ref(), percent));
                }
                Ok(WorkerCommand::RunAntiDust) => {
                    self.finish(power_fan::run_anti_dust(self.dchu.as_ref()));
                }
                Ok(WorkerCommand::ApplyAntiDustSchedule(schedule)) => {
                    self.finish(power_fan::apply_anti_dust_schedule(
                        self.dchu.as_ref(),
                        schedule,
                    ));
                }
                Ok(WorkerCommand::ApplyRpmEdit { cpu, gpu }) => {
                    self.finish(power_fan::apply_rpm_edit(self.dchu.as_ref(), cpu, gpu));
                }
                Ok(WorkerCommand::ApplyFanCurve(curve)) => {
                    self.finish(power_fan::apply_fan_curve(self.dchu.as_ref(), curve));
                }
                Ok(WorkerCommand::ApplyQuiet(disable_dgpu)) => {
                    self.finish(power_fan::apply_quiet(self.dchu.as_ref(), disable_dgpu));
                }
                Ok(WorkerCommand::ApplyGpuSwitch(mode)) => {
                    self.finish(power_fan::apply_gpu_switch(self.dchu.as_ref(), mode));
                }
                Ok(WorkerCommand::ApplyFlexiCharger(mode, start, stop)) => {
                    self.finish(power_fan::apply_flexi_charger(
                        self.dchu.as_ref(),
                        mode,
                        start,
                        stop,
                    ));
                }
                Ok(WorkerCommand::ApplyLowRefresh(enabled)) => {
                    self.finish(power_fan::apply_low_refresh(self.dchu.as_ref(), enabled));
                }
                Ok(WorkerCommand::ApplyDisplayToggle(toggle, enabled)) => {
                    self.finish(io_controls::apply_display_toggle(
                        self.dchu.as_ref(),
                        toggle,
                        enabled,
                    ));
                }
                Ok(WorkerCommand::ApplyMonitorTimeouts(ac, dc)) => {
                    self.finish(io_controls::apply_monitor_timeouts(ac, dc));
                }
                Ok(WorkerCommand::ApplyScreenSaver(enabled, kind, minutes)) => {
                    self.finish(io_controls::apply_screen_saver(
                        self.dchu.as_ref(),
                        enabled,
                        kind,
                        minutes,
                    ));
                }
                Ok(WorkerCommand::ApplyBrightness(percent)) => {
                    self.finish(io_controls::apply_brightness(percent));
                }
                Ok(WorkerCommand::ApplyBlueLight(level)) => {
                    self.finish(io_controls::apply_blue_light(level));
                }
                Ok(WorkerCommand::ApplyVolume(percent)) => {
                    self.finish(io_controls::apply_volume(percent));
                }
                Ok(WorkerCommand::ApplyMute(muted)) => {
                    self.finish(io_controls::apply_mute(muted));
                }
                Ok(WorkerCommand::ApplyRgbBrightness(level)) => {
                    self.finish(rgb::apply_brightness(self.dchu.as_ref(), level));
                }
                Ok(WorkerCommand::ApplyRgbColor(color)) => {
                    self.finish(rgb::apply_color(self.dchu.as_ref(), color));
                }
                Ok(WorkerCommand::ApplyRgbEffect(effect)) => {
                    self.finish(rgb::apply_effect(self.dchu.as_ref(), effect));
                }
                Ok(WorkerCommand::ApplyRgbSleepTimer(enabled, seconds)) => {
                    self.finish(rgb::apply_sleep_timer(self.dchu.as_ref(), enabled, seconds));
                }
                Ok(WorkerCommand::ApplyRgbBootEffect(enabled)) => {
                    self.finish(rgb::apply_boot_effect(self.dchu.as_ref(), enabled));
                }
                Ok(WorkerCommand::ApplyCamera(enabled)) => {
                    self.finish(input::apply_camera(self.dchu.as_ref(), enabled));
                }
                Ok(WorkerCommand::ApplyTouchpad(enabled)) => {
                    self.finish(input::apply_touchpad(self.dchu.as_ref(), enabled));
                }
                Ok(WorkerCommand::ApplyAirplane(enabled)) => {
                    self.finish(input::apply_airplane(self.dchu.as_ref(), enabled));
                }
                Ok(WorkerCommand::ApplyQuickToggle(toggle, enabled)) => {
                    self.finish(input::apply_quick_toggle(
                        self.dchu.as_ref(),
                        toggle,
                        enabled,
                    ));
                }
                Ok(WorkerCommand::ApplyLockKey(vk)) => {
                    self.finish(input::apply_lock_key(vk));
                }
                Ok(WorkerCommand::Refresh) => self.refresh(),
                Err(RecvTimeoutError::Timeout) => self.on_timeout(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn finish(&mut self, outcome: CommandOutcome) {
        self.status = outcome.status;
        self.sync_selection_once |= outcome.sync_selection;
        self.push_quick(false);
    }

    fn push(&mut self, sync_selection: bool, extended_reads: bool) {
        snapshot::push_snapshot(
            &self.weak,
            &mut self.sampler,
            self.dchu.as_ref(),
            snapshot::SnapshotInput {
                status: self.status.clone(),
                thermal: self.last_thermal,
                driver_health: &self.driver_health,
            },
            sync_selection,
            extended_reads,
        );
    }

    fn push_quick(&mut self, sync_selection: bool) {
        snapshot::push_snapshot(
            &self.weak,
            &mut self.sampler,
            self.dchu.as_ref(),
            snapshot::SnapshotInput {
                status: self.status.clone(),
                thermal: self.last_thermal,
                driver_health: &self.driver_health,
            },
            sync_selection,
            false,
        );
    }

    fn refresh(&mut self) {
        if self.dchu.is_none() {
            match Dchu::open() {
                Ok(opened) => {
                    self.status = Some((
                        tr("Signed DCHU transport connected.", "签名 DCHU 通道已连接。").to_owned(),
                        1,
                    ));
                    self.dchu = Some(opened);
                }
                Err(error) => self.status = Some((error.to_string(), 2)),
            }
            self.next_retry = Instant::now() + Duration::from_secs(5);
        }
        self.refresh_thermal();
        self.driver_health = driver_manager::detect(self.dchu.is_some());
        self.push(true, true);
    }

    fn on_timeout(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if self.dchu.is_none() && Instant::now() >= self.next_retry {
            if let Ok(opened) = Dchu::open() {
                self.dchu = Some(opened);
                self.status = Some((
                    tr("Signed DCHU transport connected.", "签名 DCHU 通道已连接。").to_owned(),
                    1,
                ));
            }
            self.next_retry = Instant::now() + Duration::from_secs(5);
        }
        let full_read = self.tick == 1 || self.tick.is_multiple_of(5);
        if full_read {
            self.refresh_thermal();
            self.driver_health = driver_manager::detect(self.dchu.is_some());
        }
        let sync_selection = if full_read {
            std::mem::take(&mut self.sync_selection_once)
        } else {
            false
        };
        self.push(sync_selection, full_read);
    }

    fn refresh_thermal(&mut self) {
        let Some(dchu) = self.dchu.as_ref() else {
            self.thermal_failures = self.thermal_failures.saturating_add(1);
            if self.thermal_failures >= 3 {
                self.last_thermal = None;
            }
            return;
        };
        let started = Instant::now();
        match dchu.read_thermal() {
            Ok(sample) if sample.has_plausible_temperature() => {
                log_action(&format!(
                    "worker: thermal cpu={}C gpu={}C fan3={}C fan4={}C sampled in {} us",
                    sample.cpu.temperature_c,
                    sample.gpu.temperature_c,
                    sample.fan3.temperature_c,
                    sample.fan4.temperature_c,
                    started.elapsed().as_micros()
                ));
                self.last_thermal = Some(sample);
                self.thermal_failures = 0;
            }
            Ok(sample) => {
                self.thermal_failures = self.thermal_failures.saturating_add(1);
                log_action(&format!(
                    "worker: thermal sample ignored (cpu={}C gpu={}C fan3={}C fan4={}C, failures={})",
                    sample.cpu.temperature_c,
                    sample.gpu.temperature_c,
                    sample.fan3.temperature_c,
                    sample.fan4.temperature_c,
                    self.thermal_failures
                ));
                if self.thermal_failures >= 3 {
                    self.last_thermal = None;
                }
            }
            Err(error) => {
                self.thermal_failures = self.thermal_failures.saturating_add(1);
                log_action(&format!(
                    "worker: thermal read failed ({error}, failures={})",
                    self.thermal_failures
                ));
                if self.thermal_failures >= 3 {
                    self.last_thermal = None;
                }
            }
        }
    }
}

pub(crate) fn spawn_worker(receiver: mpsc::Receiver<WorkerCommand>, weak: Weak<MainWindow>) {
    thread::spawn(move || WorkerRuntime::new(weak).run(receiver));
}
