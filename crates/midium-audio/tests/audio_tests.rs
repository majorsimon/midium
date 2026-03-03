use midium_audio::create_backend;
use midium_core::types::AudioTarget;

#[test]
fn test_backend_creates_successfully() {
    let backend = create_backend();
    assert!(backend.is_ok(), "create_backend() failed: {:?}", backend.err());
}

#[test]
fn test_capabilities_report() {
    let backend = create_backend().unwrap();
    let caps = backend.capabilities();

    // All platforms should report these fields without panicking
    let _ = caps.per_app_volume;
    let _ = caps.device_switching;
    let _ = caps.input_device_switching;

    #[cfg(target_os = "macos")]
    {
        // macOS always supports device switching
        assert!(caps.device_switching);
        assert!(caps.input_device_switching);
    }

    #[cfg(target_os = "windows")]
    {
        // Windows WASAPI supports per-app volume
        assert!(caps.per_app_volume);
    }
}

#[test]
fn test_list_devices() {
    let backend = create_backend().unwrap();

    // These should not error even in CI without audio hardware (may return empty)
    let outputs = backend.list_output_devices();
    assert!(outputs.is_ok(), "list_output_devices failed: {:?}", outputs.err());

    let inputs = backend.list_input_devices();
    assert!(inputs.is_ok(), "list_input_devices failed: {:?}", inputs.err());
}

#[test]
fn test_volume_clamping() {
    let backend = create_backend().unwrap();

    // Setting volume outside 0.0-1.0 should not panic
    // (may fail due to no audio device in CI, but should not panic)
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
