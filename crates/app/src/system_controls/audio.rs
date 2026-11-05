#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioState {
    pub volume_percent: u8,
    pub muted: bool,
}

#[cfg(windows)]
pub fn audio_state() -> Result<AudioState, String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| -> windows::core::Result<AudioState> {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            let volume = (endpoint.GetMasterVolumeLevelScalar()? * 100.0).round();
            let muted = endpoint.GetMute()?.as_bool();
            Ok(AudioState {
                volume_percent: volume.clamp(0.0, 100.0) as u8,
                muted,
            })
        })()
        .map_err(|error| error.to_string());
        if initialized {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
pub fn audio_state() -> Result<AudioState, String> {
    Err("audio control is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_audio_volume(percent: u8) -> Result<(), String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| -> windows::core::Result<()> {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            endpoint.SetMasterVolumeLevelScalar(
                f32::from(percent.min(100)) / 100.0,
                std::ptr::null(),
            )?;
            Ok(())
        })()
        .map_err(|error| error.to_string());
        if initialized {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
pub fn set_audio_volume(_percent: u8) -> Result<(), String> {
    Err("audio control is only available on Windows".to_owned())
}

#[cfg(windows)]
pub fn set_audio_mute(muted: bool) -> Result<(), String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| -> windows::core::Result<()> {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            endpoint.SetMute(muted, std::ptr::null())?;
            Ok(())
        })()
        .map_err(|error| error.to_string());
        if initialized {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
pub fn set_audio_mute(_muted: bool) -> Result<(), String> {
    Err("audio control is only available on Windows".to_owned())
}
