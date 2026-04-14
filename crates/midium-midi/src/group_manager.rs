use std::sync::Arc;

use tracing::{debug, trace, warn};

use midium_core::dispatch::VolumeControl;
use midium_core::event_bus::EventBus;
use midium_core::types::{AppEvent, AudioTarget, FaderGroup, MidiEvent, MidiMessage, ValueTransform};

use crate::profile::{ButtonRole, DeviceProfile, MidiControlType, ProfileControlType};

// ---------------------------------------------------------------------------
// Internal resolved form of a FaderGroup
// ---------------------------------------------------------------------------

struct ControlSpec {
    channel: u8,
    midi_type: MidiControlType,
    number: u8,
}

struct ResolvedGroup {
    /// Fuzzy device pattern from the config.
    device_pattern: String,
    target: AudioTarget,
    transform: ValueTransform,
    /// Current value tracked for stateful transforms (RelativeEncoder).
    fader_state: f64,
    fader: Option<ControlSpec>,
    mute_btn: Option<ControlSpec>,
    solo_btn: Option<ControlSpec>,
    record_btn: Option<ControlSpec>,
}

// ---------------------------------------------------------------------------
// GroupManager
// ---------------------------------------------------------------------------

/// Handles fader-group MIDI events and button LED feedback.
///
/// For each configured `FaderGroup` the manager:
/// - Routes fader movements to `set_volume` on the audio backend.
/// - Routes mute-button presses to `toggle_mute`, then updates all three LEDs.
/// - On device connect, syncs the S/M/R LED states for all matching groups.
pub struct GroupManager {
    groups: Vec<ResolvedGroup>,
    audio: Box<dyn VolumeControl>,
    event_bus: EventBus,
    profiles: Arc<Vec<DeviceProfile>>,
}

impl GroupManager {
    pub fn new(
        fader_groups: Vec<FaderGroup>,
        profiles: Arc<Vec<DeviceProfile>>,
        audio: Box<dyn VolumeControl>,
        event_bus: EventBus,
    ) -> Self {
        let groups = fader_groups
            .into_iter()
            .map(|fg| resolve_group(fg, &profiles))
            .collect();
        Self { groups, audio, event_bus, profiles }
    }

    /// Subscribe to the EventBus and run until shutdown.
    pub async fn run(mut self) {
        let mut rx = self.event_bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(AppEvent::Midi(event)) => self.handle_midi(&event),
                Ok(AppEvent::DeviceConnected { device }) => self.sync_leds(&device),
                Ok(AppEvent::GroupsChanged { groups }) => {
                    debug!(count = groups.len(), "Fader groups reloaded");
                    self.groups = groups
                        .into_iter()
                        .map(|fg| resolve_group(fg, &self.profiles))
                        .collect();
                }
                Ok(AppEvent::Shutdown) | Err(_) => break,
                _ => {}
            }
        }
    }

    fn handle_midi(&mut self, event: &MidiEvent) {
        for group in &mut self.groups {
            if !device_matches(&event.device, &group.device_pattern) {
                continue;
            }

            // Fader movement → set volume
            if let Some(fader) = &group.fader {
                if control_matches(event, fader) {
                    let raw = extract_raw(&event.message);
                    if let Some(value) = group.transform.apply(raw, group.fader_state) {
                        group.fader_state = value;
                        trace!(target = ?group.target, volume = value, "Group fader → set_volume");
                        if let Err(e) = self.audio.set_volume(&group.target, value) {
                            warn!("GroupManager set_volume: {e}");
                        }
                    }
                    return;
                }
            }

            // Mute button press → toggle mute + update LEDs
            if let Some(mute) = &group.mute_btn {
                if control_matches(event, mute) && is_press(&event.message) {
                    match self.audio.is_muted(&group.target) {
                        Ok(muted) => {
                            let new_muted = !muted;
                            debug!(target = ?group.target, muted = new_muted, "Group mute toggled");
                            if let Err(e) = self.audio.set_mute(&group.target, new_muted) {
                                warn!("GroupManager set_mute: {e}");
                            } else {
                                send_group_leds(&self.event_bus, group, new_muted, &event.device);
                            }
                        }
                        Err(e) => warn!("GroupManager is_muted: {e}"),
                    }
                    return;
                }
            }

            // Solo button: consume silently (LED is always on; no action on press).
            if let Some(solo) = &group.solo_btn {
                if control_matches(event, solo) {
                    return;
                }
            }

            // Record button press: for Device targets, set it as the default output.
            if let Some(record) = &group.record_btn {
                if control_matches(event, record) && is_press(&event.message) {
                    if let midium_core::types::AudioTarget::Device { id } = &group.target {
                        debug!(device_id = %id, "Group R button → set_default_output");
                        if let Err(e) = self.audio.set_default_output(id) {
                            warn!("GroupManager set_default_output: {e}");
                        } else {
                            // R is now on (device is default); keep M/S as they are.
                            let muted = self.audio.is_muted(&group.target).unwrap_or(false);
                            send_group_leds(&self.event_bus, group, muted, &event.device);
                        }
                    }
                    return;
                }
            }
        }
    }

    /// Send the current LED state for every group whose device pattern matches
    /// `connected_device`.  Called whenever a MIDI device is connected.
    fn sync_leds(&self, connected_device: &str) {
        for group in &self.groups {
            if !device_matches(connected_device, &group.device_pattern) {
                continue;
            }
            let muted = self.audio.is_muted(&group.target).unwrap_or(false);
            // For Device targets the R LED means "is default output", not "is unmuted".
            let r_on = match &group.target {
                midium_core::types::AudioTarget::Device { id } => {
                    self.audio.is_default_output(id).unwrap_or(false)
                }
                _ => !muted,
            };
            send_group_leds_full(&self.event_bus, group, muted, r_on, connected_device);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `ResolvedGroup` by looking up profile controls for this group number.
fn resolve_group(fg: FaderGroup, profiles: &[DeviceProfile]) -> ResolvedGroup {
    let mut fader = None;
    let mut mute_btn = None;
    let mut solo_btn = None;
    let mut record_btn = None;

    // Match profile via fuzzy substring (same logic as MidiManager).
    if let Some(profile) = crate::profile::match_profile(&fg.device, profiles) {
        for ctrl in profile.controls.iter().filter(|c| c.group == Some(fg.group)) {
            let spec = ControlSpec {
                channel: ctrl.channel,
                midi_type: ctrl.midi_type.clone(),
                number: ctrl.number,
            };
            match ctrl.control_type {
                ProfileControlType::Slider | ProfileControlType::Encoder => {
                    fader = Some(spec);
                }
                ProfileControlType::Button => match &ctrl.button_role {
                    Some(ButtonRole::Mute) => mute_btn = Some(spec),
                    Some(ButtonRole::Solo) => solo_btn = Some(spec),
                    Some(ButtonRole::Record) => record_btn = Some(spec),
                    None => {}
                },
                ProfileControlType::Knob => {
                    // Knobs don't map to fader groups by default; skip.
                }
            }
        }
        if fader.is_none() {
            warn!(
                device = %fg.device,
                group = fg.group,
                profile = %profile.name,
                "Fader group: no slider found in profile group"
            );
        }
    } else {
        warn!(
            device = %fg.device,
            group = fg.group,
            "Fader group: no matching device profile found"
        );
    }

    ResolvedGroup {
        device_pattern: fg.device,
        target: fg.target,
        transform: fg.transform,
        fader_state: 0.0,
        fader,
        mute_btn,
        solo_btn,
        record_btn,
    }
}

/// Publish `SendMidi` events to set S / M / R button LEDs for a group.
/// `r_on` is computed by the caller — it's `!muted` for app/master targets,
/// or `is_default_output` for Device targets.
fn send_group_leds_full(bus: &EventBus, group: &ResolvedGroup, muted: bool, r_on: bool, actual_device: &str) {
    if let Some(solo) = &group.solo_btn {
        send_led(bus, actual_device, solo, true);
    }
    if let Some(mute) = &group.mute_btn {
        send_led(bus, actual_device, mute, muted);
    }
    if let Some(record) = &group.record_btn {
        send_led(bus, actual_device, record, r_on);
    }
}

/// Convenience wrapper for non-device targets where R = !muted.
fn send_group_leds(bus: &EventBus, group: &ResolvedGroup, muted: bool, actual_device: &str) {
    send_group_leds_full(bus, group, muted, !muted, actual_device);
}

/// Build and publish a single `SendMidi` LED message.
///
/// For CC-type controls we send a Control Change; for Note-type controls we
/// send a Note On with velocity 0x7F (on) or 0x00 (off).
fn send_led(bus: &EventBus, device: &str, ctrl: &ControlSpec, on: bool) {
    let value: u8 = if on { 0x7F } else { 0x00 };
    let data = match ctrl.midi_type {
        MidiControlType::Cc => vec![0xB0 | (ctrl.channel & 0x0F), ctrl.number, value],
        MidiControlType::Note => vec![0x90 | (ctrl.channel & 0x0F), ctrl.number, value],
        MidiControlType::PitchBend => return, // PitchBend LEDs not applicable
    };
    bus.publish(AppEvent::SendMidi {
        device: device.to_string(),
        data,
    });
}

/// True if the MIDI event's device name matches the stored pattern (fuzzy substring).
fn device_matches(actual: &str, pattern: &str) -> bool {
    actual.to_lowercase().contains(&pattern.to_lowercase())
        || pattern.to_lowercase().contains(&actual.to_lowercase())
}

/// True if the MIDI event matches a particular control spec.
fn control_matches(event: &MidiEvent, spec: &ControlSpec) -> bool {
    if event.channel != spec.channel {
        return false;
    }
    match (&event.message, &spec.midi_type) {
        (MidiMessage::ControlChange { control, .. }, MidiControlType::Cc) => {
            *control == spec.number
        }
        (MidiMessage::NoteOn { note, .. } | MidiMessage::NoteOff { note, .. }, MidiControlType::Note) => {
            *note == spec.number
        }
        _ => false,
    }
}

/// True if the message represents a button press (not a release).
fn is_press(msg: &MidiMessage) -> bool {
    match msg {
        MidiMessage::ControlChange { value, .. } => *value > 0,
        MidiMessage::NoteOn { velocity, .. } => *velocity > 0,
        MidiMessage::NoteOff { .. } => false,
        MidiMessage::PitchBend { .. } => false,
    }
}

/// Extract the raw 0-127 value from a MIDI message (same as in MappingEngine).
fn extract_raw(msg: &MidiMessage) -> u8 {
    match msg {
        MidiMessage::ControlChange { value, .. } => *value,
        MidiMessage::NoteOn { velocity, .. } => *velocity,
        MidiMessage::NoteOff { .. } => 0,
        MidiMessage::PitchBend { value } => (*value >> 7) as u8,
    }
}
