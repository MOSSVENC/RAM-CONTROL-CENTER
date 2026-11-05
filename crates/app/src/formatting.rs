use crate::localization::{is_zh, tr};

pub(crate) fn format_threshold_ranges(values: &[u8]) -> String {
    if values.is_empty() {
        return "--".to_owned();
    }
    let split = values.len().div_ceil(2);
    let (start, stop) = values.split_at(split);
    let range = |items: &[u8]| {
        let min = items.iter().min().copied().unwrap_or(0);
        let max = items.iter().max().copied().unwrap_or(0);
        format!("{min}–{max}%")
    };
    format!(
        "{} {} · {} {}",
        range(start),
        tr("start", "起始"),
        range(stop),
        tr("stop", "停止")
    )
}

pub(crate) fn toggle_value_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "Enabled",
        Some(false) => "Disabled",
        None => "Not initialized",
    }
}

pub(crate) fn format_battery_time(seconds: Option<u32>) -> String {
    let Some(seconds) = seconds else {
        return "--".to_owned();
    };
    let minutes = seconds / 60;
    if minutes >= 60 {
        if is_zh() {
            format!("{} 小时 {:02} 分钟", minutes / 60, minutes % 60)
        } else {
            format!("{} h {:02} min", minutes / 60, minutes % 60)
        }
    } else if is_zh() {
        format!("{minutes} 分钟")
    } else {
        format!("{minutes} min")
    }
}

pub(crate) fn format_battery_capacity(remaining_mwh: u32, max_mwh: u32) -> String {
    if max_mwh == 0 {
        return "--".to_owned();
    }
    format!(
        "{:.1} / {:.1} Wh",
        f64::from(remaining_mwh) / 1000.0,
        f64::from(max_mwh) / 1000.0
    )
}

pub(crate) fn format_power_rate(milliwatts: u32) -> String {
    if milliwatts == 0 {
        "--".to_owned()
    } else {
        format!("{:.1} W", f64::from(milliwatts) / 1000.0)
    }
}

pub(crate) fn format_clock(mhz: u32) -> String {
    if mhz == 0 {
        return "--".to_owned();
    }
    if mhz >= 1_000 {
        format!("{:.2} GHz", f64::from(mhz) / 1_000.0)
    } else {
        format!("{mhz} MHz")
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "--".to_owned();
    }
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let gib = bytes as f64 / GIB;
    if gib >= 100.0 {
        format!("{gib:.0} GB")
    } else if gib >= 10.0 {
        format!("{gib:.1} GB")
    } else {
        format!("{gib:.2} GB")
    }
}

pub(crate) fn shorten_path(path: &str) -> String {
    path.rsplit_once('\\')
        .map(|(_, file)| format!("Signed codec · {file}"))
        .unwrap_or_else(|| "Signed codec located".to_owned())
}
