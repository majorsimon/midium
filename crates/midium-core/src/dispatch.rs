use tracing::{debug, info, warn};

use crate::event_bus::EventBus;
use crate::types::{Action, AppEvent, AudioTarget};

/// Trait implemented by the audio backend so the dispatcher can call into it.
pub trait VolumeControl: Send + Sync {
    fn set_volume(&self, target: &AudioTarget, volume: f64) -> anyhow::Result<()>;
    fn set_mute(&self, target: &AudioTarget, muted: bool) -> anyhow::Result<()>;
    fn is_muted(&self, target: &AudioTarget) -> anyhow::Result<bool>;
    fn set_default_output(&self, device_id: &str) -> anyhow::Result<()>;
    fn set_default_input(&self, device_id: &str) -> anyhow::Result<()>;
    /// Returns whether `device_id` is currently the system default output device.
    /// Backends that don't support this query should leave the default impl (returns `false`).
    fn is_default_output(&self, _device_id: &str) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Trait for listing audio devices, used by the dispatcher for CycleDevices actions.
/// Kept separate from VolumeControl so it can be optionally provided.
pub trait DeviceLister: Send + Sync {
    fn list_output_device_ids(&self) -> Vec<(String, bool)>; // (id, is_default)
    fn list_input_device_ids(&self) -> Vec<(String, bool)>;
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
    devices: Option<Box<dyn DeviceLister>>,
    event_bus: Option<EventBus>,
}

impl ActionDispatcher {
    pub fn new(audio: Box<dyn VolumeControl>) -> Self {
        Self {
            audio,
            shortcuts: None,
            devices: None,
            event_bus: None,
        }
    }

    pub fn with_shortcuts(
        audio: Box<dyn VolumeControl>,
        shortcuts: Box<dyn ShortcutExecutor>,
    ) -> Self {
        Self {
            audio,
            shortcuts: Some(shortcuts),
            devices: None,
            event_bus: None,
        }
    }

    pub fn with_device_lister(mut self, lister: Box<dyn DeviceLister>) -> Self {
        self.devices = Some(lister);
        self
    }

    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Cycle to the next device in the list, wrapping around.
    fn cycle_device(&self, devices: Vec<(String, bool)>, is_output: bool) {
        if devices.is_empty() {
            return;
        }
        let current_idx = devices.iter().position(|(_, def)| *def).unwrap_or(0);
        let next_idx = (current_idx + 1) % devices.len();
        let next_id = &devices[next_idx].0;
        let direction = if is_output { "output" } else { "input" };
        info!(from = current_idx, to = next_idx, device = %next_id, "Cycling {direction} device");
        let result = if is_output {
            self.audio.set_default_output(next_id)
        } else {
            self.audio.set_default_input(next_id)
        };
        if let Err(e) = result {
            warn!("Failed to cycle {direction} device: {e}");
        }
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
            Action::CycleOutputDevices => {
                if let Some(lister) = &self.devices {
                    self.cycle_device(lister.list_output_device_ids(), true);
                } else {
                    warn!("CycleOutputDevices: no device lister registered");
                }
            }
            Action::CycleInputDevices => {
                if let Some(lister) = &self.devices {
                    self.cycle_device(lister.list_input_device_ids(), false);
                } else {
                    warn!("CycleInputDevices: no device lister registered");
                }
            }
            // Shortcuts / media keys
            Action::MediaPlayPause
            | Action::MediaNext
            | Action::MediaPrev
            | Action::SendKeyboardShortcut { .. } => {
                if let Some(sc) = &self.shortcuts {
                    sc.execute(action);
                } else {
                    warn!(?action, "No shortcut executor registered");
                }
            }
            Action::SendMidiMessage {
                device,
                channel,
                message_type,
                number,
                value,
            } => {
                let ch = *channel & 0x0F;
                let num = *number & 0x7F;
                let vel = *value & 0x7F;
                let status = match message_type.as_str() {
                    "cc" => 0xB0 | ch,
                    "note" => 0x90 | ch,
                    other => {
                        warn!(message_type = %other, "SendMidiMessage: expected message_type \"cc\" or \"note\"");
                        return;
                    }
                };
                let data = vec![status, num, vel];
                if let Some(bus) = &self.event_bus {
                    bus.publish(AppEvent::SendMidi {
                        device: device.clone(),
                        data,
                    });
                } else {
                    warn!("SendMidiMessage: no event bus registered");
                }
            }
        }
    }
}
