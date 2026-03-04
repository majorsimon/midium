//! Per-process audio volume control on macOS 14.2+ via Audio Tap API.
//!
//! Uses `AudioHardwareCreateProcessTap` to intercept audio from individual
//! processes, then scales samples in an IO proc callback to implement
//! per-app volume control.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use coreaudio_sys::*;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use tracing::{debug, error, warn};

use midium_core::types::AudioSessionInfo;

// ---------------------------------------------------------------------------
// FFI declarations for macOS 14.2+ Audio Tap API
// ---------------------------------------------------------------------------

// Property selectors not yet in coreaudio-sys
// Verified against CoreAudio/AudioHardware.h from the macOS SDK
const KAUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"prs#");
const KAUDIO_TAP_PROPERTY_UUID: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"tuid");
const KAUDIO_AGGREGATE_DEVICE_TAP_LIST_KEY: &str = "tapl";

// Process object properties (from AudioHardware.h)
const KAUDIO_PROCESS_PROPERTY_PID: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"ppid");
const KAUDIO_PROCESS_PROPERTY_BUNDLE_ID: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"pbid");

extern "C" {
    fn AudioHardwareCreateProcessTap(
        description: *const AnyObject,
        tap_id: *mut AudioObjectID,
    ) -> OSStatus;

    fn AudioHardwareDestroyProcessTap(tap_id: AudioObjectID) -> OSStatus;
}

// ---------------------------------------------------------------------------
// Tap state
// ---------------------------------------------------------------------------

struct TapState {
    tap_id: AudioObjectID,
    aggregate_device_id: AudioObjectID,
    /// Volume multiplier stored as bit-cast f64 for lock-free audio thread access.
    volume: Arc<AtomicU64>,
    muted: Arc<AtomicU64>,
    io_proc_id: Option<AudioDeviceIOProcID>,
}

impl Drop for TapState {
    fn drop(&mut self) {
        // Stop and destroy IO proc
        if let Some(io_proc) = self.io_proc_id {
            unsafe {
                AudioDeviceStop(self.aggregate_device_id, io_proc);
                AudioDeviceDestroyIOProcID(self.aggregate_device_id, io_proc);
            }
        }
        // Destroy aggregate device
        if self.aggregate_device_id != kAudioObjectUnknown {
            unsafe {
                let address = AudioObjectPropertyAddress {
                    mSelector: kAudioPlugInDestroyAggregateDevice,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain,
                };
                let size = mem::size_of::<AudioObjectID>() as u32;
                AudioObjectSetPropertyData(
                    kAudioObjectSystemObject,
                    &address,
                    0,
                    ptr::null(),
                    size,
                    &self.aggregate_device_id as *const _ as *const _,
                );
            }
        }
        // Destroy the tap
        if self.tap_id != kAudioObjectUnknown {
            unsafe {
                AudioHardwareDestroyProcessTap(self.tap_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// AudioTapManager
// ---------------------------------------------------------------------------

pub struct AudioTapManager {
    taps: Arc<Mutex<HashMap<String, TapState>>>,
}

impl AudioTapManager {
    pub fn new() -> Self {
        debug!("AudioTapManager created (macOS 14.2+ Audio Tap API)");
        Self {
            taps: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enumerate processes currently producing audio.
    pub fn enumerate_audio_processes(&self) -> Vec<AudioSessionInfo> {
        let address = AudioObjectPropertyAddress {
            mSelector: KAUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST,
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
            warn!(status, "Failed to get process object list size");
            return Vec::new();
        }

        let count = size as usize / mem::size_of::<AudioObjectID>();
        if count == 0 {
            return Vec::new();
        }

        let mut process_ids = vec![kAudioObjectUnknown; count];
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                &mut size,
                process_ids.as_mut_ptr() as *mut _,
            )
        };
        if status != 0 {
            warn!(status, "Failed to get process object list");
            return Vec::new();
        }

        let taps = self.taps.lock().unwrap();
        let mut sessions = Vec::new();

        for &proc_obj_id in &process_ids {
            let pid = get_process_pid(proc_obj_id);
            let name = get_process_bundle_id(proc_obj_id)
                .unwrap_or_else(|| format!("PID {}", pid.unwrap_or(0)));

            // Check if we have a tap for this process (to report current volume)
            let (volume, muted) = if let Some(tap) = taps.get(&name) {
                let vol = f64::from_bits(tap.volume.load(Ordering::Relaxed));
                let m = f64::from_bits(tap.muted.load(Ordering::Relaxed)) > 0.5;
                (vol, m)
            } else {
                (1.0, false)
            };

            sessions.push(AudioSessionInfo {
                name,
                pid,
                volume,
                muted,
            });
        }

        sessions
    }

    /// Set the volume multiplier for a process by name.
    pub fn set_process_volume(&self, name: &str, volume: f64) {
        let volume = volume.clamp(0.0, 1.0);
        let taps = self.taps.lock().unwrap();

        if let Some(tap) = taps.get(name) {
            tap.volume.store(volume.to_bits(), Ordering::Relaxed);
            return;
        }

        // Need to create a tap for this process
        drop(taps);
        match self.create_tap_for_process(name, volume) {
            Ok(()) => debug!(name, volume, "Created audio tap"),
            Err(e) => error!(name, %e, "Failed to create audio tap"),
        }
    }

    /// Set the mute state for a process.
    pub fn set_process_mute(&self, name: &str, muted: bool) {
        let val: f64 = if muted { 1.0 } else { 0.0 };
        let taps = self.taps.lock().unwrap();

        if let Some(tap) = taps.get(name) {
            tap.muted.store(val.to_bits(), Ordering::Relaxed);
            return;
        }

        // Create a tap if needed (muting implies we want to control this process)
        drop(taps);
        match self.create_tap_for_process(name, 1.0) {
            Ok(()) => {
                let taps = self.taps.lock().unwrap();
                if let Some(tap) = taps.get(name) {
                    tap.muted.store(val.to_bits(), Ordering::Relaxed);
                }
            }
            Err(e) => error!(name, %e, "Failed to create audio tap for mute"),
        }
    }

    /// Check if a process is muted.
    pub fn is_process_muted(&self, name: &str) -> bool {
        let taps = self.taps.lock().unwrap();
        taps.get(name)
            .map(|t| f64::from_bits(t.muted.load(Ordering::Relaxed)) > 0.5)
            .unwrap_or(false)
    }

    /// Get the current volume for a process.
    pub fn get_process_volume(&self, name: &str) -> f64 {
        let taps = self.taps.lock().unwrap();
        taps.get(name)
            .map(|t| f64::from_bits(t.volume.load(Ordering::Relaxed)))
            .unwrap_or(1.0)
    }

    /// Create an audio tap for a specific process.
    fn create_tap_for_process(&self, name: &str, initial_volume: f64) -> anyhow::Result<()> {
        // Find the PID for this process name from the process list
        let pid = self.find_pid_for_name(name)
            .ok_or_else(|| anyhow::anyhow!("No audio process found matching '{name}'"))?;

        // Create CATapDescription via Objective-C
        let tap_desc = create_tap_description(pid)?;

        // Create the process tap
        let mut tap_id: AudioObjectID = kAudioObjectUnknown;
        let status = unsafe {
            AudioHardwareCreateProcessTap(
                Retained::as_ptr(&tap_desc) as *const AnyObject,
                &mut tap_id,
            )
        };
        if status != 0 {
            anyhow::bail!("AudioHardwareCreateProcessTap failed (OSStatus: {status})");
        }

        // Get the tap's UUID
        let tap_uuid = get_tap_uuid(tap_id)?;

        // Create an aggregate device with the tap
        let aggregate_device_id = create_aggregate_device_with_tap(&tap_uuid)?;

        // Set up volume state
        let volume = Arc::new(AtomicU64::new(initial_volume.to_bits()));
        let muted = Arc::new(AtomicU64::new(0f64.to_bits()));

        // Install IO proc for volume scaling
        let io_proc_id = install_volume_io_proc(
            aggregate_device_id,
            Arc::clone(&volume),
            Arc::clone(&muted),
        )?;

        // Start the device
        let status = unsafe { AudioDeviceStart(aggregate_device_id, io_proc_id) };
        if status != 0 {
            warn!(status, "AudioDeviceStart failed — tap may not process audio");
        }

        let state = TapState {
            tap_id,
            aggregate_device_id,
            volume,
            muted,
            io_proc_id: Some(io_proc_id),
        };

        self.taps.lock().unwrap().insert(name.to_string(), state);
        Ok(())
    }

    fn find_pid_for_name(&self, name: &str) -> Option<u32> {
        let address = AudioObjectPropertyAddress {
            mSelector: KAUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST,
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
            return None;
        }

        let count = size as usize / mem::size_of::<AudioObjectID>();
        let mut process_ids = vec![kAudioObjectUnknown; count];
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                &mut size,
                process_ids.as_mut_ptr() as *mut _,
            )
        };
        if status != 0 {
            return None;
        }

        let name_lower = name.to_lowercase();
        for &proc_obj_id in &process_ids {
            if let Some(bundle_id) = get_process_bundle_id(proc_obj_id) {
                if bundle_id.to_lowercase().contains(&name_lower) {
                    return get_process_pid(proc_obj_id);
                }
            }
        }
        None
    }
}

impl Drop for AudioTapManager {
    fn drop(&mut self) {
        // All TapState entries will be dropped, cleaning up taps and devices
        if let Ok(mut taps) = self.taps.lock() {
            taps.clear();
        }
        debug!("AudioTapManager cleaned up");
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn get_process_pid(proc_obj_id: AudioObjectID) -> Option<u32> {
    let address = AudioObjectPropertyAddress {
        mSelector: KAUDIO_PROCESS_PROPERTY_PID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut pid: u32 = 0;
    let mut size = mem::size_of::<u32>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            proc_obj_id,
            &address,
            0,
            ptr::null(),
            &mut size,
            &mut pid as *mut _ as *mut _,
        )
    };

    if status == 0 { Some(pid) } else { None }
}

fn get_process_bundle_id(proc_obj_id: AudioObjectID) -> Option<String> {
    let address = AudioObjectPropertyAddress {
        mSelector: KAUDIO_PROCESS_PROPERTY_BUNDLE_ID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut cf_str: CFStringRef = ptr::null();
    let mut size = mem::size_of::<CFStringRef>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            proc_obj_id,
            &address,
            0,
            ptr::null(),
            &mut size,
            &mut cf_str as *mut _ as *mut _,
        )
    };

    if status != 0 || cf_str.is_null() {
        return None;
    }

    let name = unsafe { cfstring_to_string(cf_str) };
    unsafe { CFRelease(cf_str as *const _) };
    Some(name)
}

/// Create a CATapDescription for stereo mixdown of a single process.
fn create_tap_description(pid: u32) -> anyhow::Result<Retained<AnyObject>> {
    unsafe {
        let class = AnyClass::get(c"CATapDescription")
            .ok_or_else(|| anyhow::anyhow!("CATapDescription class not found — macOS 14.2+ required"))?;

        // Create NSNumber from pid
        let ns_number_class = AnyClass::get(c"NSNumber")
            .ok_or_else(|| anyhow::anyhow!("NSNumber class not found"))?;
        let pid_number: *mut AnyObject = msg_send![ns_number_class, numberWithUnsignedInt: pid];
        if pid_number.is_null() {
            anyhow::bail!("Failed to create NSNumber for PID {pid}");
        }

        // Create NSArray with the single PID
        let ns_array_class = AnyClass::get(c"NSArray")
            .ok_or_else(|| anyhow::anyhow!("NSArray class not found"))?;
        let objects: [*mut AnyObject; 1] = [pid_number];
        let pid_array: *mut AnyObject = msg_send![
            ns_array_class,
            arrayWithObjects: objects.as_ptr(),
            count: 1usize
        ];
        if pid_array.is_null() {
            anyhow::bail!("Failed to create NSArray for PIDs");
        }

        // [[CATapDescription alloc] initStereoMixdownOfProcesses:pidArray]
        let alloc: *mut AnyObject = msg_send![class, alloc];
        if alloc.is_null() {
            anyhow::bail!("Failed to alloc CATapDescription");
        }

        let desc: *mut AnyObject = msg_send![alloc, initStereoMixdownOfProcesses: pid_array];
        if desc.is_null() {
            anyhow::bail!("initStereoMixdownOfProcesses returned nil");
        }

        Ok(Retained::from_raw(desc)
            .ok_or_else(|| anyhow::anyhow!("Failed to retain CATapDescription"))?)
    }
}

fn get_tap_uuid(tap_id: AudioObjectID) -> anyhow::Result<String> {
    let address = AudioObjectPropertyAddress {
        mSelector: KAUDIO_TAP_PROPERTY_UUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut cf_str: CFStringRef = ptr::null();
    let mut size = mem::size_of::<CFStringRef>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            tap_id,
            &address,
            0,
            ptr::null(),
            &mut size,
            &mut cf_str as *mut _ as *mut _,
        )
    };

    if status != 0 || cf_str.is_null() {
        anyhow::bail!("Failed to get tap UUID (OSStatus: {status})");
    }

    let uuid = unsafe { cfstring_to_string(cf_str) };
    unsafe { CFRelease(cf_str as *const _) };
    Ok(uuid)
}

fn create_aggregate_device_with_tap(tap_uuid: &str) -> anyhow::Result<AudioObjectID> {
    // Build the aggregate device description dictionary via CoreFoundation
    unsafe {
        let uid_key = cfstring_from_str("uid");
        let uid_val = cfstring_from_str(&format!("midium-tap-{tap_uuid}"));
        let name_key = cfstring_from_str(std::str::from_utf8(kAudioAggregateDeviceNameKey).unwrap());
        let name_val = cfstring_from_str("Midium Tap Device");
        let tap_list_key = cfstring_from_str(KAUDIO_AGGREGATE_DEVICE_TAP_LIST_KEY);

        // Create tap list array with the UUID
        let tap_uuid_cf = cfstring_from_str(tap_uuid);
        let mut tap_uuid_ptr: *const c_void = tap_uuid_cf as *const c_void;
        let tap_array = CFArrayCreate(
            ptr::null(),
            &mut tap_uuid_ptr as *mut *const c_void,
            1,
            &kCFTypeArrayCallBacks,
        );

        // Build the dictionary
        let mut keys: [*const c_void; 3] = [
            uid_key as *const c_void,
            name_key as *const c_void,
            tap_list_key as *const c_void,
        ];
        let mut values: [*const c_void; 3] = [
            uid_val as *const c_void,
            name_val as *const c_void,
            tap_array as *const c_void,
        ];

        let dict = CFDictionaryCreate(
            ptr::null(),
            keys.as_mut_ptr(),
            values.as_mut_ptr(),
            3,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );

        let mut aggregate_id: AudioObjectID = kAudioObjectUnknown;
        let status = AudioHardwareCreateAggregateDevice(
            dict as _,
            &mut aggregate_id,
        );

        // Release CF objects
        CFRelease(dict as *const _);
        CFRelease(tap_array as *const _);
        CFRelease(tap_uuid_cf as *const _);
        CFRelease(uid_key as *const _);
        CFRelease(uid_val as *const _);
        CFRelease(name_key as *const _);
        CFRelease(name_val as *const _);
        CFRelease(tap_list_key as *const _);

        if status != 0 {
            anyhow::bail!("AudioHardwareCreateAggregateDevice failed (OSStatus: {status})");
        }

        Ok(aggregate_id)
    }
}

/// Install an IO proc that scales audio samples by the volume multiplier.
fn install_volume_io_proc(
    device_id: AudioObjectID,
    volume: Arc<AtomicU64>,
    muted: Arc<AtomicU64>,
) -> anyhow::Result<AudioDeviceIOProcID> {
    // We use a Box to pass the volume/muted Arcs to the C callback
    struct IoContext {
        volume: Arc<AtomicU64>,
        muted: Arc<AtomicU64>,
    }

    let context = Box::new(IoContext {
        volume,
        muted,
    });
    let context_ptr = Box::into_raw(context) as *mut c_void;

    extern "C" fn io_proc(
        _device: AudioObjectID,
        _now: *const AudioTimeStamp,
        _input_data: *const AudioBufferList,
        _input_time: *const AudioTimeStamp,
        output_data: *mut AudioBufferList,
        _output_time: *const AudioTimeStamp,
        client_data: *mut c_void,
    ) -> OSStatus {
        if client_data.is_null() || output_data.is_null() {
            return 0;
        }

        let ctx = unsafe { &*(client_data as *const IoContext) };
        let vol = f64::from_bits(ctx.volume.load(Ordering::Relaxed)) as f32;
        let is_muted = f64::from_bits(ctx.muted.load(Ordering::Relaxed)) > 0.5;
        let multiplier = if is_muted { 0.0f32 } else { vol };

        unsafe {
            let buf_list = &mut *output_data;
            let buffer_count = buf_list.mNumberBuffers as usize;
            let buffers = std::slice::from_raw_parts_mut(
                buf_list.mBuffers.as_mut_ptr(),
                buffer_count,
            );

            for buffer in buffers {
                if buffer.mData.is_null() {
                    continue;
                }
                let sample_count = buffer.mDataByteSize as usize / mem::size_of::<f32>();
                let samples = std::slice::from_raw_parts_mut(
                    buffer.mData as *mut f32,
                    sample_count,
                );
                for sample in samples {
                    *sample *= multiplier;
                }
            }
        }

        0
    }

    let mut io_proc_id: AudioDeviceIOProcID = None;
    let status = unsafe {
        AudioDeviceCreateIOProcID(
            device_id,
            Some(io_proc),
            context_ptr,
            &mut io_proc_id,
        )
    };

    if status != 0 {
        // Clean up the leaked context
        unsafe { drop(Box::from_raw(context_ptr as *mut IoContext)) };
        anyhow::bail!("AudioDeviceCreateIOProcID failed (OSStatus: {status})");
    }

    Ok(io_proc_id)
}

// ---------------------------------------------------------------------------
// CoreFoundation string helpers
// ---------------------------------------------------------------------------

unsafe fn cfstring_from_str(s: &str) -> CFStringRef {
    CFStringCreateWithBytes(
        ptr::null(),
        s.as_ptr(),
        s.len() as _,
        kCFStringEncodingUTF8,
        false as _,
    )
}

unsafe fn cfstring_to_string(cf_ref: CFStringRef) -> String {
    let c_ptr = CFStringGetCStringPtr(cf_ref, kCFStringEncodingUTF8);
    if !c_ptr.is_null() {
        return std::ffi::CStr::from_ptr(c_ptr)
            .to_string_lossy()
            .into_owned();
    }

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

// ---------------------------------------------------------------------------
// macOS version check
// ---------------------------------------------------------------------------

/// Returns true if running macOS 14.2 or later (Audio Tap API availability).
pub fn supports_audio_taps() -> bool {
    // Use sysctl to get kern.osproductversion
    let mut size: sysctl_ffi::size_t = 0;
    let name = c"kern.osproductversion";
    let ret = unsafe {
        sysctl_ffi::sysctlbyname(
            name.as_ptr(),
            ptr::null_mut(),
            &mut size,
            ptr::null(),
            0,
        )
    };
    if ret != 0 || size == 0 {
        return false;
    }

    let mut buf = vec![0u8; size];
    let ret = unsafe {
        sysctl_ffi::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
            ptr::null(),
            0,
        )
    };
    if ret != 0 {
        return false;
    }

    // Parse version string like "14.2.1"
    let version = String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .to_string();
    let parts: Vec<u32> = version
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();

    match (parts.first(), parts.get(1)) {
        (Some(&major), Some(&minor)) => major > 14 || (major == 14 && minor >= 2),
        (Some(&major), None) => major > 14,
        _ => false,
    }
}

// Need libc for sysctl
#[allow(non_camel_case_types)]
mod sysctl_ffi {
    pub type size_t = usize;

    extern "C" {
        pub fn sysctlbyname(
            name: *const std::ffi::c_char,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut size_t,
            newp: *const std::ffi::c_void,
            newlen: size_t,
        ) -> std::ffi::c_int;
    }
}
