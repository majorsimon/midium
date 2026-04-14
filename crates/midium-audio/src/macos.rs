use std::mem;
use std::ptr;

use coreaudio_sys::*;
use tracing::{debug, warn};

use midium_core::dispatch::VolumeControl;
use midium_core::types::{AudioCapabilities, AudioDeviceInfo, AudioSessionInfo, AudioTarget};

use crate::backend::AudioBackend;
use crate::macos_tap::{self, AudioTapManager};

pub struct CoreAudioBackend {
    tap_manager: Option<AudioTapManager>,
}

impl CoreAudioBackend {
    pub fn new() -> anyhow::Result<Self> {
        let tap_manager = if macos_tap::supports_audio_taps() {
            debug!("macOS 14.2+ detected — enabling per-app volume via Audio Tap API");
            Some(AudioTapManager::new())
        } else {
            debug!("macOS < 14.2 — per-app volume not available");
            None
        };
        debug!("CoreAudio backend initialized");
        Ok(Self { tap_manager })
    }

    /// Get the default output device ID.
    fn default_output_device() -> anyhow::Result<AudioObjectID> {
        let mut device_id: AudioObjectID = kAudioObjectUnknown;
        let mut size = mem::size_of::<AudioObjectID>() as u32;

        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut device_id as *mut _ as *mut _,
            )
        };

        if status != 0 {
            anyhow::bail!("Failed to get default output device (OSStatus: {status})");
        }
        Ok(device_id)
    }

    /// Get the default input device ID.
    fn default_input_device() -> anyhow::Result<AudioObjectID> {
        let mut device_id: AudioObjectID = kAudioObjectUnknown;
        let mut size = mem::size_of::<AudioObjectID>() as u32;

        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut device_id as *mut _ as *mut _,
            )
        };

        if status != 0 {
            anyhow::bail!("Failed to get default input device (OSStatus: {status})");
        }
        Ok(device_id)
    }

    /// Get the name of an audio device by its ID.
    fn device_name(device_id: AudioObjectID) -> anyhow::Result<String> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioObjectPropertyName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut name_ref: CFStringRef = ptr::null();
        let mut size = mem::size_of::<CFStringRef>() as u32;

        let status = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut name_ref as *mut _ as *mut _,
            )
        };

        if status != 0 {
            anyhow::bail!("Failed to get device name (OSStatus: {status})");
        }

        let name = unsafe { cfstring_to_string(name_ref) };
        unsafe { CFRelease(name_ref as *const _) };
        Ok(name)
    }

    /// List all audio devices (either input or output based on scope).
    fn list_devices_with_scope(
        &self,
        scope: AudioObjectPropertyScope,
        is_input: bool,
    ) -> anyhow::Result<Vec<AudioDeviceInfo>> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut size: u32 = 0;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                &mut size,
            )
        };
        if status != 0 {
            anyhow::bail!("Failed to get device list size (OSStatus: {status})");
        }

        let count = size as usize / mem::size_of::<AudioObjectID>();
        let mut device_ids = vec![kAudioObjectUnknown; count];

        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                &mut size,
                device_ids.as_mut_ptr() as *mut _,
            )
        };
        if status != 0 {
            anyhow::bail!("Failed to get device list (OSStatus: {status})");
        }

        let default_id = if is_input {
            Self::default_input_device().unwrap_or(kAudioObjectUnknown)
        } else {
            Self::default_output_device().unwrap_or(kAudioObjectUnknown)
        };

        let mut devices = Vec::new();
        for &dev_id in &device_ids {
            // Check if this device has streams in the requested scope
            if !Self::has_streams(dev_id, scope) {
                continue;
            }
            let name = Self::device_name(dev_id).unwrap_or_else(|_| format!("Device {dev_id}"));
            devices.push(AudioDeviceInfo {
                id: dev_id.to_string(),
                name,
                is_default: dev_id == default_id,
                is_input,
            });
        }

        Ok(devices)
    }

    /// Check if a device has audio streams in the given scope.
    fn has_streams(device_id: AudioObjectID, scope: AudioObjectPropertyScope) -> bool {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreams,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut size: u32 = 0;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(device_id, &address, 0, ptr::null(), &mut size)
        };
        status == 0 && size > 0
    }

    /// Get volume for a specific device ID.
    fn get_device_volume(device_id: AudioObjectID) -> anyhow::Result<f64> {
        // Try the master channel first, then channel 1
        for channel in [kAudioObjectPropertyElementMain, 1] {
            let address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyVolumeScalar,
                mScope: kAudioDevicePropertyScopeOutput,
                mElement: channel,
            };

            let has = unsafe {
                AudioObjectHasProperty(device_id, &address)
            };

            if has != 0 {
                let mut volume: f32 = 0.0;
                let mut size = mem::size_of::<f32>() as u32;

                let status = unsafe {
                    AudioObjectGetPropertyData(
                        device_id,
                        &address,
                        0,
                        ptr::null(),
                        &mut size,
                        &mut volume as *mut _ as *mut _,
                    )
                };

                if status == 0 {
                    return Ok(volume as f64);
                }
            }
        }

        anyhow::bail!("Device {device_id} does not support volume control")
    }

    /// Set volume for a specific device ID.
    fn set_device_volume(device_id: AudioObjectID, volume: f32) -> anyhow::Result<()> {
        for channel in [kAudioObjectPropertyElementMain, 1, 2] {
            let address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyVolumeScalar,
                mScope: kAudioDevicePropertyScopeOutput,
                mElement: channel,
            };

            let has = unsafe {
                AudioObjectHasProperty(device_id, &address)
            };

            if has != 0 {
                let settable = unsafe {
                    let mut s: u8 = 0;
                    AudioObjectIsPropertySettable(device_id, &address, &mut s as *mut u8);
                    s != 0
                };

                if settable {
                    let size = mem::size_of::<f32>() as u32;
                    let status = unsafe {
                        AudioObjectSetPropertyData(
                            device_id,
                            &address,
                            0,
                            ptr::null(),
                            size,
                            &volume as *const _ as *const _,
                        )
                    };

                    if status != 0 {
                        warn!(device_id, channel, status, "Failed to set volume on channel");
                    }
                }
            }
        }
        Ok(())
    }

    /// Get mute state for a device.
    fn get_device_mute(device_id: AudioObjectID) -> anyhow::Result<bool> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMain,
        };

        let has = unsafe { AudioObjectHasProperty(device_id, &address) };
        if has == 0 {
            return Ok(false); // device doesn't support mute query
        }

        let mut muted: u32 = 0;
        let mut size = mem::size_of::<u32>() as u32;

        let status = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut muted as *mut _ as *mut _,
            )
        };

        if status != 0 {
            anyhow::bail!("Failed to get mute state (OSStatus: {status})");
        }
        Ok(muted != 0)
    }

    /// Set mute state for a device.
    fn set_device_mute(device_id: AudioObjectID, muted: bool) -> anyhow::Result<()> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMain,
        };

        let value: u32 = if muted { 1 } else { 0 };
        let size = mem::size_of::<u32>() as u32;

        let status = unsafe {
            AudioObjectSetPropertyData(
                device_id,
                &address,
                0,
                ptr::null(),
                size,
                &value as *const _ as *const _,
            )
        };

        if status != 0 {
            anyhow::bail!("Failed to set mute state (OSStatus: {status})");
        }
        Ok(())
    }

    /// Set the default output device.
    fn set_default_output_device(device_id: AudioObjectID) -> anyhow::Result<()> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let size = mem::size_of::<AudioObjectID>() as u32;
        let status = unsafe {
            AudioObjectSetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                size,
                &device_id as *const _ as *const _,
            )
        };

        if status != 0 {
            anyhow::bail!("Failed to set default output device (OSStatus: {status})");
        }
        Ok(())
    }

    /// Set the default input device.
    fn set_default_input_device(device_id: AudioObjectID) -> anyhow::Result<()> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let size = mem::size_of::<AudioObjectID>() as u32;
        let status = unsafe {
            AudioObjectSetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                size,
                &device_id as *const _ as *const _,
            )
        };

        if status != 0 {
            anyhow::bail!("Failed to set default input device (OSStatus: {status})");
        }
        Ok(())
    }

    fn resolve_device_id(target: &AudioTarget) -> anyhow::Result<AudioObjectID> {
        match target {
            AudioTarget::SystemMaster => Self::default_output_device(),
            AudioTarget::Device { id } => {
                id.parse::<AudioObjectID>()
                    .map_err(|_| anyhow::anyhow!("Invalid device ID: {id}"))
            }
            AudioTarget::Application { .. } | AudioTarget::FocusedApplication => {
                // Per-app targets are handled separately via the tap manager
                anyhow::bail!("Per-application volume requires Audio Tap API (macOS 14.2+)")
            }
        }
    }
}

impl VolumeControl for CoreAudioBackend {
    fn set_volume(&self, target: &AudioTarget, volume: f64) -> anyhow::Result<()> {
        if let AudioTarget::Application { name } = target {
            if let Some(ref mgr) = self.tap_manager {
                mgr.set_process_volume(name, volume);
                return Ok(());
            }
            anyhow::bail!("Per-app volume not available (requires macOS 14.2+)");
        }
        let device_id = Self::resolve_device_id(target)?;
        Self::set_device_volume(device_id, volume as f32)
    }

    fn set_mute(&self, target: &AudioTarget, muted: bool) -> anyhow::Result<()> {
        if let AudioTarget::Application { name } = target {
            if let Some(ref mgr) = self.tap_manager {
                mgr.set_process_mute(name, muted);
                return Ok(());
            }
            anyhow::bail!("Per-app volume not available (requires macOS 14.2+)");
        }
        let device_id = Self::resolve_device_id(target)?;
        Self::set_device_mute(device_id, muted)
    }

    fn is_muted(&self, target: &AudioTarget) -> anyhow::Result<bool> {
        if let AudioTarget::Application { name } = target {
            if let Some(ref mgr) = self.tap_manager {
                return Ok(mgr.is_process_muted(name));
            }
            anyhow::bail!("Per-app volume not available (requires macOS 14.2+)");
        }
        let device_id = Self::resolve_device_id(target)?;
        Self::get_device_mute(device_id)
    }

    fn set_default_output(&self, device_id: &str) -> anyhow::Result<()> {
        let id: AudioObjectID = device_id.parse()
            .map_err(|_| anyhow::anyhow!("Invalid device ID: {device_id}"))?;
        Self::set_default_output_device(id)
    }

    fn set_default_input(&self, device_id: &str) -> anyhow::Result<()> {
        let id: AudioObjectID = device_id.parse()
            .map_err(|_| anyhow::anyhow!("Invalid device ID: {device_id}"))?;
        Self::set_default_input_device(id)
    }

    fn is_default_output(&self, device_id: &str) -> anyhow::Result<bool> {
        let requested: AudioObjectID = device_id.parse()
            .map_err(|_| anyhow::anyhow!("Invalid device ID: {device_id}"))?;
        Ok(Self::default_output_device()? == requested)
    }
}

impl AudioBackend for CoreAudioBackend {
    fn list_output_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>> {
        self.list_devices_with_scope(kAudioDevicePropertyScopeOutput, false)
    }

    fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>> {
        self.list_devices_with_scope(kAudioDevicePropertyScopeInput, true)
    }

    fn get_volume(&self, target: &AudioTarget) -> anyhow::Result<f64> {
        if let AudioTarget::Application { name } = target {
            if let Some(ref mgr) = self.tap_manager {
                return Ok(mgr.get_process_volume(name));
            }
            anyhow::bail!("Per-app volume not available (requires macOS 14.2+)");
        }
        let device_id = Self::resolve_device_id(target)?;
        Self::get_device_volume(device_id)
    }

    fn list_sessions(&self) -> anyhow::Result<Vec<AudioSessionInfo>> {
        if let Some(ref mgr) = self.tap_manager {
            return Ok(mgr.enumerate_audio_processes());
        }
        Ok(Vec::new())
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            per_app_volume: macos_tap::supports_audio_taps(),
            device_switching: true,
            input_device_switching: true,
        }
    }
}

// ---------------------------------------------------------------------------
// CFString helper
// ---------------------------------------------------------------------------

unsafe fn cfstring_to_string(cf_ref: CFStringRef) -> String {
    let c_ptr = CFStringGetCStringPtr(cf_ref, kCFStringEncodingUTF8);
    if !c_ptr.is_null() {
        return std::ffi::CStr::from_ptr(c_ptr)
            .to_string_lossy()
            .into_owned();
    }

    // Fallback: CFStringGetCStringPtr can return null
    let len = CFStringGetLength(cf_ref);
    let max_size = CFStringGetMaximumSizeForEncoding(len, kCFStringEncodingUTF8) + 1;
    let mut buf = vec![0u8; max_size as usize];
    let success = CFStringGetCString(
        cf_ref,
        buf.as_mut_ptr() as *mut _,
        max_size,
        kCFStringEncodingUTF8,
    );
    if success != 0 {
        let cstr = std::ffi::CStr::from_ptr(buf.as_ptr() as *const _);
        cstr.to_string_lossy().into_owned()
    } else {
        String::from("(unknown)")
    }
}
