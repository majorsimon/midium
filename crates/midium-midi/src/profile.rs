use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::debug;

/// A device profile describes a specific MIDI controller's layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub name: String,
    /// Patterns to match against MIDI port names (case-insensitive substring).
    pub match_patterns: Vec<String>,
    #[serde(default)]
    pub controls: Vec<ProfileControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileControl {
    pub label: String,
    pub control_type: ProfileControlType,
    pub channel: u8,
    /// CC number or Note number.
    pub number: u8,
    #[serde(default)]
    pub min_value: u8,
    #[serde(default = "default_max")]
    pub max_value: u8,
    /// Groups related controls on the same physical channel strip together.
    /// Controls with the same group index belong to the same channel.
    #[serde(default)]
    pub group: Option<u8>,
    /// Role of this button within its channel group (buttons only).
    #[serde(default)]
    pub button_role: Option<ButtonRole>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileControlType {
    Slider,
    Knob,
    Button,
    Encoder,
}

/// The functional role of a button within its channel strip group.
/// Used to route LED feedback to the correct button.
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

/// Load all device profiles from a directory of TOML files.
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
                        debug!(name = %profile.name, "Loaded device profile");
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
pub fn match_profile<'a>(port_name: &str, profiles: &'a [DeviceProfile]) -> Option<&'a DeviceProfile> {
    let name_lower = port_name.to_lowercase();
    profiles.iter().find(|p| {
        p.match_patterns
            .iter()
            .any(|pat| name_lower.contains(&pat.to_lowercase()))
    })
}
