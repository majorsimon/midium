use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::BOOL;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE, ERole,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED, STGM_READ,
};

// ---------------------------------------------------------------------------
// IPolicyConfig — undocumented COM interface for setting the default audio
// endpoint on Windows Vista through 11.
//
// CLSID: {870AF99C-171D-4F9E-AF0D-E63DF40C2BC9}  (CPolicyConfigClient)
// IID:   {F8679F50-850A-41CF-9C72-430F290290C8}
// ---------------------------------------------------------------------------

#[windows::core::interface("F8679F50-850A-41CF-9C72-430F290290C8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    // Stub methods that precede SetDefaultEndpoint in the vtable.
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut u8) -> windows::core::HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut u8) -> windows::core::HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> windows::core::HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, endpoint_fmt: *mut u8, mix_fmt: *mut u8) -> windows::core::HRESULT;
    fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, default_period: *mut i64, min_period: *mut i64) -> windows::core::HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> windows::core::HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut u32) -> windows::core::HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: u32) -> windows::core::HRESULT;
    fn GetPropertyValue(&self, device: PCWSTR, fx_store: i32, key: *const u32, value: *mut u8) -> windows::core::HRESULT;
    fn SetPropertyValue(&self, device: PCWSTR, fx_store: i32, key: *const u32, value: *mut u8) -> windows::core::HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> windows::core::HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> windows::core::HRESULT;
}

const CLSID_POLICY_CONFIG_CLIENT: windows::core::GUID =
    windows::core::GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

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
                STGM_READ,
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
                        .SetMute(muted, std::ptr::null())
                        .map_err(|e| anyhow::anyhow!("SetMute: {e}"))?;
                }
                Ok(())
            }
            AudioTarget::Application { name } => {
                set_session_mute_by_name(&enumerator, name, muted)
            }
            AudioTarget::FocusedApplication => {
                warn!("FocusedApplication mute not yet implemented on Windows");
                Ok(())
            }
        }
    }

    fn is_muted(&self, target: &AudioTarget) -> anyhow::Result<bool> {
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
                let muted: BOOL = unsafe {
                    ep_vol
                        .GetMute()
                        .map_err(|e| anyhow::anyhow!("GetMute: {e}"))?
                };
                Ok(muted.as_bool())
            }
            AudioTarget::Application { name } => {
                get_session_muted_by_name(&enumerator, name)
            }
            AudioTarget::FocusedApplication => {
                warn!("FocusedApplication mute query not yet implemented on Windows");
                Ok(false)
            }
        }
    }

    fn set_default_output(&self, device_id: &str) -> anyhow::Result<()> {
        set_default_endpoint(device_id)
    }

    /// Set the default input device. Uses the same undocumented IPolicyConfig
    /// `SetDefaultEndpoint` API as output — the interface works for both render
    /// and capture endpoints (the endpoint ID itself encodes direction).
    fn set_default_input(&self, device_id: &str) -> anyhow::Result<()> {
        set_default_endpoint(device_id)
    }

    fn is_default_output(&self, device_id: &str) -> anyhow::Result<bool> {
        let enumerator = Self::enumerator()?;
        let default_device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| anyhow::anyhow!("GetDefaultAudioEndpoint: {e}"))?
        };
        let default_id = unsafe {
            default_device.GetId().ok().and_then(|p| p.to_string().ok()).unwrap_or_default()
        };
        Ok(default_id == device_id)
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

    fn get_volume(&self, target: &AudioTarget) -> anyhow::Result<f64> {
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
                let vol = unsafe {
                    ep_vol
                        .GetMasterVolumeLevelScalar()
                        .map_err(|e| anyhow::anyhow!("GetMasterVolumeLevelScalar: {e}"))?
                };
                Ok(vol as f64)
            }
            AudioTarget::Application { name } => {
                get_session_volume_by_name(&enumerator, name)
            }
            AudioTarget::FocusedApplication => {
                warn!("FocusedApplication volume query not yet implemented on Windows");
                Ok(0.0)
            }
        }
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
            device_switching: true,
            input_device_switching: true,
        }
    }
}

/// Set the given endpoint as the default for all three roles (Console,
/// Multimedia, Communications) using the undocumented IPolicyConfig interface.
fn set_default_endpoint(device_id: &str) -> anyhow::Result<()> {
    let id_wide: Vec<u16> = device_id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(
            &CLSID_POLICY_CONFIG_CLIENT,
            None,
            CLSCTX_ALL,
        )
        .map_err(|e| anyhow::anyhow!("CoCreateInstance IPolicyConfig: {e}"))?;

        for role in [eConsole, eMultimedia, eCommunications] {
            policy
                .SetDefaultEndpoint(PCWSTR(id_wide.as_ptr()), role)
                .ok()
                .map_err(|e| anyhow::anyhow!("SetDefaultEndpoint: {e}"))?;
        }
    }
    Ok(())
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

/// Set the mute state of the audio session whose display name contains `name`.
fn set_session_mute_by_name(
    enumerator: &IMMDeviceEnumerator,
    name: &str,
    muted: bool,
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
                    .SetMute(muted, std::ptr::null())
                    .map_err(|e| anyhow::anyhow!("SetMute: {e}"))?;
            }
            return Ok(());
        }
    }
    anyhow::bail!("No audio session matching '{name}' found")
}

/// Get the mute state of the audio session whose display name contains `name`.
fn get_session_muted_by_name(
    enumerator: &IMMDeviceEnumerator,
    name: &str,
) -> anyhow::Result<bool> {
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
            let muted: BOOL = unsafe {
                simple_vol.GetMute().unwrap_or(BOOL(0))
            };
            return Ok(muted.as_bool());
        }
    }
    anyhow::bail!("No audio session matching '{name}' found")
}

/// Get the volume of the audio session whose display name contains `name`.
fn get_session_volume_by_name(
    enumerator: &IMMDeviceEnumerator,
    name: &str,
) -> anyhow::Result<f64> {
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
            let vol = unsafe {
                simple_vol.GetMasterVolume().unwrap_or(0.0) as f64
            };
            return Ok(vol);
        }
    }
    anyhow::bail!("No audio session matching '{name}' found")
}
