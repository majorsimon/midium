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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileControlType {
    Slider,
    Knob,
    Button,
    Encoder,
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
