use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::debug;

/// A device profile describes a specific MIDI controller's layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub name: String,
    /// Hardware vendor name, e.g. "Korg".
    #[serde(default)]
    pub vendor: Option<String>,
    /// Hardware model name, e.g. "nanoKONTROL2".
    #[serde(default)]
    pub model: Option<String>,
    /// Patterns to match against MIDI port names (case-insensitive substring).
    pub match_patterns: Vec<String>,
    #[serde(default)]
    pub controls: Vec<ProfileControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileControl {
    pub label: String,
    pub control_type: ProfileControlType,
    /// Whether this control sends CC, Note, or PitchBend messages.
    /// Defaults to CC (covers most sliders, knobs, and encoders).
    #[serde(default)]
    pub midi_type: MidiControlType,
    pub channel: u8,
    /// CC number, Note number, or 0 for PitchBend.
    pub number: u8,
    #[serde(default)]
    pub min_value: u8,
    #[serde(default = "default_max")]
    pub max_value: u8,
    /// Groups related controls on the same physical channel strip together.
    #[serde(default)]
    pub group: Option<u8>,
    /// Role of this button within its channel group (buttons only).
    #[serde(default)]
    pub button_role: Option<ButtonRole>,
    /// Logical section name for UI grouping, e.g. "Faders", "Knobs", "Transport".
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileControlType {
    Slider,
    Knob,
    Button,
    Encoder,
}

/// Whether this control's MIDI messages are CC, Note, or PitchBend.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiControlType {
    #[default]
    Cc,
    Note,
    PitchBend,
}

/// The functional role of a button within its channel strip group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonRole {
    /// S (solo/assign) button — lit when the strip has an assigned target.
    Solo,
    /// M (mute) button — lit when the strip target is muted.
    Mute,
    /// R (record/active) button — lit when the strip target is producing audio.
    Record,
}

fn default_max() -> u8 {
    127
}

// ---------------------------------------------------------------------------
// Bundled profiles (embedded at compile time)
// ---------------------------------------------------------------------------

/// Returns all device profiles compiled into the binary.
/// The filesystem loader can override these by name.
pub fn bundled_profiles() -> Vec<DeviceProfile> {
    const SOURCES: &[&str] = &[
        include_str!("../../../profiles/korg_nanokontrol2.toml"),
        include_str!("../../../profiles/behringer_xtouch_mini.toml"),
        include_str!("../../../profiles/akai_midimix.toml"),
        include_str!("../../../profiles/arturia_beatstep.toml"),
        include_str!("../../../profiles/generic.toml"),
    ];
    SOURCES
        .iter()
        .filter_map(|src| {
            toml::from_str::<DeviceProfile>(src)
                .map_err(|e| tracing::warn!("Failed to parse bundled profile: {e}"))
                .ok()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Filesystem loader
// ---------------------------------------------------------------------------

/// Load device profiles from a directory of TOML files.
/// Returns only the profiles found in that directory (no bundled ones).
pub fn load_profiles(dir: &Path) -> Vec<DeviceProfile> {
    let mut profiles = Vec::new();
    if !dir.exists() {
        return profiles;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return profiles,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            match std::fs::read_to_string(&path) {
                Ok(content) => match toml::from_str::<DeviceProfile>(&content) {
                    Ok(profile) => {
                        debug!(name = %profile.name, "Loaded device profile from filesystem");
                        profiles.push(profile);
                    }
                    Err(e) => {
                        tracing::warn!(path = %path.display(), "Failed to parse profile: {e}");
                    }
                },
                Err(e) => {
                    tracing::warn!(path = %path.display(), "Failed to read profile: {e}");
                }
            }
        }
    }
    profiles
}

/// Match a MIDI port name against loaded profiles.
pub fn match_profile<'a>(
    port_name: &str,
    profiles: &'a [DeviceProfile],
) -> Option<&'a DeviceProfile> {
    let name_lower = port_name.to_lowercase();
    profiles.iter().find(|p| {
        p.match_patterns
            .iter()
            .any(|pat| name_lower.contains(&pat.to_lowercase()))
    })
}
