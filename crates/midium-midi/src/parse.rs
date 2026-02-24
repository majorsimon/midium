use midium_core::types::{MidiEvent, MidiMessage};

/// Parse raw MIDI bytes into a typed MidiEvent.
///
/// Returns `None` for SysEx, Clock, Active Sensing, and other messages we
/// don't handle.
pub fn parse_midi(device: &str, bytes: &[u8]) -> Option<MidiEvent> {
    if bytes.is_empty() {
        return None;
    }

    let status = bytes[0];
    let msg_type = status & 0xF0;
    let channel = status & 0x0F;

    let message = match msg_type {
        // Note Off
        0x80 if bytes.len() >= 3 => Some(MidiMessage::NoteOff {
            note: bytes[1],
            velocity: bytes[2],
        }),
        // Note On
        0x90 if bytes.len() >= 3 => {
            // Note On with velocity 0 is treated as Note Off by convention
            if bytes[2] == 0 {
                Some(MidiMessage::NoteOff {
                    note: bytes[1],
                    velocity: 0,
                })
            } else {
                Some(MidiMessage::NoteOn {
                    note: bytes[1],
                    velocity: bytes[2],
                })
            }
        }
        // Control Change
        0xB0 if bytes.len() >= 3 => Some(MidiMessage::ControlChange {
            control: bytes[1],
            value: bytes[2],
        }),
        // Pitch Bend
        0xE0 if bytes.len() >= 3 => {
            let value = (bytes[2] as u16) << 7 | (bytes[1] as u16);
            Some(MidiMessage::PitchBend { value })
        }
        // Ignore everything else (SysEx, Program Change, Aftertouch, etc.)
        _ => None,
    };

    message.map(|m| MidiEvent {
        device: device.to_string(),
        channel,
        message: m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cc() {
        let event = parse_midi("test", &[0xB0, 7, 100]).unwrap();
        assert_eq!(event.channel, 0);
        assert_eq!(
            event.message,
            MidiMessage::ControlChange {
                control: 7,
                value: 100
            }
        );
    }

    #[test]
    fn parse_note_on() {
        let event = parse_midi("test", &[0x91, 60, 127]).unwrap();
        assert_eq!(event.channel, 1);
        assert_eq!(
            event.message,
            MidiMessage::NoteOn {
                note: 60,
                velocity: 127
            }
        );
    }

    #[test]
    fn parse_note_on_zero_velocity_is_off() {
        let event = parse_midi("test", &[0x90, 60, 0]).unwrap();
        assert_eq!(
            event.message,
            MidiMessage::NoteOff {
                note: 60,
                velocity: 0
            }
        );
    }

    #[test]
    fn parse_note_off() {
        let event = parse_midi("test", &[0x80, 60, 64]).unwrap();
        assert_eq!(
            event.message,
            MidiMessage::NoteOff {
                note: 60,
                velocity: 64
            }
        );
    }

    #[test]
    fn parse_pitch_bend() {
        let event = parse_midi("test", &[0xE0, 0x00, 0x40]).unwrap();
        assert_eq!(event.message, MidiMessage::PitchBend { value: 8192 }); // center
    }

    #[test]
    fn parse_unknown_ignored() {
        assert!(parse_midi("test", &[0xF0, 0x7E, 0x7F]).is_none()); // SysEx
        assert!(parse_midi("test", &[0xF8]).is_none()); // Clock
        assert!(parse_midi("test", &[]).is_none()); // Empty
    }
}
