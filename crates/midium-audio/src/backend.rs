use midium_core::types::{AudioCapabilities, AudioDeviceInfo, AudioSessionInfo, AudioTarget};
use midium_core::dispatch::VolumeControl;

/// Platform-agnostic audio backend trait.
///
/// Each platform implements this trait using its native audio API:
/// - macOS: CoreAudio
/// - Linux: PulseAudio (PipeWire compatible)
/// - Windows: WASAPI
///
/// Requires `Send + Sync` so it can be held in Tauri's managed state.
pub trait AudioBackend: VolumeControl + Send + Sync {
    /// List all output (playback) devices.
    fn list_output_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>>;

    /// List all input (capture) devices.
    fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>>;

    /// Get the current system master volume (0.0–1.0).
    fn get_volume(&self, target: &AudioTarget) -> anyhow::Result<f64>;

    /// List per-application audio sessions (if supported).
    fn list_sessions(&self) -> anyhow::Result<Vec<AudioSessionInfo>>;

    /// Query what this backend supports.
    fn capabilities(&self) -> AudioCapabilities;
}
