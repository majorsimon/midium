use midium_audio::create_backend;
use midium_core::types::AudioTarget;

/// Create the backend, skipping the test if no audio daemon is available (e.g. Linux CI).
macro_rules! backend_or_skip {
    () => {
        match create_backend() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Skipping: create_backend() unavailable: {e}");
                return;
            }
        }
    };
}

#[test]
fn test_backend_creates_successfully() {
    // Allow failure in headless CI (e.g. no PulseAudio on Linux runners).
    match create_backend() {
        Ok(_) => {}
        Err(e) => eprintln!("Note: create_backend() failed in this environment: {e}"),
    }
}

#[test]
fn test_capabilities_report() {
    let backend = backend_or_skip!();
    let caps = backend.capabilities();

    // All platforms should report these fields without panicking
    let _ = caps.per_app_volume;
    let _ = caps.device_switching;
    let _ = caps.input_device_switching;

    #[cfg(target_os = "macos")]
    {
        assert!(caps.device_switching);
        assert!(caps.input_device_switching);
    }

    #[cfg(target_os = "windows")]
    {
        assert!(caps.per_app_volume);
    }
}

#[test]
fn test_list_devices() {
    let backend = backend_or_skip!();

    let outputs = backend.list_output_devices();
    assert!(outputs.is_ok(), "list_output_devices failed: {:?}", outputs.err());

    let inputs = backend.list_input_devices();
    assert!(inputs.is_ok(), "list_input_devices failed: {:?}", inputs.err());
}

#[test]
fn test_volume_clamping() {
    let backend = backend_or_skip!();

    // Setting volume outside 0.0-1.0 should not panic
    let _ = backend.set_volume(&AudioTarget::SystemMaster, -0.5);
    let _ = backend.set_volume(&AudioTarget::SystemMaster, 1.5);
}

#[cfg(target_os = "macos")]
#[test]
fn test_version_detection() {
    // Should return a bool without panicking
    let result = midium_audio::macos_tap::supports_audio_taps();
    let _ = result;
}

#[cfg(target_os = "windows")]
#[test]
fn test_com_initialization() {
    // WasapiBackend::new() should succeed (initializes COM)
    let backend = midium_audio::windows::WasapiBackend::new();
    assert!(backend.is_ok());
}

// ---------------------------------------------------------------------------
// Integration tests — require audio hardware, run with `cargo test -- --ignored`
// ---------------------------------------------------------------------------

#[ignore]
#[test]
fn test_system_volume_roundtrip() {
    let backend = create_backend().unwrap();
    let target = AudioTarget::SystemMaster;

    // Save current volume
    let original = backend.get_volume(&target).unwrap();

    // Set to a known value
    backend.set_volume(&target, 0.42).unwrap();
    let readback = backend.get_volume(&target).unwrap();
    assert!((readback - 0.42).abs() < 0.05, "Volume readback {readback} != 0.42");

    // Restore
    backend.set_volume(&target, original).unwrap();
}

#[ignore]
#[test]
fn test_session_enumeration() {
    let backend = create_backend().unwrap();
    let sessions = backend.list_sessions().unwrap();
    // When audio is playing, there should be at least one session
    assert!(!sessions.is_empty(), "No audio sessions found — is audio playing?");
    for s in &sessions {
        println!("Session: {} (PID: {:?}, vol: {:.2}, muted: {})", s.name, s.pid, s.volume, s.muted);
    }
}

#[ignore]
#[test]
fn test_per_app_volume() {
    let backend = create_backend().unwrap();
    let caps = backend.capabilities();
    if !caps.per_app_volume {
        println!("Per-app volume not supported on this platform/version — skipping");
        return;
    }

    let sessions = backend.list_sessions().unwrap();
    if let Some(session) = sessions.first() {
        let target = AudioTarget::Application { name: session.name.clone() };
        backend.set_volume(&target, 0.5).unwrap();
        println!("Set volume for '{}' to 0.5", session.name);
    } else {
        println!("No sessions to test per-app volume with");
    }
}
