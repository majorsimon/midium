use std::path::Path;
use tracing::debug;

pub use midium_core::types::{
    ButtonRole, DeviceProfile, MidiControlType, ProfileControl, ProfileControlType,
};

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

/// Merge filesystem profiles with bundled profiles (filesystem overrides by name).
pub fn merge_profiles(
    bundled: Vec<DeviceProfile>,
    fs_dirs: &[std::path::PathBuf],
) -> Vec<DeviceProfile> {
    let mut profiles = bundled;
    for p in fs_dirs.iter().flat_map(|d| load_profiles(d)) {
        if let Some(pos) = profiles.iter().position(|b| b.name == p.name) {
            profiles[pos] = p;
        } else {
            profiles.push(p);
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
