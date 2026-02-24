use std::collections::HashMap;
use tracing::{debug, trace};

use crate::types::{
    Action, AppEvent, ControlId, Mapping, MidiEvent, MidiMessage, ValueTransform,
};
use crate::event_bus::EventBus;

/// Resolves MIDI events to Actions using the configured mappings.
pub struct MappingEngine {
    /// ControlId → (Action, Transform) — exact key is the mapping's control field.
    mappings: HashMap<ControlId, (Action, ValueTransform)>,
    /// Current value state keyed by the *event's* ControlId (exact port name).
    state: HashMap<ControlId, f64>,
    event_bus: EventBus,
}

impl MappingEngine {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            mappings: HashMap::new(),
            state: HashMap::new(),
            event_bus,
        }
    }

    /// Load mappings from a list (typically deserialized from TOML).
    pub fn load_mappings(&mut self, mappings: Vec<Mapping>) {
        self.mappings.clear();
        for m in mappings {
            debug!(control = ?m.control, action = ?m.action, "Loaded mapping");
            self.mappings.insert(m.control, (m.action, m.transform));
        }
    }

    /// Process a MIDI event: look up mapping, apply transform, dispatch action.
    pub fn process_midi_event(&mut self, event: &MidiEvent) {
        let control_id = ControlId::from_event(event);
        let raw_value = extract_value(&event.message);

        // Two-phase lookup: exact first, then fuzzy substring match on device name.
        // This lets mappings use "nanoKONTROL2" to match "nanoKONTROL2 MIDI 1".
        let resolved = self.mappings.get(&control_id).cloned().or_else(|| {
            self.mappings
                .iter()
                .find(|(k, _)| {
                    k.channel == control_id.channel
                        && k.control_type == control_id.control_type
                        && control_id
                            .device
                            .to_lowercase()
                            .contains(&k.device.to_lowercase())
                })
                .map(|(_, v)| v.clone())
        });

        if let Some((action, transform)) = resolved {
            let current = *self.state.get(&control_id).unwrap_or(&0.0);

            if let Some(value) = transform.apply(raw_value, current) {
                self.state.insert(control_id.clone(), value);

                trace!(
                    ?control_id,
                    raw_value,
                    transformed = value,
                    ?action,
                    "Mapping resolved"
                );

                self.event_bus.publish(AppEvent::ActionTriggered {
                    action,
                    value,
                });
            } else {
                trace!(?control_id, raw_value, "Transform suppressed action (button release)");
            }
        } else {
            trace!(?control_id, raw_value, "No mapping for control");
        }
    }
}

/// Extract the raw value byte from a MIDI message.
fn extract_value(msg: &MidiMessage) -> u8 {
    match msg {
        MidiMessage::ControlChange { value, .. } => *value,
        MidiMessage::NoteOn { velocity, .. } => *velocity,
        MidiMessage::NoteOff { .. } => 0,
        MidiMessage::PitchBend { value } => {
            // Map 14-bit (0-16383) to 7-bit (0-127)
            (*value >> 7) as u8
        }
    }
}
