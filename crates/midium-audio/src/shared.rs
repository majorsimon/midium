use std::sync::Arc;

use midium_core::dispatch::{DeviceLister, VolumeControl};
use midium_core::types::AudioTarget;

use crate::backend::AudioBackend;

/// Adapter that bridges `Arc<dyn AudioBackend>` to `Box<dyn VolumeControl>`
/// and `Box<dyn DeviceLister>`.
///
/// `ActionDispatcher` and `GroupManager` take `Box<dyn VolumeControl>`, but
/// callers need the full `AudioBackend` trait for IPC queries. This newtype
/// lets both sides share the same backend instance via `Arc`.
pub struct SharedAudio(pub Arc<dyn AudioBackend>);

impl VolumeControl for SharedAudio {
    fn set_volume(&self, t: &AudioTarget, v: f64) -> anyhow::Result<()> {
        self.0.set_volume(t, v)
    }
    fn set_mute(&self, t: &AudioTarget, m: bool) -> anyhow::Result<()> {
        self.0.set_mute(t, m)
    }
    fn is_muted(&self, t: &AudioTarget) -> anyhow::Result<bool> {
        self.0.is_muted(t)
    }
    fn set_default_output(&self, id: &str) -> anyhow::Result<()> {
        self.0.set_default_output(id)
    }
    fn set_default_input(&self, id: &str) -> anyhow::Result<()> {
        self.0.set_default_input(id)
    }
    fn is_default_output(&self, id: &str) -> anyhow::Result<bool> {
        self.0.is_default_output(id)
    }
}

impl DeviceLister for SharedAudio {
    fn list_output_device_ids(&self) -> Vec<(String, bool)> {
        self.0
            .list_output_devices()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.is_default))
            .collect()
    }
    fn list_input_device_ids(&self) -> Vec<(String, bool)> {
        self.0
            .list_input_devices()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.is_default))
            .collect()
    }
}
