use tracing::{debug, warn};
use crate::types::{Action, AudioTarget};

/// Trait implemented by the audio backend so the dispatcher can call into it.
pub trait VolumeControl: Send + Sync {
    fn set_volume(&self, target: &AudioTarget, volume: f64) -> anyhow::Result<()>;
    fn set_mute(&self, target: &AudioTarget, muted: bool) -> anyhow::Result<()>;
    fn is_muted(&self, target: &AudioTarget) -> anyhow::Result<bool>;
    fn set_default_output(&self, device_id: &str) -> anyhow::Result<()>;
    fn set_default_input(&self, device_id: &str) -> anyhow::Result<()>;
}

/// Trait implemented by the shortcuts backend (media keys, keyboard shortcuts).
///
/// Kept in midium-core so ActionDispatcher can hold it without creating a
/// circular dependency (midium-shortcuts depends on midium-core, not vice versa).
pub trait ShortcutExecutor: Send + Sync {
    fn execute(&self, action: &Action);
}

/// Routes resolved actions to the appropriate subsystem (audio, shortcuts, plugins).
pub struct ActionDispatcher {
    audio: Box<dyn VolumeControl>,
    shortcuts: Option<Box<dyn ShortcutExecutor>>,
}

impl ActionDispatcher {
    pub fn new(audio: Box<dyn VolumeControl>) -> Self {
        Self { audio, shortcuts: None }
    }

    pub fn with_shortcuts(
        audio: Box<dyn VolumeControl>,
        shortcuts: Box<dyn ShortcutExecutor>,
    ) -> Self {
        Self { audio, shortcuts: Some(shortcuts) }
    }

    /// Execute an action with the given transformed value (0.0–1.0).
    pub fn dispatch(&self, action: &Action, value: f64) {
        match action {
            Action::SetVolume { target } => {
                debug!(?target, volume = value, "Setting volume");
                if let Err(e) = self.audio.set_volume(target, value) {
                    warn!("Failed to set volume: {e}");
                }
            }
            Action::ToggleMute { target } => {
                match self.audio.is_muted(target) {
                    Ok(muted) => {
                        debug!(?target, new_muted = !muted, "Toggling mute");
                        if let Err(e) = self.audio.set_mute(target, !muted) {
                            warn!("Failed to toggle mute: {e}");
                        }
                    }
                    Err(e) => warn!("Failed to check mute state: {e}"),
                }
            }
            Action::SetDefaultOutput { device_id } => {
                debug!(device_id, "Switching default output");
                if let Err(e) = self.audio.set_default_output(device_id) {
                    warn!("Failed to set default output: {e}");
                }
            }
            Action::SetDefaultInput { device_id } => {
                debug!(device_id, "Switching default input");
                if let Err(e) = self.audio.set_default_input(device_id) {
                    warn!("Failed to set default input: {e}");
                }
            }
            Action::ActionGroup { actions } => {
                for a in actions {
                    self.dispatch(a, value);
                }
            }
            // Handled by PluginManager's own EventBus subscription
            Action::RunPluginAction { .. } => {}
            // Shortcuts / media keys
            Action::MediaPlayPause
            | Action::MediaNext
            | Action::MediaPrev
            | Action::CycleOutputDevices
            | Action::CycleInputDevices
            | Action::SendKeyboardShortcut { .. } => {
                if let Some(sc) = &self.shortcuts {
                    sc.execute(action);
                } else {
                    warn!(?action, "No shortcut executor registered");
                }
            }
        }
    }
}
