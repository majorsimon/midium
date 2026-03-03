use windows::core::Interface;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

use tracing::{debug, warn};

use midium_core::dispatch::VolumeControl;
use midium_core::types::{AudioCapabilities, AudioDeviceInfo, AudioSessionInfo, AudioTarget};

use crate::backend::AudioBackend;

// PKEY_Device_FriendlyName: {a45c254e-df1c-4efd-8020-67d146a850e0}, pid 14
const PKEY_DEVICE_FRIENDLY_NAME: windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY =
    windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(
            0xa45c254e_df1c_4efd_8020_67d146a850e0,
        ),
        pid: 14,
    };

const STGM_READ: u32 = 0;

pub struct WasapiBackend;

impl WasapiBackend {
    pub fn new() -> anyhow::Result<Self> {
        // Initialize COM for this thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        debug!("WASAPI backend initialized");
        Ok(Self)
    }

    fn enumerator() -> anyhow::Result<IMMDeviceEnumerator> {
        unsafe {
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| anyhow::anyhow!("CoCreateInstance IMMDeviceEnumerator: {e}"))
        }
    }

    fn device_name(device: &windows::Win32::Media::Audio::IMMDevice) -> String {
        use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
        unsafe {
            let store = device.OpenPropertyStore(
                windows::Win32::System::Com::StructuredStorage::STGM(STGM_READ),
            );
            if let Ok(store) = store {
                if let Ok(prop) = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME) {
                    if let Ok(s) = PropVariantToStringAlloc(&prop) {
                        return s.to_string().unwrap_or_default();
                    }
                }
            }
            "(unknown)".to_string()
        }
    }

    fn endpoint_volume(device: &windows::Win32::Media::Audio::IMMDevice)
        -> anyhow::Result<IAudioEndpointVolume>
    {
        unsafe {
            device
                .Activate::<IAudioEndpointVolume>(
                    CLSCTX_ALL,
                    None,
                )
                .map_err(|e| anyhow::anyhow!("Activate IAudioEndpointVolume: {e}"))
        }
    }
}

impl VolumeControl for WasapiBackend {
    fn set_volume(&self, target: &AudioTarget, volume: f64) -> anyhow::Result<()> {
        let enumerator = Self::enumerator()?;
        match target {
            AudioTarget::SystemMaster | AudioTarget::Device { .. } => {
                let device = match target {
                    AudioTarget::SystemMaster => unsafe {
                        enumerator
                            .GetDefaultAudioEndpoint(eRender, eConsole)
                            .map_err(|e| anyhow::anyhow!("GetDefaultAudioEndpoint: {e}"))?
                    },
                    AudioTarget::Device { id } => unsafe {
                        let id_wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
                        enumerator
                            .GetDevice(windows::core::PCWSTR(id_wide.as_ptr()))
                            .map_err(|e| anyhow::anyhow!("GetDevice: {e}"))?
                    },
                    _ => unreachable!(),
                };
                let ep_vol = Self::endpoint_volume(&device)?;
                unsafe {
                    ep_vol
                        .SetMasterVolumeLevelScalar(volume as f32, std::ptr::null())
                        .map_err(|e| anyhow::anyhow!("SetMasterVolumeLevelScalar: {e}"))?;
                }
                Ok(())
            }
            AudioTarget::Application { name } => {
                set_session_volume_by_name(&enumerator, name, volume)
            }
            AudioTarget::FocusedApplication => {
                warn!("FocusedApplication volume not yet implemented on Windows");
                Ok(())
            }
        }
    }

    fn set_mute(&self, target: &AudioTarget, muted: bool) -> anyhow::Result<()> {
        let enumerator = Self::enumerator()?;
        let device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| anyhow::anyhow!("GetDefaultAudioEndpoint: {e}"))?
        };
        let ep_vol = Self::endpoint_volume(&device)?;
        unsafe {
            ep_vol
                .SetMute(muted, std::ptr::null())
                .map_err(|e| anyhow::anyhow!("SetMute: {e}"))?;
        }
        Ok(())
    }

    fn is_muted(&self, _target: &AudioTarget) -> anyhow::Result<bool> {
        let enumerator = Self::enumerator()?;
        let device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| anyhow::anyhow!("GetDefaultAudioEndpoint: {e}"))?
        };
        let ep_vol = Self::endpoint_volume(&device)?;
        let muted: BOOL = unsafe {
            ep_vol
                .GetMute()
                .map_err(|e| anyhow::anyhow!("GetMute: {e}"))?
        };
        Ok(muted.as_bool())
    }

    fn set_default_output(&self, device_id: &str) -> anyhow::Result<()> {
        warn!(device_id, "Default output switching not yet implemented on Windows (IPolicyConfig required)");
        Ok(())
    }

    fn set_default_input(&self, device_id: &str) -> anyhow::Result<()> {
        warn!(device_id, "Default input switching not yet implemented on Windows");
        Ok(())
    }
}

impl AudioBackend for WasapiBackend {
    fn list_output_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>> {
        let enumerator = Self::enumerator()?;
        let default_device = unsafe {
            enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()
        };
        let default_id = default_device.as_ref().and_then(|d| {
            unsafe { d.GetId().ok().and_then(|p| p.to_string().ok()) }
        });

        let collection = unsafe {
            enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|e| anyhow::anyhow!("EnumAudioEndpoints: {e}"))?
        };
        let count = unsafe { collection.GetCount().map_err(|e| anyhow::anyhow!("{e}"))? };

        let mut devices = Vec::new();
        for i in 0..count {
            let device = unsafe {
                collection.Item(i).map_err(|e| anyhow::anyhow!("{e}"))?
            };
            let id = unsafe {
                device.GetId().ok().and_then(|p| p.to_string().ok()).unwrap_or_default()
            };
            devices.push(AudioDeviceInfo {
                name: Self::device_name(&device),
                is_default: Some(&id) == default_id.as_ref(),
                id,
                is_input: false,
            });
        }
        Ok(devices)
    }

    fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>> {
        let enumerator = Self::enumerator()?;
        let collection = unsafe {
            enumerator
                .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
                .map_err(|e| anyhow::anyhow!("EnumAudioEndpoints: {e}"))?
        };
        let count = unsafe { collection.GetCount().map_err(|e| anyhow::anyhow!("{e}"))? };

        let mut devices = Vec::new();
        for i in 0..count {
            let device = unsafe { collection.Item(i).map_err(|e| anyhow::anyhow!("{e}"))? };
            let id = unsafe {
                device.GetId().ok().and_then(|p| p.to_string().ok()).unwrap_or_default()
            };
            devices.push(AudioDeviceInfo {
                name: Self::device_name(&device),
                is_default: false,
                id,
                is_input: true,
            });
        }
        Ok(devices)
    }

    fn get_volume(&self, _target: &AudioTarget) -> anyhow::Result<f64> {
        let enumerator = Self::enumerator()?;
        let device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| anyhow::anyhow!("GetDefaultAudioEndpoint: {e}"))?
        };
        let ep_vol = Self::endpoint_volume(&device)?;
        let vol = unsafe {
            ep_vol
                .GetMasterVolumeLevelScalar()
                .map_err(|e| anyhow::anyhow!("GetMasterVolumeLevelScalar: {e}"))?
        };
        Ok(vol as f64)
    }

    fn list_sessions(&self) -> anyhow::Result<Vec<AudioSessionInfo>> {
        let enumerator = Self::enumerator()?;
        let device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| anyhow::anyhow!("GetDefaultAudioEndpoint: {e}"))?
        };

        let session_manager = unsafe {
            device
                .Activate::<windows::Win32::Media::Audio::IAudioSessionManager2>(CLSCTX_ALL, None)
                .map_err(|e| anyhow::anyhow!("Activate IAudioSessionManager2: {e}"))?
        };

        let session_enum = unsafe {
            session_manager
                .GetSessionEnumerator()
                .map_err(|e| anyhow::anyhow!("GetSessionEnumerator: {e}"))?
        };

        let count = unsafe {
            session_enum
                .GetCount()
                .map_err(|e| anyhow::anyhow!("GetCount: {e}"))?
        };

        let mut sessions = Vec::new();
        for i in 0..count {
            let session_ctrl = unsafe {
                session_enum
                    .GetSession(i)
                    .map_err(|e| anyhow::anyhow!("GetSession: {e}"))?
            };
            let session_ctrl2: windows::Win32::Media::Audio::IAudioSessionControl2 =
                session_ctrl.cast().map_err(|e| anyhow::anyhow!("cast IAudioSessionControl2: {e}"))?;

            let display_name = unsafe {
                session_ctrl
                    .GetDisplayName()
                    .ok()
                    .and_then(|p| p.to_string().ok())
                    .unwrap_or_else(|| "(unnamed)".to_string())
            };

            let pid = unsafe { session_ctrl2.GetProcessId().ok() };

            let simple_vol: windows::Win32::Media::Audio::ISimpleAudioVolume =
                session_ctrl.cast().map_err(|e| anyhow::anyhow!("cast ISimpleAudioVolume: {e}"))?;
            let volume = unsafe {
                simple_vol.GetMasterVolume().unwrap_or(0.0) as f64
            };
            let muted: BOOL = unsafe {
                simple_vol.GetMute().unwrap_or(BOOL(0))
            };

            sessions.push(AudioSessionInfo {
                name: display_name,
                pid,
                volume,
                muted: muted.as_bool(),
            });
        }
        Ok(sessions)
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            per_app_volume: true,
            device_switching: false,
            input_device_switching: false,
        }
    }
}

/// Set the volume of the audio session whose display name contains `name`.
fn set_session_volume_by_name(
    enumerator: &IMMDeviceEnumerator,
    name: &str,
    volume: f64,
) -> anyhow::Result<()> {
    let device = unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| anyhow::anyhow!("GetDefaultAudioEndpoint: {e}"))?
    };
    let session_manager = unsafe {
        device
            .Activate::<windows::Win32::Media::Audio::IAudioSessionManager2>(CLSCTX_ALL, None)
            .map_err(|e| anyhow::anyhow!("Activate IAudioSessionManager2: {e}"))?
    };
    let session_enum = unsafe {
        session_manager
            .GetSessionEnumerator()
            .map_err(|e| anyhow::anyhow!("GetSessionEnumerator: {e}"))?
    };
    let count = unsafe {
        session_enum
            .GetCount()
            .map_err(|e| anyhow::anyhow!("GetCount: {e}"))?
    };

    let name_lower = name.to_lowercase();
    for i in 0..count {
        let session_ctrl = unsafe {
            session_enum
                .GetSession(i)
                .map_err(|e| anyhow::anyhow!("GetSession: {e}"))?
        };
        let display_name = unsafe {
            session_ctrl
                .GetDisplayName()
                .ok()
                .and_then(|p| p.to_string().ok())
                .unwrap_or_default()
        };
        if display_name.to_lowercase().contains(&name_lower) {
            let simple_vol: windows::Win32::Media::Audio::ISimpleAudioVolume =
                session_ctrl.cast().map_err(|e| anyhow::anyhow!("cast ISimpleAudioVolume: {e}"))?;
            unsafe {
                simple_vol
                    .SetMasterVolume(volume as f32, std::ptr::null())
                    .map_err(|e| anyhow::anyhow!("SetMasterVolume: {e}"))?;
            }
            return Ok(());
        }
    }
    anyhow::bail!("No audio session matching '{name}' found")
}
