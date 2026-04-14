//! Shared CoreFoundation string helpers for macOS audio backends.

use std::ptr;
use coreaudio_sys::*;

/// Convert a `CFStringRef` to an owned Rust `String`.
///
/// # Safety
/// `cf_ref` must be a valid, non-null `CFStringRef`.
pub unsafe fn cfstring_to_string(cf_ref: CFStringRef) -> String {
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

/// Create a `CFStringRef` from a Rust `&str`.
///
/// # Safety
/// The returned `CFStringRef` must be released by the caller (or consumed by a
/// CoreFoundation function that takes ownership).
pub unsafe fn cfstring_from_str(s: &str) -> CFStringRef {
    CFStringCreateWithBytes(
        ptr::null(),
        s.as_ptr(),
        s.len() as _,
        kCFStringEncodingUTF8,
        false as _,
    )
}
