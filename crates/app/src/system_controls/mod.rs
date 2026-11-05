mod audio;
mod display;
mod display_care;
#[cfg(windows)]
mod power;
mod radio;

#[cfg(windows)]
pub(crate) mod registry;

pub use audio::{AudioState, audio_state, set_audio_mute, set_audio_volume};
pub use display::{screen_brightness, set_blue_light_filter, set_screen_brightness};
pub use display_care::{
    DisplayCareState, display_care_state, set_dark_mode, set_monitor_timeouts, set_screen_saver,
    set_taskbar_auto_hide, set_taskbar_transparency,
};
pub use radio::{airplane_mode_enabled, set_airplane_mode};
