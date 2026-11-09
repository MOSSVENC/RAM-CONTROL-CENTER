use std::sync::atomic::{AtomicBool, Ordering};

use neo_control_protocol::{FanMode, PowerMode, QuickToggle};

include!(concat!(env!("OUT_DIR"), "/runtime_translations.rs"));

static LANGUAGE_ZH: AtomicBool = AtomicBool::new(true);

pub(crate) fn is_zh() -> bool {
    LANGUAGE_ZH.load(Ordering::Relaxed)
}

pub(crate) fn tr(message_id: &str) -> &str {
    if !is_zh() {
        return message_id;
    }
    ZH_CATALOG
        .binary_search_by(|(key, _)| key.cmp(&message_id))
        .map(|index| ZH_CATALOG[index].1)
        .unwrap_or(message_id)
}

pub(crate) fn localized_power_label(mode: PowerMode) -> &'static str {
    tr(mode.label())
}

pub(crate) fn localized_fan_label(mode: FanMode) -> &'static str {
    tr(mode.label())
}

pub(crate) fn localized_toggle_label(toggle: QuickToggle) -> &'static str {
    tr(toggle.label())
}

pub(crate) fn localized_gpu_label(mode: u8) -> &'static str {
    let message_id = match mode {
        1 => "Integrated GPU",
        2 => "Discrete GPU",
        3 => "MSHybrid",
        4 => "Dynamic",
        _ => "Unknown",
    };
    tr(message_id)
}

pub(crate) fn set_zh(zh: bool) {
    LANGUAGE_ZH.store(zh, Ordering::Relaxed);
}
