use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// MIDI Types
// ---------------------------------------------------------------------------

/// A parsed MIDI event from a physical controller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiEvent {
    /// Which device this came from (port name).
    pub device: String,
    /// MIDI channel (0-15).
    pub channel: u8,
    /// The specific message type.
    pub message: MidiMessage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MidiMessage {
    ControlChange { control: u8, value: u8 },
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8, velocity: u8 },
    PitchBend { value: u16 },
}

// ---------------------------------------------------------------------------
// Control Identity — used for mapping lookups
// ---------------------------------------------------------------------------

/// Uniquely identifies a physical control on a specific device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControlId {
    /// Device name pattern or exact match.
    pub device: String,
    /// MIDI channel.
    pub channel: u8,
    /// The type of MIDI control.
    pub control_type: ControlType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlType {
    CC(u8),
    Note(u8),
    PitchBend,
}

impl ControlId {
    /// Build a ControlId from a MidiEvent.
    pub fn from_event(event: &MidiEvent) -> Self {
        let control_type = match &event.message {
            MidiMessage::ControlChange { control, .. } => ControlType::CC(*control),
            MidiMessage::NoteOn { note, .. } | MidiMessage::NoteOff { note, .. } => {
                ControlType::Note(*note)
            }
            MidiMessage::PitchBend { .. } => ControlType::PitchBend,
        };
        Self {
            device: event.device.clone(),
            channel: event.channel,
            control_type,
        }
    }
}

// ---------------------------------------------------------------------------
// Actions — what a mapped control triggers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    SetVolume { target: AudioTarget },
    ToggleMute { target: AudioTarget },
    SetDefaultOutput { device_id: String },
    SetDefaultInput { device_id: String },
    CycleOutputDevices,
    CycleInputDevices,
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    RunPluginAction { plugin: String, action: String },
    SendKeyboardShortcut { keys: Vec<String> },
    ActionGroup { actions: Vec<Action> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioTarget {
    SystemMaster,
    Device { id: String },
    Application { name: String },
    FocusedApplication,
}

// ---------------------------------------------------------------------------
// Value Transforms
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ValueTransform {
    /// Raw 0-127 mapped linearly to 0.0-1.0
    Linear,
    /// Logarithmic curve (feels natural for volume)
    Logarithmic,
    /// Relative encoder: values 1-63 = increment, 65-127 = decrement
    RelativeEncoder { sensitivity: f64 },
    /// Button toggles between 0.0 and 1.0
    Toggle,
    /// Value tracks button state (pressed=1.0, released=0.0)
    Momentary,
}

impl ValueTransform {
    /// Transform a raw MIDI value (0-127) into a normalized 0.0-1.0 float.
    ///
    /// Returns `None` to suppress the action entirely (e.g. button release for
    /// Toggle/Momentary, where only the press should trigger).
    pub fn apply(&self, raw: u8, current: f64) -> Option<f64> {
        match self {
            ValueTransform::Linear => Some(raw as f64 / 127.0),
            ValueTransform::Logarithmic => {
                let normalized = raw as f64 / 127.0;
                // Perceptual power curve: more resolution at low volumes.
                // Maps 0→0, ~0.5→0.25, 1→1.
                Some(normalized.powi(2))
            }
            ValueTransform::RelativeEncoder { sensitivity } => {
                let delta = if raw < 64 {
                    raw as f64 * sensitivity
                } else {
                    -((128 - raw) as f64) * sensitivity
                };
                Some((current + delta).clamp(0.0, 1.0))
            }
            ValueTransform::Toggle => {
                if raw > 0 {
                    // Press: flip the state
                    Some(if current > 0.5 { 0.0 } else { 1.0 })
                } else {
                    // Release: suppress — we already acted on the press
                    None
                }
            }
            ValueTransform::Momentary => {
                if raw > 0 {
                    // Press only
                    Some(1.0)
                } else {
                    // Release: suppress
                    None
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fader Group — links a profile channel strip to an audio target
// ---------------------------------------------------------------------------

/// A fader-group binding: ties a profile channel-strip group to an audio target.
///
/// When active:
/// - The strip's fader (slider/encoder) sets volume on the target.
/// - The M button toggles mute on the target.
/// - S LED = always lit (group has an assignment).
/// - M LED = lit when muted.
/// - R LED = lit when not muted (target is active).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaderGroup {
    /// Device name pattern for fuzzy matching (case-insensitive substring).
    pub device: String,
    /// Profile `group` number that this binding applies to.
    pub group: u8,
    /// Audio target controlled by this channel strip.
    pub target: AudioTarget,
    /// How to transform the fader's MIDI value. Defaults to `Logarithmic`.
    #[serde(default = "default_fader_transform")]
    pub transform: ValueTransform,
}

fn default_fader_transform() -> ValueTransform {
    ValueTransform::Logarithmic
}

// ---------------------------------------------------------------------------
// Mapping entry (persisted in TOML)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    pub control: ControlId,
    pub action: Action,
    #[serde(default = "default_transform")]
    pub transform: ValueTransform,
}

fn default_transform() -> ValueTransform {
    ValueTransform::Linear
}

// ---------------------------------------------------------------------------
// App-level events on the EventBus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum AppEvent {
    Midi(MidiEvent),
    /// Published when an audio backend detects an external volume change.
    /// Currently handled in the Tauri event bridge but not yet emitted by any backend.
    VolumeChanged {
        target: AudioTarget,
        volume: f64,
    },
    /// Published when an audio backend detects an external mute state change.
    /// Not yet emitted by any backend.
    MuteChanged {
        target: AudioTarget,
        muted: bool,
    },
    DeviceConnected {
        device: String,
    },
    DeviceDisconnected {
        device: String,
    },
    ActionTriggered {
        action: Action,
        value: f64,
    },
    /// Request the MIDI manager to send raw bytes to a device output port.
    SendMidi {
        device: String,
        data: Vec<u8>,
    },
    /// Notify the GroupManager that fader group config has changed.
    GroupsChanged {
        groups: Vec<FaderGroup>,
    },
    Shutdown,
}

// ---------------------------------------------------------------------------
// Audio device info (returned by AudioBackend)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub is_input: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSessionInfo {
    pub name: String,
    pub pid: Option<u32>,
    pub volume: f64,
    pub muted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AudioCapabilities {
    pub per_app_volume: bool,
    pub device_switching: bool,
    pub input_device_switching: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_transform_full_range() {
        let t = ValueTransform::Logarithmic;
        assert_eq!(t.apply(0, 0.0), Some(0.0));
        // Mid-point should be ~25% (0.5^2 = 0.25), not 0
        let mid = t.apply(64, 0.0).unwrap();
        assert!(mid > 0.1 && mid < 0.4, "mid={mid}");
        // Max should be 1.0
        assert_eq!(t.apply(127, 0.0), Some(1.0));
    }

    #[test]
    fn toggle_suppresses_release() {
        let t = ValueTransform::Toggle;
        // Press: should toggle
        assert_eq!(t.apply(127, 0.0), Some(1.0));
        // Release: should return None (no action)
        assert_eq!(t.apply(0, 1.0), None);
    }

    #[test]
    fn momentary_suppresses_release() {
        let t = ValueTransform::Momentary;
        assert_eq!(t.apply(127, 0.0), Some(1.0));
        assert_eq!(t.apply(0, 1.0), None);
    }

    #[test]
    fn linear_full_range() {
        let t = ValueTransform::Linear;
        assert_eq!(t.apply(0, 0.0), Some(0.0));
        assert_eq!(t.apply(127, 0.0), Some(1.0));
    }

    #[test]
    fn relative_encoder_clockwise() {
        let t = ValueTransform::RelativeEncoder { sensitivity: 0.01 };
        // raw=1 → small positive increment from 0.0
        let v = t.apply(1, 0.0).unwrap();
        assert!((v - 0.01).abs() < 1e-9, "v={v}");
        // raw=5 → larger increment
        let v = t.apply(5, 0.0).unwrap();
        assert!((v - 0.05).abs() < 1e-9, "v={v}");
    }

    #[test]
    fn relative_encoder_counterclockwise() {
        let t = ValueTransform::RelativeEncoder { sensitivity: 0.01 };
        // raw=127 → decrement by 1 step: -(128-127) * 0.01 = -0.01
        let v = t.apply(127, 0.5).unwrap();
        assert!((v - 0.49).abs() < 1e-9, "v={v}");
        // raw=65 → decrement by 63 steps: -(128-65) * 0.01 = -0.63
        let v = t.apply(65, 0.8).unwrap();
        assert!((v - 0.17).abs() < 1e-9, "v={v}");
    }

    #[test]
    fn relative_encoder_accumulates() {
        let t = ValueTransform::RelativeEncoder { sensitivity: 0.01 };
        let mut current = 0.0;
        // 10 clockwise clicks of raw=1
        for _ in 0..10 {
            current = t.apply(1, current).unwrap();
        }
        assert!((current - 0.1).abs() < 1e-9, "current={current}");
    }

    #[test]
    fn relative_encoder_clamps() {
        let t = ValueTransform::RelativeEncoder { sensitivity: 0.5 };
        // Large clockwise from near max → clamp to 1.0
        let v = t.apply(63, 0.9).unwrap();
        assert_eq!(v, 1.0);
        // Large counterclockwise from near min → clamp to 0.0
        let v = t.apply(65, 0.1).unwrap();
        assert_eq!(v, 0.0);
    }

    #[test]
    fn relative_encoder_sensitivity_scaling() {
        let low = ValueTransform::RelativeEncoder { sensitivity: 0.005 };
        let high = ValueTransform::RelativeEncoder { sensitivity: 0.05 };
        let v_low = low.apply(1, 0.0).unwrap();
        let v_high = high.apply(1, 0.0).unwrap();
        assert!((v_high / v_low - 10.0).abs() < 1e-9);
    }
}
