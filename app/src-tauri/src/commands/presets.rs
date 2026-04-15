use tauri::{Emitter, State};

use midium_core::config::{presets_dir, MappingsConfig, PresetMeta};
use midium_core::types::AppEvent;

use crate::state::AppState;
use super::mappings::persist_mappings;

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect::<String>()
        .trim()
        .to_string()
}

#[tauri::command]
pub fn list_presets() -> Result<Vec<String>, String> {
    let dir = presets_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    Ok(names)
}

#[tauri::command]
pub async fn save_preset(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let safe_name = sanitize_name(&name);
    if safe_name.is_empty() {
        return Err("Preset name cannot be empty".into());
    }

    let dir = presets_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let config = state.mappings_config.lock().await;
    let content = toml::to_string(&*config).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{safe_name}.toml")), content).map_err(|e| e.to_string())?;

    let mut meta = PresetMeta::load();
    meta.active_preset = Some(safe_name);
    meta.save()
}

#[tauri::command]
pub async fn load_preset(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let path = presets_dir().join(format!("{name}.toml"));
    if !path.exists() {
        return Err(format!("Preset '{name}' not found"));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let new_config: MappingsConfig = toml::from_str(&content).map_err(|e| e.to_string())?;

    let mut config = state.mappings_config.lock().await;
    *config = new_config;

    state
        .mapping_engine
        .lock()
        .await
        .load_mappings(config.mappings.clone());

    state
        .event_bus
        .publish(AppEvent::GroupsChanged {
            groups: config.fader_groups.clone(),
        });

    persist_mappings(&config)?;

    let mut meta = PresetMeta::load();
    meta.active_preset = Some(name);
    meta.save()?;

    let _ = app_handle.emit("preset-loaded", ());
    Ok(())
}

#[tauri::command]
pub fn delete_preset(name: String) -> Result<(), String> {
    let path = presets_dir().join(format!("{name}.toml"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }

    let mut meta = PresetMeta::load();
    if meta.active_preset.as_deref() == Some(&name) {
        meta.active_preset = None;
        meta.save()?;
    }
    Ok(())
}

#[tauri::command]
pub fn rename_preset(old: String, new: String) -> Result<(), String> {
    let safe_new = sanitize_name(&new);
    if safe_new.is_empty() {
        return Err("New name cannot be empty".into());
    }

    let dir = presets_dir();
    let old_path = dir.join(format!("{old}.toml"));
    let new_path = dir.join(format!("{safe_new}.toml"));

    if !old_path.exists() {
        return Err(format!("Preset '{old}' not found"));
    }
    if new_path.exists() {
        return Err(format!("Preset '{safe_new}' already exists"));
    }

    std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())?;

    let mut meta = PresetMeta::load();
    if meta.active_preset.as_deref() == Some(&old) {
        meta.active_preset = Some(safe_new);
        meta.save()?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_active_preset() -> Result<Option<String>, String> {
    Ok(PresetMeta::load().active_preset)
}
