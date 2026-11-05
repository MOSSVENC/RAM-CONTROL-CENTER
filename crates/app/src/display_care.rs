use crate::system_controls::{self, DisplayCareState};

pub(crate) fn display_care_offset(toggle: u8) -> Option<i32> {
    match toggle {
        0 => Some(46),
        1 => Some(45),
        2 => Some(47),
        _ => None,
    }
}

pub(crate) fn restore_display_toggle(
    toggle: u8,
    previous: &DisplayCareState,
) -> Result<(), String> {
    match toggle {
        0 => system_controls::set_taskbar_auto_hide(previous.taskbar_auto_hide),
        1 => system_controls::set_taskbar_transparency(previous.taskbar_transparency),
        2 => system_controls::set_dark_mode(previous.dark_mode),
        _ => Err("unknown display-care toggle".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_care_offsets_match_vendor_linear_indices() {
        assert_eq!(display_care_offset(0), Some(46));
        assert_eq!(display_care_offset(1), Some(45));
        assert_eq!(display_care_offset(2), Some(47));
        assert_eq!(display_care_offset(3), None);
    }
}
