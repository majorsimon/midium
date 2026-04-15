use serde::Serialize;
use tauri::State;

use midium_core::config::config_dir;
use midium_core::types::DeviceProfile;

use crate::state::AppState;

const MAX_IMPORT_SIZE: usize = 1_000_000; // 1 MB

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
        + ".toml"
}

fn user_profiles_dir() -> std::path::PathBuf {
    config_dir().join("profiles")
}

fn validate_profile(profile: &DeviceProfile) -> Result<(), String> {
    if profile.name.trim().is_empty() {
        return Err("Profile name must not be empty".into());
    }
    for (i, c) in profile.controls.iter().enumerate() {
        if c.label.trim().is_empty() {
            return Err(format!("Control #{} has an empty label", i + 1));
        }
        if c.channel > 15 {
            return Err(format!(
                "Control '{}': channel must be 0-15 (got {})",
                c.label, c.channel
            ));
        }
        if c.number > 127 {
            return Err(format!(
                "Control '{}': number must be 0-127 (got {})",
                c.label, c.number
            ));
        }
        if c.min_value > c.max_value {
            return Err(format!(
                "Control '{}': min_value ({}) must be <= max_value ({})",
                c.label, c.min_value, c.max_value
            ));
        }
        if c.max_value > 127 {
            return Err(format!(
                "Control '{}': max_value must be <= 127 (got {})",
                c.label, c.max_value
            ));
        }
    }
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct ProfileMeta {
    pub name: String,
    pub is_user: bool,
    pub control_count: usize,
}

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<DeviceProfile>, String> {
    Ok(state.profiles.read().await.clone())
}

#[tauri::command]
pub async fn list_profile_meta(state: State<'_, AppState>) -> Result<Vec<ProfileMeta>, String> {
    let profiles = state.profiles.read().await;
    let dir = user_profiles_dir();
    let user_files: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|content| toml::from_str::<DeviceProfile>(&content).ok())
                    .map(|p| p.name)
            } else {
                None
            }
        })
        .collect();
    Ok(profiles
        .iter()
        .map(|p| ProfileMeta {
            name: p.name.clone(),
            is_user: user_files.contains(&p.name),
            control_count: p.controls.len(),
        })
        .collect())
}

#[tauri::command]
pub async fn get_profile(
    state: State<'_, AppState>,
    name: String,
) -> Result<DeviceProfile, String> {
    let profiles = state.profiles.read().await;
    profiles
        .iter()
        .find(|p| p.name == name)
        .cloned()
        .ok_or_else(|| format!("Profile '{name}' not found"))
}

#[tauri::command]
pub async fn export_profile(state: State<'_, AppState>, name: String) -> Result<String, String> {
    let profiles = state.profiles.read().await;
    let profile = profiles
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
    let profile: DeviceProfile = toml::from_str(&content).map_err(|e| e.to_string())?;
    let name = profile.name.clone();

    let dir = user_profiles_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let filename = sanitize_filename(&name);
    let canonical = toml::to_string(&profile).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(&filename), canonical).map_err(|e| e.to_string())?;

    Ok(name)
}

#[tauri::command]
pub fn save_profile(profile: DeviceProfile) -> Result<(), String> {
    validate_profile(&profile)?;

    let dir = user_profiles_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let filename = sanitize_filename(&profile.name);
    let canonical = toml::to_string(&profile).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(&filename), canonical).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn delete_profile(name: String) -> Result<(), String> {
    let dir = user_profiles_dir();
    let filename = sanitize_filename(&name);
    let path = dir.join(&filename);
    if !path.exists() {
        return Err(format!(
            "Cannot delete '{}': no user profile file on disk (bundled profiles cannot be deleted)",
            name
        ));
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn duplicate_profile(
    state: State<'_, AppState>,
    name: String,
    new_name: String,
) -> Result<(), String> {
    if new_name.trim().is_empty() {
        return Err("New profile name must not be empty".into());
    }
    let profiles = state.profiles.read().await;
    if profiles.iter().any(|p| p.name == new_name) {
        return Err(format!("A profile named '{new_name}' already exists"));
    }
    let source = profiles
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("Profile '{name}' not found"))?;

    let mut copy = source.clone();
    copy.name = new_name;

    drop(profiles);
    save_profile(copy)
}
