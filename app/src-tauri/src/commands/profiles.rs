use tauri::State;

use midium_core::config::config_dir;
use midium_midi::DeviceProfile;

use crate::state::AppState;

const MAX_IMPORT_SIZE: usize = 1_000_000; // 1 MB

#[tauri::command]
pub fn list_profiles(state: State<AppState>) -> Vec<DeviceProfile> {
    (*state.profiles).clone()
}

#[tauri::command]
pub fn export_profile(state: State<AppState>, name: String) -> Result<String, String> {
    let profile = state
        .profiles
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("Profile '{name}' not found"))?;
    toml::to_string(profile).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_profile(content: String) -> Result<String, String> {
    if content.len() > MAX_IMPORT_SIZE {
        return Err("Profile too large (max 1 MB)".into());
    }
    let profile: midium_midi::DeviceProfile =
        toml::from_str(&content).map_err(|e| e.to_string())?;
    let name = profile.name.clone();

    let dir = config_dir().join("profiles");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let filename = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
        + ".toml";

    // Write the re-serialised struct, not the raw input, to ensure
    // on-disk content matches what load_profiles() would produce.
    let canonical = toml::to_string(&profile).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(&filename), canonical).map_err(|e| e.to_string())?;

    Ok(name)
}
