use std::sync::atomic::{AtomicBool, Ordering};

use neo_control_protocol::{FanMode, PowerMode, QuickToggle};

static LANGUAGE_ZH: AtomicBool = AtomicBool::new(true);

pub(crate) fn is_zh() -> bool {
    LANGUAGE_ZH.load(Ordering::Relaxed)
}

pub(crate) fn tr<'a>(english: &'a str, chinese: &'a str) -> &'a str {
    if is_zh() { chinese } else { english }
}

pub(crate) fn localized_power_label(mode: PowerMode) -> &'static str {
    if !is_zh() {
        return mode.label();
    }
    match mode {
        PowerMode::Quiet => "静音",
        PowerMode::PowerSaving => "节能",
        PowerMode::Performance => "性能",
        PowerMode::Entertainment => "娱乐",
        PowerMode::QuietPlus => "静音+",
        PowerMode::Turbo => "涡轮",
    }
}

pub(crate) fn localized_fan_label(mode: FanMode) -> &'static str {
    if !is_zh() {
        return mode.label();
    }
    match mode {
        FanMode::Auto => "自动",
        FanMode::Maximum => "最大",
        FanMode::Silent => "静音",
        FanMode::MaxQ => "MaxQ",
        FanMode::Custom => "自定义曲线",
        FanMode::AntiDust => "防尘",
        FanMode::Noiseless => "静音模式",
        FanMode::Iqst => "IQST",
    }
}

pub(crate) fn localized_toggle_label(toggle: QuickToggle) -> &'static str {
    if !is_zh() {
        return toggle.label();
    }
    match toggle {
        QuickToggle::FnLock => "Fn 键锁定",
        QuickToggle::WinLock => "Windows 键锁定",
        QuickToggle::SwapWinFn => "交换 Win / Fn",
        QuickToggle::HotkeysControlCenter => "控制中心热键",
        QuickToggle::Copilot => "Copilot 键",
    }
}

pub(crate) fn localized_gpu_label(mode: u8) -> &'static str {
    if !is_zh() {
        return match mode {
            1 => "Integrated GPU",
            2 => "Discrete GPU",
            3 => "MSHybrid",
            4 => "Dynamic",
            _ => "Unknown",
        };
    }
    match mode {
        1 => "集成显卡",
        2 => "独立显卡",
        3 => "MSHybrid",
        4 => "动态",
        _ => "未知",
    }
}

pub(crate) fn set_zh(zh: bool) {
    LANGUAGE_ZH.store(zh, Ordering::Relaxed);
}
