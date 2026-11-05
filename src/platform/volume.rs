use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

pub struct VolumeManager {
    endpoint: Option<IAudioEndpointVolume>,
}

impl VolumeManager {
    pub fn new() -> Self {
        let endpoint = Self::get_endpoint().ok();
        Self { endpoint }
    }

    fn get_endpoint() -> windows::core::Result<IAudioEndpointVolume> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            Ok(endpoint)
        }
    }

    pub fn read(&mut self) -> (f32, bool) {
        if self.endpoint.is_none() {
            self.endpoint = Self::get_endpoint().ok();
        }

        let mut needs_retry = false;
        if let Some(endpoint) = &self.endpoint {
            unsafe {
                match (endpoint.GetMasterVolumeLevelScalar(), endpoint.GetMute()) {
                    (Ok(level), Ok(mute)) => return (level * 100.0, mute.as_bool()),
                    _ => needs_retry = true,
                }
            }
        }

        // COM pointer was likely invalidated (e.g. sleep/device change). Fetch a fresh endpoint.
        if needs_retry {
            self.endpoint = Self::get_endpoint().ok();
            if let Some(endpoint) = &self.endpoint {
                unsafe {
                    if let (Ok(level), Ok(mute)) = (endpoint.GetMasterVolumeLevelScalar(), endpoint.GetMute()) {
                        return (level * 100.0, mute.as_bool());
                    }
                }
            }
        }
        (50.0, false)
    }

    pub fn set_volume(&mut self, percent: f32) {
        if self.endpoint.is_none() {
            self.endpoint = Self::get_endpoint().ok();
        }

        let mut needs_retry = false;
        if let Some(endpoint) = &self.endpoint {
            let scalar = (percent / 100.0).clamp(0.0, 1.0);
            unsafe {
                if endpoint.SetMasterVolumeLevelScalar(scalar, std::ptr::null()).is_err() {
                    needs_retry = true;
                } else if percent > 0.0 {
                    if endpoint.SetMute(false, std::ptr::null()).is_err() {
                        needs_retry = true;
                    }
                }
            }
        }

        if needs_retry {
            self.endpoint = Self::get_endpoint().ok();
            if let Some(endpoint) = &self.endpoint {
                let scalar = (percent / 100.0).clamp(0.0, 1.0);
                unsafe {
                    let _ = endpoint.SetMasterVolumeLevelScalar(scalar, std::ptr::null());
                    if percent > 0.0 {
                        let _ = endpoint.SetMute(false, std::ptr::null());
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn set_mute(&mut self, mute: bool) {
        if self.endpoint.is_none() {
            self.endpoint = Self::get_endpoint().ok();
        }

        let mut needs_retry = false;
        if let Some(endpoint) = &self.endpoint {
            unsafe {
                if endpoint.SetMute(mute, std::ptr::null()).is_err() {
                    needs_retry = true;
                }
            }
        }

        if needs_retry {
            self.endpoint = Self::get_endpoint().ok();
            if let Some(endpoint) = &self.endpoint {
                unsafe {
                    let _ = endpoint.SetMute(mute, std::ptr::null());
                }
            }
        }
    }
}
