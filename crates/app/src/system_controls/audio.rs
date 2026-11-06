#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioState {
    pub volume_percent: u8,
    pub muted: bool,
}

#[cfg(windows)]
enum AudioAction {
    Read,
    SetVolume(f32),
    SetMute(bool),
}

#[cfg(windows)]
fn with_default_endpoint(action: AudioAction) -> Result<AudioState, String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    // SAFETY: COM is initialized on this thread before creating the
    // multithreaded apartment objects. All COM interfaces are dropped before
    // the matching CoUninitialize call below and never escape this closure.
    let (initialized, result) = unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| -> windows::core::Result<AudioState> {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            match action {
                AudioAction::Read => {
                    let volume = (endpoint.GetMasterVolumeLevelScalar()? * 100.0).round();
                    let muted = endpoint.GetMute()?.as_bool();
                    Ok(AudioState {
                        volume_percent: volume.clamp(0.0, 100.0) as u8,
                        muted,
                    })
                }
                AudioAction::SetVolume(level) => {
                    endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
                    Ok(AudioState::default())
                }
                AudioAction::SetMute(muted) => {
                    endpoint.SetMute(muted, std::ptr::null())?;
                    Ok(AudioState::default())
                }
            }
        })();
        (initialized, result)
    };
    let result = result.map_err(|error| error.to_string());
    if initialized {
        // SAFETY: this thread initialized COM successfully in the block above;
        // every created COM interface has been dropped by this point.
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(windows)]
pub fn audio_state() -> Result<AudioState, String> {
    with_default_endpoint(AudioAction::Read)
}

#[cfg(not(windows))]
pub fn audio_state() -> Result<AudioState, String> {
    Err("audio control is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_audio_volume(percent: u8) -> Result<(), String> {
    with_default_endpoint(AudioAction::SetVolume(f32::from(percent.min(100)) / 100.0)).map(|_| ())
}

#[cfg(not(windows))]
pub fn set_audio_volume(_percent: u8) -> Result<(), String> {
    Err("audio control is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_audio_mute(muted: bool) -> Result<(), String> {
    with_default_endpoint(AudioAction::SetMute(muted)).map(|_| ())
}

#[cfg(not(windows))]
pub fn set_audio_mute(_muted: bool) -> Result<(), String> {
    Err("audio control is only available on Windows".to_owned())
}
