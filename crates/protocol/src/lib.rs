//! Direct, in-process transport to the signed AcpiBridge device.
//!
//! Neo Control does not load the vendor Control Center codec DLL, launch
//! `CC40.exe`, replace the signed driver, or write EC/BIOS firmware. On Windows
//! this crate opens the signed AcpiBridge device and sends the recovered DCHU
//! IOCTL frames directly. Non-Windows builds expose the same data model with an
//! `UnsupportedPlatform` error so the UI can still be developed.

use std::fmt;

mod flexikey;
mod keyboard_filter;
#[cfg(windows)]
mod win_device;
pub use flexikey::{
    FlexiKeyAction, FlexiKeyBinding, FlexiKeyDevice, FlexiKeyError, FlexiKeyProfile, FlexiKeyStore,
    MacroStep, MacroStepKind,
};
pub use keyboard_filter::{
    DEVICE_PATH as KEYBOARD_FILTER_DEVICE_PATH, FilterMacro, FilterRecord,
    IOCTL_ADD_FILTER_MACRO as KEYBOARD_FILTER_ADD_MACRO_IOCTL,
    IOCTL_CLEAR_FILTER_MACRO as KEYBOARD_FILTER_CLEAR_MACRO_IOCTL,
    IOCTL_ENABLE_FILTER as KEYBOARD_FILTER_ENABLE_IOCTL,
    IOCTL_ENABLE_USB_DEVICE as KEYBOARD_FILTER_ENABLE_USB_IOCTL,
    IOCTL_REMOVE_FILTER_MACRO as KEYBOARD_FILTER_REMOVE_MACRO_IOCTL, KeyboardFilter,
    encode_filter_macro, encode_filter_macro_removal,
};

pub const TELEMETRY_COMMAND: i32 = 12;
pub const POWER_COMMAND: i32 = 121;
pub const FAN_COMMAND: i32 = 121;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanChannel {
    pub rpm: u32,
    pub duty_percent: u8,
    pub temperature_c: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThermalSample {
    pub cpu: FanChannel,
    pub gpu: FanChannel,
    pub fan3: FanChannel,
    pub fan4: FanChannel,
}

/// One point in the EC's four-point fan curve. Duty is kept in the vendor's
/// native 0..=255 representation so a read can be round-tripped exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanCurvePoint {
    pub temp_c: u8,
    pub duty_raw: u8,
}

impl FanCurvePoint {
    pub fn duty_percent(self) -> u8 {
        duty_from_raw(self.duty_raw)
    }

    pub const fn is_unused(self) -> bool {
        self.temp_c == 0 && self.duty_raw == 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanCurveChannel {
    pub points: [FanCurvePoint; 4],
}

impl FanCurveChannel {
    pub const fn is_unused(self) -> bool {
        self.points[0].is_unused()
            && self.points[1].is_unused()
            && self.points[2].is_unused()
            && self.points[3].is_unused()
    }
}

impl std::ops::Index<usize> for FanCurveChannel {
    type Output = FanCurvePoint;

    fn index(&self, index: usize) -> &Self::Output {
        &self.points[index]
    }
}

/// Runtime fan table read from DCHU command 13 (EEVT). The EC echoes the table
/// it is actually executing, which makes this the authoritative readback after
/// a block-14 custom-curve write.
/// CC40 anti-dust scheduler decoded from page 4 offsets 81..83.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AntiDustSchedule {
    pub enabled: bool,
    pub resume: bool,
    pub day: u8,
    pub hour: u8,
}

impl AntiDustSchedule {
    pub fn from_settings(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 3 {
            return None;
        }
        let day = bytes[1] & 0x7f;
        Some(Self {
            // Page 4/81 is the scheduler initialization byte. The vendor uses
            // day 127 as the "no scheduled day" sentinel.
            enabled: day != 127,
            resume: bytes[1] & 0x80 != 0,
            day,
            hour: bytes[2],
        })
    }

    pub fn to_settings(self) -> [u8; 5] {
        let day = if self.enabled { self.day } else { 127 };
        [1, day | (u8::from(self.resume) << 7), self.hour, 0, 0]
    }

    pub fn command_value(self) -> u32 {
        let day = if self.enabled { self.day } else { 127 };
        u32::from(day) + (u32::from(self.resume) << 7) + (u32::from(self.hour) << 8)
    }

    pub fn validate(self) -> Result<()> {
        if self.day > 127 {
            return Err(DchuError::InvalidArgument(
                "anti-dust day must be within 0..=127".to_owned(),
            ));
        }
        if self.hour > 23 {
            return Err(DchuError::InvalidArgument(
                "anti-dust hour must be within 0..=23".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanCurveState {
    pub fan_count: u8,
    pub initial_mode: u8,
    pub channels: [FanCurveChannel; 3],
}

/// One of the ten points in CC40's RPM-controlled fan table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RpmFanLevel {
    pub temp_down_c: u8,
    pub temp_up_c: u8,
    pub rpm_down: u16,
    pub rpm_up: u16,
    pub duty_up: u8,
    pub duty_down: u8,
    pub time_down: u8,
    pub time_up: u8,
}

/// A complete fan module from the RPM fan-table protocol. Reserved bytes are
/// retained so read/modify/write operations never discard vendor data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RpmFanChannel {
    pub present: bool,
    pub id: u8,
    pub open_fan_local_temp: u8,
    pub shutdown_local_temp: u8,
    pub open_fan_temp: u8,
    pub close_fan_temp: u8,
    pub temp_shutdown: u8,
    pub temp_passive: u8,
    pub temp_fan_full: u8,
    pub idle_rpm: u16,
    pub max_rpm: u16,
    pub max_turbo_rpm: u16,
    pub reserved: [u8; 8],
    pub levels: [RpmFanLevel; 10],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RpmFanTable {
    pub channels: [RpmFanChannel; 4],
    pub fan12_raw: [u8; 256],
    pub fan34_raw: [u8; 256],
}

impl Default for RpmFanTable {
    fn default() -> Self {
        Self {
            channels: [RpmFanChannel::default(); 4],
            fan12_raw: [0; 256],
            fan34_raw: [0; 256],
        }
    }
}

/// User-editable middle nodes for one fan. Duties are percentages, matching the
/// vendor UI and the block-14 FEVT frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanCurveNodes {
    pub t2: u8,
    pub d2: u8,
    pub t3: u8,
    pub d3: u8,
}

impl Default for FanCurveNodes {
    fn default() -> Self {
        Self {
            t2: 60,
            d2: 45,
            t3: 80,
            d3: 65,
        }
    }
}

impl FanCurveNodes {
    pub fn validate(self) -> Result<()> {
        if !(20..=98).contains(&self.t2) {
            return Err(DchuError::InvalidArgument(
                "fan curve T2 must be within 20..=98 °C".to_owned(),
            ));
        }
        if self.t3 <= self.t2 || self.t3 > 99 {
            return Err(DchuError::InvalidArgument(
                "fan curve T3 must be greater than T2 and at most 99 °C".to_owned(),
            ));
        }
        if !(20..=100).contains(&self.d2) {
            return Err(DchuError::InvalidArgument(
                "fan curve D2 must be within 20..=100%".to_owned(),
            ));
        }
        if self.d3 < self.d2 || self.d3 > 100 {
            return Err(DchuError::InvalidArgument(
                "fan curve D3 must be monotonic and at most 100%".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Middle nodes for the three hardware fan channels. An unused channel may be
/// represented by all-zero nodes and is omitted from validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomFanCurve {
    pub channels: [Option<FanCurveNodes>; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlatformState {
    pub power_mode: Option<u8>,
    pub fan_mode: Option<u8>,
    pub fan_offset: Option<u8>,
    pub turbo_fan: Option<bool>,
    pub slow_fan: Option<bool>,
    pub anti_dust_running: Option<bool>,
    pub power_policy: Option<u8>,
    pub capabilities: Option<i32>,
    pub platform_id: Option<i32>,
    pub low_refresh_rate_supported: Option<bool>,
    pub low_refresh_rate_enabled: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuSwitchState {
    pub current_mode: u8,
    pub supported_mask: u8,
}

impl GpuSwitchState {
    pub const fn current_label(self) -> &'static str {
        match self.current_mode {
            1 => "Integrated GPU",
            2 => "Discrete GPU",
            3 => "MSHybrid",
            4 => "Dynamic",
            _ => "Unknown",
        }
    }

    pub const fn supports(self, mode: GpuSwitchMode) -> bool {
        self.supported_mask & (1 << (mode.as_u8() - 1)) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GpuSwitchMode {
    Integrated = 1,
    Discrete = 2,
    MsHybrid = 3,
    Dynamic = 4,
}

impl GpuSwitchMode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Integrated),
            2 => Some(Self::Discrete),
            3 => Some(Self::MsHybrid),
            4 => Some(Self::Dynamic),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Integrated => "Integrated GPU",
            Self::Discrete => "Discrete GPU",
            Self::MsHybrid => "MSHybrid",
            Self::Dynamic => "Dynamic",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlexiChargerState {
    pub mode: u8,
    pub start_threshold: u8,
    pub stop_threshold: u8,
    pub threshold_choices: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FlexiChargerMode {
    Disabled = 0,
    Preset70To80 = 1,
    Custom = 2,
}

impl FlexiChargerMode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Disabled),
            1 => Some(Self::Preset70To80),
            2 => Some(Self::Custom),
            _ => None,
        }
    }

    pub const fn thresholds(self) -> Option<(u8, u8)> {
        match self {
            Self::Disabled => None,
            Self::Preset70To80 => Some((70, 80)),
            Self::Custom => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerMode {
    Quiet = 0,
    PowerSaving = 1,
    Performance = 2,
    Entertainment = 3,
    QuietPlus = 4,
    Turbo = 66,
}

impl PowerMode {
    pub const UI_MODES: [Self; 4] = [
        Self::PowerSaving,
        Self::Performance,
        Self::Entertainment,
        Self::QuietPlus,
    ];

    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Quiet => "Quiet",
            Self::PowerSaving => "Power saving",
            Self::Performance => "Performance",
            Self::Entertainment => "Entertainment",
            Self::QuietPlus => "Quiet+",
            Self::Turbo => "Turbo",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Quiet => "Lowest platform power and fan activity.",
            Self::PowerSaving => "Prioritizes battery life and lower heat.",
            Self::Performance => "Maximum sustained platform response.",
            Self::Entertainment => "Balances acoustics with sustained response.",
            Self::QuietPlus => "The quietest supported platform profile.",
            Self::Turbo => "Firmware composite turbo profile.",
        }
    }

    /// The second-stage firmware boost mode used after command 121/25.
    ///
    /// FnKey maps these UI profiles to DCHU command 79. Keeping the mapping in
    /// one typed place prevents the boot agent and the interactive UI from
    /// applying only half of the vendor power transition.
    pub const fn boost_mode(self) -> Option<u8> {
        match self {
            Self::Quiet | Self::Entertainment => Some(2),
            Self::PowerSaving | Self::QuietPlus => Some(1),
            Self::Performance => Some(0),
            Self::Turbo => None,
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Quiet),
            1 => Some(Self::PowerSaving),
            2 => Some(Self::Performance),
            3 => Some(Self::Entertainment),
            4 => Some(Self::QuietPlus),
            66 => Some(Self::Turbo),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FanMode {
    Auto = 0,
    Maximum = 1,
    Silent = 3,
    MaxQ = 5,
    Custom = 6,
    AntiDust = 7,
    Noiseless = 8,
    Iqst = 9,
}

impl FanMode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Maximum => "Maximum",
            Self::Silent => "Silent",
            Self::MaxQ => "MaxQ",
            Self::Custom => "Custom curve",
            Self::AntiDust => "Anti-dust",
            Self::Noiseless => "Noiseless",
            Self::Iqst => "IQST",
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Auto),
            1 => Some(Self::Maximum),
            3 => Some(Self::Silent),
            5 => Some(Self::MaxQ),
            6 => Some(Self::Custom),
            7 => Some(Self::AntiDust),
            8 => Some(Self::Noiseless),
            9 => Some(Self::Iqst),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickToggle {
    FnLock,
    WinLock,
    SwapWinFn,
    HotkeysControlCenter,
    Copilot,
}

impl QuickToggle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FnLock => "Fn lock",
            Self::WinLock => "Windows key lock",
            Self::SwapWinFn => "Swap Win / Fn",
            Self::HotkeysControlCenter => "Control Center hotkey",
            Self::Copilot => "Copilot key",
        }
    }

    pub const fn settings_offset(self) -> i32 {
        match self {
            Self::FnLock => 36,
            Self::WinLock => 37,
            Self::SwapWinFn => 38,
            Self::HotkeysControlCenter => 43,
            Self::Copilot => 44,
        }
    }

    const fn raw_is_inverted(self) -> bool {
        matches!(self, Self::WinLock)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuickToggleState {
    pub fn_lock: Option<bool>,
    pub win_lock: Option<bool>,
    pub swap_win_fn: Option<bool>,
    pub hotkeys_control_center: Option<bool>,
    pub copilot: Option<bool>,
}

impl QuickToggleState {
    pub const fn value(self, toggle: QuickToggle) -> Option<bool> {
        match toggle {
            QuickToggle::FnLock => self.fn_lock,
            QuickToggle::WinLock => self.win_lock,
            QuickToggle::SwapWinFn => self.swap_win_fn,
            QuickToggle::HotkeysControlCenter => self.hotkeys_control_center,
            QuickToggle::Copilot => self.copilot,
        }
    }

    pub fn set_value(&mut self, toggle: QuickToggle, enabled: bool) {
        match toggle {
            QuickToggle::FnLock => self.fn_lock = Some(enabled),
            QuickToggle::WinLock => self.win_lock = Some(enabled),
            QuickToggle::SwapWinFn => self.swap_win_fn = Some(enabled),
            QuickToggle::HotkeysControlCenter => self.hotkeys_control_center = Some(enabled),
            QuickToggle::Copilot => self.copilot = Some(enabled),
        }
    }

    /// Vendor ABI encoding: 1 enabled, 2 disabled. The Windows-key lock
    /// field is inverted by CC40's configuration IL.
    pub fn raw_values(self) -> Option<[u8; 5]> {
        Some([
            encode_toggle(QuickToggle::FnLock, self.fn_lock?),
            encode_toggle(QuickToggle::WinLock, self.win_lock?),
            encode_toggle(QuickToggle::SwapWinFn, self.swap_win_fn?),
            encode_toggle(
                QuickToggle::HotkeysControlCenter,
                self.hotkeys_control_center?,
            ),
            encode_toggle(QuickToggle::Copilot, self.copilot?),
        ])
    }

    /// CC40 treats vendor value 0 as an uninitialized preference and builds
    /// its first write from unchecked defaults. Match that behavior explicitly.
    pub fn raw_values_or_defaults(self) -> [u8; 5] {
        [
            encode_toggle(QuickToggle::FnLock, self.fn_lock.unwrap_or(false)),
            encode_toggle(QuickToggle::WinLock, self.win_lock.unwrap_or(false)),
            encode_toggle(QuickToggle::SwapWinFn, self.swap_win_fn.unwrap_or(false)),
            encode_toggle(
                QuickToggle::HotkeysControlCenter,
                self.hotkeys_control_center.unwrap_or(false),
            ),
            encode_toggle(QuickToggle::Copilot, self.copilot.unwrap_or(false)),
        ]
    }

    pub fn from_raw_values(values: [u8; 5]) -> Self {
        Self {
            fn_lock: decode_toggle(QuickToggle::FnLock, values[0]),
            win_lock: decode_toggle(QuickToggle::WinLock, values[1]),
            swap_win_fn: decode_toggle(QuickToggle::SwapWinFn, values[2]),
            hotkeys_control_center: decode_toggle(QuickToggle::HotkeysControlCenter, values[3]),
            copilot: decode_toggle(QuickToggle::Copilot, values[4]),
        }
    }
}

const fn encode_toggle(toggle: QuickToggle, enabled: bool) -> u8 {
    let value = if enabled { 1 } else { 2 };
    if toggle.raw_is_inverted() {
        match value {
            1 => 2,
            2 => 1,
            _ => unreachable!(),
        }
    } else {
        value
    }
}

const fn decode_toggle(toggle: QuickToggle, value: u8) -> Option<bool> {
    if toggle.raw_is_inverted() {
        match value {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    } else {
        match value {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }
}

/// Capability flags decoded from the vendor feature page and WMI support words.
/// These flags decide which CC4.0 controls are valid on the current platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub bios_feature_version: u16,
    pub power_saving: bool,
    pub performance: bool,
    pub flexikey: bool,
    pub uwp_power_mode: bool,
    pub airplane_osd_check: bool,
    pub entertainment: bool,
    pub quiet: bool,
    pub turbo_fan: bool,
    pub gpu_switch: bool,
    pub nvidia_power_off: bool,
    pub slow_fan: bool,
    pub simple_gpu_oc: bool,
    pub rpm_fan_control: bool,
    pub iqst_quiet: bool,
    pub iqst_power_saving: bool,
    pub iqst_performance: bool,
    pub iqst_entertainment: bool,
    pub flexi_charger_auto_enable: bool,
    pub fn_0_9_hotkey: bool,
    pub pd_power_level: bool,
    pub ccd_power_switch: bool,
    pub logo_a_cover_zone: bool,
    pub logo_a_cover: bool,
    pub lightbar_2025: bool,
    pub kb_15color_breath: bool,
    pub kb_15color_dynamic_breath: bool,
    pub pd_ap_controlled: bool,
    pub cpu_oc: bool,
    pub perkey_default_static_color: bool,
    pub dtt: bool,
    pub amd_manual_mode: bool,
    pub power_mode_ui_id: u8,
    pub ui_style_id: u8,
    pub oled_panel: bool,
    pub keyboard_type: u8,
    pub fan_count: u8,
    pub fan_exist_mask: u8,
    pub override_boot_effect: bool,
}

impl PlatformCapabilities {
    pub fn supports_power_mode(self, mode: PowerMode) -> bool {
        match mode {
            PowerMode::Quiet => self.quiet,
            PowerMode::PowerSaving => self.power_saving,
            PowerMode::Performance => self.performance,
            PowerMode::Entertainment => self.entertainment,
            PowerMode::QuietPlus => self.quiet,
            PowerMode::Turbo => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RgbKeyboardState {
    pub boot_effect: bool,
    pub effect: u8,
    pub brightness: u8,
    pub enabled: bool,
    pub color: RgbColor,
    pub sleep_enabled: bool,
    pub sleep_seconds: u32,
}

#[derive(Debug)]
pub enum DchuError {
    UnsupportedPlatform,
    Status {
        operation: &'static str,
        code: i32,
    },
    InvalidArgument(String),
    ReadbackMismatch {
        operation: &'static str,
        expected: u8,
        actual: Option<u8>,
        rollback: Option<String>,
    },
    ControlFailed {
        operation: &'static str,
        detail: String,
    },
    QuickToggleMismatch {
        expected: [u8; 5],
        actual: Option<[u8; 5]>,
        rollback: Option<String>,
    },
    ThreadUnavailable,
}

impl fmt::Display for DchuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                write!(f, "DCHU hardware access is only available on Windows")
            }
            Self::Status { operation, code } => {
                write!(f, "{operation} returned transport status {code}")
            }
            Self::InvalidArgument(message) => f.write_str(message),
            Self::ReadbackMismatch {
                operation,
                expected,
                actual,
                rollback,
            } => {
                write!(
                    f,
                    "{operation} did not read back: expected {expected}, got {}",
                    actual
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "no value".to_owned())
                )?;
                if let Some(rollback) = rollback {
                    write!(f, "; rollback: {rollback}")?;
                }
                Ok(())
            }
            Self::ControlFailed { operation, detail } => {
                write!(f, "{operation} failed: {detail}")
            }
            Self::QuickToggleMismatch {
                expected,
                actual,
                rollback,
            } => {
                write!(
                    f,
                    "quick-toggle readback mismatch: expected {expected:?}, got {actual:?}"
                )?;
                if let Some(rollback) = rollback {
                    write!(f, "; rollback: {rollback}")?;
                }
                Ok(())
            }
            Self::ThreadUnavailable => write!(f, "the hardware worker stopped unexpectedly"),
        }
    }
}

impl std::error::Error for DchuError {}

pub type Result<T> = std::result::Result<T, DchuError>;

pub fn decode_thermal(buffer: &[u8; 256]) -> ThermalSample {
    ThermalSample {
        cpu: decode_channel(buffer, 2, 16, 18),
        gpu: decode_channel(buffer, 4, 19, 21),
        fan3: decode_channel(buffer, 6, 22, 24),
        fan4: decode_channel(buffer, 36, 38, 40),
    }
}

fn decode_channel(
    buffer: &[u8; 256],
    rpm_offset: usize,
    duty_offset: usize,
    temp_offset: usize,
) -> FanChannel {
    let raw_rpm = u16::from_be_bytes([buffer[rpm_offset], buffer[rpm_offset + 1]]);
    FanChannel {
        rpm: rpm_from_raw(raw_rpm),
        duty_percent: duty_from_raw(buffer[duty_offset]),
        temperature_c: buffer[temp_offset],
    }
}

pub fn rpm_from_raw(raw: u16) -> u32 {
    if raw == 0 {
        return 0;
    }
    // Confirmed CC40 formula:
    // round(2 * 60 / (0.00005565217391304348 * raw_rpm))
    (120.0 / (0.00005565217391304348 * f64::from(raw))).round() as u32
}

pub fn duty_from_raw(raw: u8) -> u8 {
    (f32::from(raw) / 255.0 * 100.0).round() as u8
}

pub fn duty_raw_from_percent(percent: u8) -> u8 {
    (f32::from(percent.clamp(0, 100)) / 100.0 * 255.0).round() as u8
}

/// Decode the runtime fan table returned by DCHU command 13 (EEVT).
///
/// Layout confirmed against CC40 and the independent Clevo reference:
/// byte 12 = fan count, byte 14 = initial mode, and three 8-byte records at
/// offset 16 with T1,D1,T2,D2,T3,D3,T4,D4. Duties are native 0..=255.
pub fn decode_rpm_fan_channel(raw: &[u8; 112], present: bool) -> RpmFanChannel {
    let u16be = |index: usize| u16::from_be_bytes([raw[index], raw[index + 1]]);
    let mut channel = RpmFanChannel {
        present,
        id: raw[0],
        open_fan_local_temp: raw[1],
        shutdown_local_temp: raw[2],
        open_fan_temp: raw[3],
        close_fan_temp: raw[4],
        temp_shutdown: raw[5],
        temp_passive: raw[6],
        temp_fan_full: raw[7],
        idle_rpm: u16be(8),
        max_rpm: u16be(10),
        max_turbo_rpm: u16be(12),
        reserved: [0; 8],
        levels: [RpmFanLevel::default(); 10],
    };
    channel.reserved.copy_from_slice(&raw[14..22]);
    for (index, level) in channel.levels.iter_mut().enumerate() {
        *level = RpmFanLevel {
            temp_down_c: raw[22 + index],
            temp_up_c: raw[32 + index],
            rpm_down: u16be(42 + index * 2),
            rpm_up: u16be(62 + index * 2),
            duty_up: raw[82 + index],
            duty_down: raw[92 + index],
            time_down: raw[102 + index] >> 4,
            time_up: raw[102 + index] & 0x0f,
        };
    }
    channel
}

pub fn encode_rpm_fan_channel(channel: &RpmFanChannel) -> [u8; 112] {
    let mut raw = [0_u8; 112];
    raw[0] = channel.id;
    raw[1] = channel.open_fan_local_temp;
    raw[2] = channel.shutdown_local_temp;
    raw[3] = channel.open_fan_temp;
    raw[4] = channel.close_fan_temp;
    raw[5] = channel.temp_shutdown;
    raw[6] = channel.temp_passive;
    raw[7] = channel.temp_fan_full;
    raw[8..10].copy_from_slice(&channel.idle_rpm.to_be_bytes());
    raw[10..12].copy_from_slice(&channel.max_rpm.to_be_bytes());
    raw[12..14].copy_from_slice(&channel.max_turbo_rpm.to_be_bytes());
    raw[14..22].copy_from_slice(&channel.reserved);
    for (index, level) in channel.levels.iter().enumerate() {
        raw[22 + index] = level.temp_down_c;
        raw[32 + index] = level.temp_up_c;
        raw[42 + index * 2..44 + index * 2].copy_from_slice(&level.rpm_down.to_be_bytes());
        raw[62 + index * 2..64 + index * 2].copy_from_slice(&level.rpm_up.to_be_bytes());
        raw[82 + index] = level.duty_up;
        raw[92 + index] = level.duty_down;
        raw[102 + index] = (level.time_down << 4) | (level.time_up & 0x0f);
    }
    raw
}

pub fn validate_rpm_fan_channel(channel: &RpmFanChannel) -> Result<()> {
    if !channel.present {
        return Ok(());
    }
    for index in 1..channel.levels.len() {
        let previous = channel.levels[index - 1];
        let current = channel.levels[index];
        if current.temp_up_c < previous.temp_up_c {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} up-temperature point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
        if current.temp_down_c < previous.temp_down_c {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} down-temperature point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
        if current.rpm_up < previous.rpm_up {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} up-RPM point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
        if current.rpm_down < previous.rpm_down {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} down-RPM point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
    }
    for level in &channel.levels {
        if level.temp_up_c > 100 || level.temp_down_c > 100 {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} contains a temperature above 100 °C",
                channel.id
            )));
        }
        if channel.max_rpm != 0
            && (level.rpm_up > channel.max_rpm.saturating_add(channel.max_turbo_rpm)
                || level.rpm_down > channel.max_rpm.saturating_add(channel.max_turbo_rpm))
        {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} contains an RPM above the supported ceiling",
                channel.id
            )));
        }
    }
    Ok(())
}

pub fn rpm_fan_channel_matches(expected: &RpmFanChannel, actual: &RpmFanChannel) -> bool {
    if expected.present != actual.present {
        return false;
    }
    if !expected.present {
        return true;
    }
    expected.id == actual.id
        && expected.open_fan_local_temp == actual.open_fan_local_temp
        && expected.shutdown_local_temp == actual.shutdown_local_temp
        && expected.open_fan_temp == actual.open_fan_temp
        && expected.close_fan_temp == actual.close_fan_temp
        && expected.temp_shutdown == actual.temp_shutdown
        && expected.temp_passive == actual.temp_passive
        && expected.temp_fan_full == actual.temp_fan_full
        && expected.idle_rpm == actual.idle_rpm
        && expected.max_rpm == actual.max_rpm
        && expected.max_turbo_rpm == actual.max_turbo_rpm
        && expected.reserved == actual.reserved
        && expected.levels == actual.levels
}

pub fn rgb_subcommand_packet(subcommand: u8, value: u32) -> [u8; 4] {
    let bytes = value.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], subcommand]
}

/// CC40 packs RGB bytes differently for the 15-color keyboard (type 6) and
/// other RGB keyboard controllers. Keep this encoding in one tested helper.
pub fn rgb_part_value(keyboard_type: u8, color: RgbColor) -> u32 {
    if keyboard_type == 6 {
        let blue = if (color.red == 0 && color.green == 254 && color.blue == 129)
            || (color.red == 0 && color.green == 255 && color.blue == 127)
        {
            70
        } else {
            color.blue
        };
        (u32::from(blue / 2) << 16) | (u32::from(color.red / 2) << 8) | u32::from(color.green / 2)
    } else {
        let scaled_blue = ((40.0 / 51.0) * f32::from(color.blue)).round() as u8;
        (u32::from(scaled_blue) << 16) | (u32::from(color.red) << 8) | u32::from(color.green)
    }
}

/// Decode CC40's feature page (page 7), WMI support words and command 13 data.
pub fn decode_platform_capabilities(
    feature_page: &[u8; 256],
    wmi_16: i32,
    wmi_122: i32,
    wmi_96: i32,
    ui_style_id: u8,
    oled_panel: bool,
    fan_info: &[u8; 256],
) -> PlatformCapabilities {
    let flag = |value: i32, bit: u32| (value as u32 & (1_u32 << bit)) != 0;
    let version = u16::from_be_bytes([feature_page[0], feature_page[1]]);
    let mut caps = PlatformCapabilities {
        bios_feature_version: version,
        ui_style_id,
        oled_panel,
        keyboard_type: match fan_info[15] {
            3 | 19 | 35 | 51 | 243 => 3,
            value => value,
        },
        fan_count: fan_info[12],
        fan_exist_mask: fan_info[50],
        override_boot_effect: fan_info[43] & 1 != 0,
        ..PlatformCapabilities::default()
    };

    if flag(wmi_16, 0) {
        caps.uwp_power_mode = true;
        caps.quiet = true;
        caps.power_saving = true;
        caps.performance = true;
        caps.entertainment = true;
    }
    caps.flexikey = flag(wmi_16, 1);
    caps.cpu_oc = flag(wmi_122, 23);
    caps.airplane_osd_check = flag(wmi_122, 6);
    caps.perkey_default_static_color = flag(wmi_96, 8);
    caps.dtt = flag(wmi_96, 12);
    caps.amd_manual_mode = flag(wmi_96, 13);

    if version == 0x0100 {
        caps.turbo_fan = feature_page[16] & 0x10 != 0;
        if feature_page[17] & 0x0f != 0 {
            caps.quiet = feature_page[17] & 1 != 0;
            caps.power_saving = feature_page[17] & 2 != 0;
            caps.performance = feature_page[17] & 4 != 0;
            caps.entertainment = feature_page[17] & 8 != 0;
            if feature_page[17] & 0x80 != 0 {
                caps.quiet = false;
            }
        }
        caps.power_mode_ui_id = feature_page[17] >> 4;
        caps.gpu_switch = feature_page[18] & 1 != 0;
        caps.nvidia_power_off = feature_page[19] & 0x40 != 0;
        caps.slow_fan = feature_page[20] & 1 != 0;
        caps.simple_gpu_oc = feature_page[20] & 0x08 != 0;
        caps.rpm_fan_control = feature_page[20] & 0x10 != 0;
        caps.iqst_quiet = feature_page[20] & 0x40 != 0;
        caps.iqst_power_saving = feature_page[20] & 0x80 != 0;
        caps.iqst_performance = feature_page[21] & 1 != 0;
        caps.iqst_entertainment = feature_page[21] & 2 != 0;
        caps.flexi_charger_auto_enable = feature_page[21] & 4 != 0;
        caps.fn_0_9_hotkey = feature_page[21] & 0x20 != 0;
        caps.pd_power_level = feature_page[23] & 2 != 0;
        caps.ccd_power_switch = feature_page[23] & 8 != 0;
        caps.logo_a_cover_zone = feature_page[23] & 0x20 != 0;
        caps.logo_a_cover = feature_page[23] & 0x40 != 0;
        caps.lightbar_2025 = feature_page[23] & 0x80 != 0;
        caps.kb_15color_breath = feature_page[24] & 0x08 != 0;
        caps.kb_15color_dynamic_breath = feature_page[25] & 2 != 0;
        caps.pd_ap_controlled = feature_page[25] & 4 != 0;
    } else if feature_page[17] & 0x0f != 0 {
        caps.quiet = feature_page[17] & 1 != 0;
        caps.power_saving = feature_page[17] & 2 != 0;
        caps.performance = feature_page[17] & 4 != 0;
        caps.entertainment = feature_page[17] & 8 != 0;
        caps.power_mode_ui_id = feature_page[17] >> 4;
    }
    caps
}

pub fn decode_fan_curve(buffer: &[u8; 256]) -> FanCurveState {
    let mut channels = [FanCurveChannel::default(); 3];
    for (fan, channel) in channels.iter_mut().enumerate() {
        let base = 16 + fan * 8;
        for (point, slot) in channel.points.iter_mut().enumerate() {
            slot.temp_c = buffer[base + point * 2];
            slot.duty_raw = buffer[base + point * 2 + 1];
        }
    }
    FanCurveState {
        fan_count: buffer[12],
        initial_mode: buffer[14],
        channels,
    }
}

fn fan_curve_slope(low: FanCurvePoint, high: FanCurvePoint) -> u16 {
    if high.temp_c <= low.temp_c {
        return 0;
    }
    let low_duty = f32::from(low.duty_percent());
    let high_duty = f32::from(high.duty_percent());
    let delta_temp = f32::from(high.temp_c - low.temp_c);
    (((high_duty - low_duty) / delta_temp) * 2.55 * 16.0)
        .round()
        .clamp(0.0, u16::MAX as f32) as u16
}

/// Build the 256-byte block-14 FEVT frame from the EC's runtime table.
///
/// Only the middle nodes are present in the frame; the EC retains T1/D1 and
/// T4/D4. Slopes are stored high byte first, matching FAN.Write_WMI14.
pub fn encode_fan_curve_frame(curve: &FanCurveState) -> [u8; 256] {
    let mut frame = [0_u8; 256];
    for (fan, channel) in curve.channels.iter().enumerate() {
        if channel.is_unused() {
            continue;
        }
        let data = 2 + fan * 4;
        frame[data] = channel.points[1].temp_c;
        frame[data + 1] = channel.points[1].duty_raw;
        frame[data + 2] = channel.points[2].temp_c;
        frame[data + 3] = channel.points[2].duty_raw;

        let slopes = 0x0E + fan * 6;
        for (index, pair) in [(0, 1), (1, 2), (2, 3)].into_iter().enumerate() {
            let slope = fan_curve_slope(channel.points[pair.0], channel.points[pair.1]);
            frame[slopes + index * 2] = (slope >> 8) as u8;
            frame[slopes + index * 2 + 1] = slope as u8;
        }
    }
    frame
}

/// Apply user-edited middle nodes to the EC's current runtime table.
pub fn curve_with_custom_nodes(
    current: &FanCurveState,
    nodes: &CustomFanCurve,
) -> Result<FanCurveState> {
    let mut target = *current;
    for (fan, custom) in nodes.channels.iter().copied().enumerate() {
        let Some(custom) = custom else {
            if !target.channels[fan].is_unused() {
                return Err(DchuError::InvalidArgument(format!(
                    "fan channel {fan} is present but no custom nodes were supplied"
                )));
            }
            continue;
        };
        custom.validate()?;
        if target.channels[fan].is_unused() {
            return Err(DchuError::InvalidArgument(format!(
                "fan channel {fan} is not present on this platform"
            )));
        }
        let middle = [custom.t2, custom.t3];
        target.channels[fan].points[1] = FanCurvePoint {
            temp_c: middle[0],
            duty_raw: duty_raw_from_percent(custom.d2),
        };
        target.channels[fan].points[2] = FanCurvePoint {
            temp_c: middle[1],
            duty_raw: duty_raw_from_percent(custom.d3),
        };
    }
    Ok(target)
}

pub fn fan_curve_matches(expected: &FanCurveState, actual: &FanCurveState) -> bool {
    expected
        .channels
        .iter()
        .zip(actual.channels.iter())
        .all(|(want, got)| {
            if want.is_unused() {
                return got.is_unused();
            }
            (1..=2).all(|index| {
                want.points[index].temp_c == got.points[index].temp_c
                    && want.points[index]
                        .duty_raw
                        .abs_diff(got.points[index].duty_raw)
                        <= 1
            })
        })
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn readback_confirmed(expected: u8, actual: Option<u8>) -> bool {
    actual == Some(expected)
}

#[cfg(windows)]
mod windows_backend;

#[cfg(windows)]
pub use windows_backend::Dchu;

#[cfg(not(windows))]
#[derive(Debug)]
pub struct Dchu;

#[cfg(not(windows))]
impl Dchu {
    pub fn open() -> Result<Self> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn path(&self) -> &str {
        ""
    }

    pub fn raw_read_buffer(&self, _command: i32) -> (i32, [u8; 256]) {
        (-1, [0_u8; 256])
    }

    pub fn raw_read_integer(&self, _command: i32) -> (i32, i32) {
        (-1, 0)
    }

    pub fn raw_read_settings(&self, _page: i32, _offset: i32, length: usize) -> (i32, Vec<u8>) {
        (-1, vec![0_u8; length])
    }

    pub fn read_thermal(&self) -> Result<ThermalSample> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_platform_state(&self) -> PlatformState {
        PlatformState::default()
    }

    pub fn read_setting_byte(&self, _page: i32, _offset: i32) -> Option<u8> {
        None
    }

    pub fn write_setting_byte(&self, _page: i32, _offset: i32, _value: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_power_mode(&self, _mode: PowerMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_fan_mode(&self, _mode: FanMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_slow_fan(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_turbo_fan(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn run_anti_dust(&self) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_low_refresh_rate(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_anti_dust_schedule(&self) -> Result<AntiDustSchedule> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_anti_dust_schedule(&self, _schedule: AntiDustSchedule) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_fan_offset(&self, _percent: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_fan_curve(&self) -> Result<FanCurveState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_rpm_fan_table(&self) -> Result<RpmFanTable> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn write_rpm_fan_table(&self, _table: &RpmFanTable) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_custom_rpm_fan_table(&self, _table: &RpmFanTable) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_custom_fan_curve(&self, _curve: CustomFanCurve) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn write_fan_curve_table(&self, _curve: &FanCurveState) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_quick_toggles(&self) -> QuickToggleState {
        QuickToggleState::default()
    }

    pub fn read_gpu_switch(&self) -> Result<GpuSwitchState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_gpu_switch(&self, _mode: GpuSwitchMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn stage_gpu_switch(&self, _mode: GpuSwitchMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_flexi_charger(&self) -> Result<FlexiChargerState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_flexi_charger(
        &self,
        _mode: FlexiChargerMode,
        _start_threshold: u8,
        _stop_threshold: u8,
    ) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_capabilities(&self) -> Result<PlatformCapabilities> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_rgb_keyboard_state(&self) -> Result<RgbKeyboardState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_brightness(&self, _level: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_color(&self, _color: RgbColor) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_effect(&self, _effect: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_sleep_timer(&self, _enabled: bool, _seconds: u32) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_boot_effect(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn notify_airplane_mode(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_camera_enabled(&self) -> Option<bool> {
        None
    }

    pub fn set_camera_enabled(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_touchpad_enabled(&self) -> Option<bool> {
        None
    }

    pub fn set_touchpad_enabled(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn has_extended_api(&self) -> bool {
        false
    }

    pub fn set_quick_toggle(&self, _toggle: QuickToggle, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_macro_frame_has_vendor_collection_layout() {
        let frame = encode_filter_macro(
            &[FilterRecord::key_code(0x7A), FilterRecord::key_code(0)],
            &[
                FilterRecord::macro_down(0x7B, 10),
                FilterRecord::macro_up(0x7B, 10),
            ],
        );
        assert_eq!(frame.len(), 72);
        assert_eq!(&frame[0..4], &32_u32.to_le_bytes());
        assert_eq!(&frame[4..8], &32_u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([frame[8 + 12], frame[8 + 13]]), 0x7A);
        assert_eq!(u16::from_le_bytes([frame[8 + 14], frame[8 + 15]]), 0);
        assert_eq!(u16::from_le_bytes([frame[40 + 14], frame[40 + 15]]), 1);
        assert_eq!(u16::from_le_bytes([frame[56 + 14], frame[56 + 15]]), 2);

        let removal =
            encode_filter_macro_removal(&[FilterRecord::key_code(0x7A), FilterRecord::key_code(0)]);
        assert_eq!(removal.len(), 40);
        assert_eq!(&removal[0..4], &32_u32.to_le_bytes());
        assert_eq!(&removal[4..8], &0_u32.to_le_bytes());
    }

    #[test]
    fn anti_dust_schedule_round_trips_vendor_packing() {
        let schedule = AntiDustSchedule {
            enabled: true,
            resume: true,
            day: 5,
            hour: 3,
        };
        assert_eq!(schedule.command_value(), 5 + 128 + 3 * 256);
        assert_eq!(schedule.to_settings(), [1, 5 | 0x80, 3, 0, 0]);
        assert_eq!(
            AntiDustSchedule::from_settings(&schedule.to_settings()),
            Some(schedule)
        );
        assert!(
            AntiDustSchedule {
                enabled: true,
                resume: false,
                day: 0,
                hour: 24
            }
            .validate()
            .is_err()
        );
        let disabled = AntiDustSchedule {
            enabled: false,
            resume: true,
            day: 3,
            hour: 4,
        };
        assert_eq!(disabled.command_value(), 127 + 128 + 4 * 256);
        assert_eq!(disabled.to_settings()[1], 127 | 0x80);
    }

    #[test]
    fn power_profiles_map_to_the_vendor_boost_command() {
        assert_eq!(PowerMode::Performance.boost_mode(), Some(0));
        assert_eq!(PowerMode::PowerSaving.boost_mode(), Some(1));
        assert_eq!(PowerMode::QuietPlus.boost_mode(), Some(1));
        assert_eq!(PowerMode::Quiet.boost_mode(), Some(2));
        assert_eq!(PowerMode::Entertainment.boost_mode(), Some(2));
        assert_eq!(PowerMode::Turbo.boost_mode(), None);
    }

    #[test]
    fn rpm_formula_matches_live_samples() {
        assert_eq!(rpm_from_raw(0), 0);
        assert_eq!(rpm_from_raw(614), 3512);
        assert_eq!(rpm_from_raw(1023), 2108);
    }

    #[test]
    fn duty_rounding_is_bounded() {
        assert_eq!(duty_from_raw(0), 0);
        assert_eq!(duty_from_raw(111), 44);
        assert_eq!(duty_from_raw(255), 100);
    }

    #[test]
    fn fan_curve_layout_and_slopes_match_vendor_table() {
        let mut buffer = [0_u8; 256];
        buffer[12] = 2;
        buffer[14] = 0;
        let baseline = [
            [40_u8, 71_u8, 61_u8, 105_u8, 88_u8, 150_u8, 100_u8, 255_u8],
            [40, 71, 61, 105, 88, 150, 100, 255],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ];
        for (fan, values) in baseline.iter().enumerate() {
            let base = 16 + fan * 8;
            for (index, value) in values.iter().enumerate() {
                buffer[base + index] = *value;
            }
        }
        let curve = decode_fan_curve(&buffer);
        assert_eq!(curve.fan_count, 2);
        assert_eq!(curve.channels[0].points[1].duty_percent(), 41);
        assert!(curve.channels[2].is_unused());
        let frame = encode_fan_curve_frame(&curve);
        assert_eq!(frame[2], 61);
        assert_eq!(frame[3], 105);
        assert_eq!(frame[4], 88);
        assert_eq!(frame[5], 150);
        assert_eq!(&frame[14..20], &[0, 25, 0, 27, 0, 139]);
    }

    #[test]
    fn rpm_fan_channel_round_trips_all_ten_nodes() {
        let mut channel = RpmFanChannel {
            present: true,
            id: 0x5a,
            open_fan_local_temp: 40,
            shutdown_local_temp: 95,
            open_fan_temp: 45,
            close_fan_temp: 40,
            temp_shutdown: 96,
            temp_passive: 80,
            temp_fan_full: 90,
            idle_rpm: 0,
            max_rpm: 5200,
            max_turbo_rpm: 5600,
            reserved: [1, 2, 3, 4, 5, 6, 7, 8],
            levels: [RpmFanLevel::default(); 10],
        };
        for (index, level) in channel.levels.iter_mut().enumerate() {
            *level = RpmFanLevel {
                temp_down_c: 35 + index as u8,
                temp_up_c: 40 + index as u8,
                rpm_down: 1000 + index as u16 * 100,
                rpm_up: 1200 + index as u16 * 100,
                duty_up: 20 + index as u8,
                duty_down: 15 + index as u8,
                time_down: index as u8 % 4,
                time_up: (index as u8 + 1) % 4,
            };
        }
        let raw = encode_rpm_fan_channel(&channel);
        let decoded = decode_rpm_fan_channel(&raw, true);
        assert_eq!(decoded, channel);
        assert!(rpm_fan_channel_matches(&channel, &decoded));
    }

    #[test]
    fn custom_fan_nodes_preserve_fixed_points_and_validate_order() {
        let mut current = FanCurveState::default();
        current.channels[0] = FanCurveChannel {
            points: [
                FanCurvePoint {
                    temp_c: 40,
                    duty_raw: 71,
                },
                FanCurvePoint {
                    temp_c: 61,
                    duty_raw: 105,
                },
                FanCurvePoint {
                    temp_c: 88,
                    duty_raw: 150,
                },
                FanCurvePoint {
                    temp_c: 100,
                    duty_raw: 255,
                },
            ],
        };
        let nodes = CustomFanCurve {
            channels: [
                Some(FanCurveNodes {
                    t2: 55,
                    d2: 40,
                    t3: 85,
                    d3: 70,
                }),
                None,
                None,
            ],
        };
        let target = curve_with_custom_nodes(&current, &nodes).unwrap();
        assert_eq!(target.channels[0].points[0], current.channels[0].points[0]);
        assert_eq!(target.channels[0].points[3], current.channels[0].points[3]);
        assert_eq!(target.channels[0].points[1].temp_c, 55);
        assert_eq!(target.channels[0].points[2].temp_c, 85);
        assert!(nodes.channels[0].unwrap().validate().is_ok());
        assert!(
            FanCurveNodes {
                t2: 70,
                d2: 40,
                t3: 60,
                d3: 70
            }
            .validate()
            .is_err()
        );
        assert!(
            FanCurveNodes {
                t2: 60,
                d2: 80,
                t3: 80,
                d3: 70
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn gpu_switch_modes_and_mask_are_stable() {
        assert_eq!(GpuSwitchMode::Integrated.as_u8(), 1);
        assert_eq!(GpuSwitchMode::Discrete.as_u8(), 2);
        assert_eq!(GpuSwitchMode::MsHybrid.as_u8(), 3);
        assert_eq!(GpuSwitchMode::Dynamic.as_u8(), 4);
        let state = GpuSwitchState {
            current_mode: 3,
            supported_mask: 0b110,
        };
        assert!(state.supports(GpuSwitchMode::Discrete));
        assert!(state.supports(GpuSwitchMode::MsHybrid));
        assert!(!state.supports(GpuSwitchMode::Integrated));
    }

    #[test]
    fn flexi_charger_modes_have_stable_values() {
        assert_eq!(FlexiChargerMode::Disabled.as_u8(), 0);
        assert_eq!(FlexiChargerMode::Preset70To80.as_u8(), 1);
        assert_eq!(FlexiChargerMode::Custom.as_u8(), 2);
        assert_eq!(FlexiChargerMode::Preset70To80.thresholds(), Some((70, 80)));
    }

    #[test]
    fn quick_toggle_values_round_trip() {
        let state = QuickToggleState::from_raw_values([1, 2, 1, 2, 1]);
        assert_eq!(state.fn_lock, Some(true));
        assert_eq!(state.win_lock, Some(true));
        assert_eq!(state.raw_values(), Some([1, 2, 1, 2, 1]));
        assert_eq!(
            QuickToggleState::from_raw_values([0, 1, 2, 3, 4]).raw_values(),
            None
        );
    }

    #[test]
    fn readback_must_match_the_requested_target() {
        assert!(readback_confirmed(3, Some(3)));
        assert!(!readback_confirmed(3, Some(2)));
        assert!(!readback_confirmed(3, None));
    }

    #[test]
    fn decodes_live_feature_page_and_keyboard_capabilities() {
        let mut features = [0_u8; 256];
        let live = [
            0x01, 0x00, 0x25, 0x00, 0x00, 0x04, 0x55, 0x23, 0x01, 0x00, 0x1c, 0x02, 0x00, 0x93,
            0x08, 0x82, 0x10, 0x8d, 0x01, 0x40, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x01,
        ];
        features[..live.len()].copy_from_slice(&live);
        let mut fan_info = [0_u8; 256];
        fan_info[12] = 2;
        fan_info[15] = 6;
        fan_info[43] = 3;
        fan_info[50] = 3;
        let caps = decode_platform_capabilities(
            &features,
            0x0893,
            0x0400_0025,
            0x021c,
            4,
            false,
            &fan_info,
        );
        assert_eq!(caps.bios_feature_version, 0x0100);
        assert_eq!(caps.power_mode_ui_id, 8);
        assert!(caps.turbo_fan);
        assert!(!caps.power_saving);
        assert!(caps.performance);
        assert!(caps.entertainment);
        assert!(!caps.quiet);
        assert!(caps.gpu_switch);
        assert!(caps.nvidia_power_off);
        assert!(caps.slow_fan);
        assert!(caps.rpm_fan_control);
        assert!(!caps.simple_gpu_oc);
        assert!(caps.uwp_power_mode);
        assert!(caps.flexikey);
        assert_eq!(caps.keyboard_type, 6);
        assert_eq!(caps.fan_count, 2);
        assert_eq!(caps.fan_exist_mask, 3);
        assert!(caps.override_boot_effect);
    }

    #[test]
    fn rgb_keyboard_encoding_matches_vendor_type_6() {
        let color = RgbColor {
            red: 200,
            green: 100,
            blue: 50,
        };
        assert_eq!(rgb_part_value(6, color), 0x0019_6432);
        assert_eq!(
            rgb_subcommand_packet(240, 0x0019_6432),
            [0x32, 0x64, 0x19, 240]
        );
        let special = RgbColor {
            red: 0,
            green: 254,
            blue: 129,
        };
        assert_eq!(rgb_part_value(6, special), 0x0023_007f);
    }

    #[test]
    fn decodes_confirmed_telemetry_layout() {
        let mut buffer = [0_u8; 256];
        buffer[2..4].copy_from_slice(&614_u16.to_be_bytes());
        buffer[4..6].copy_from_slice(&1023_u16.to_be_bytes());
        buffer[16] = 111;
        buffer[18] = 68;
        buffer[19] = 65;
        buffer[21] = 50;
        let sample = decode_thermal(&buffer);
        assert_eq!(sample.cpu.rpm, 3512);
        assert_eq!(sample.cpu.temperature_c, 68);
        assert_eq!(sample.cpu.duty_percent, 44);
        assert_eq!(sample.gpu.rpm, 2108);
        assert_eq!(sample.gpu.temperature_c, 50);
        assert_eq!(sample.gpu.duty_percent, 25);
    }
}
