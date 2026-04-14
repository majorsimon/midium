use tauri::State;

use midium_core::config::{config_dir, MappingsConfig};

use crate::state::AppState;

const MAX_IMPORT_SIZE: usize = 1_000_000; // 1 MB

#[tauri::command]
pub async fn get_mappings(
    state: State<'_, AppState>,
) -> Result<Vec<midium_core::types::Mapping>, String> {
    Ok(state.mappings_config.lock().await.mappings.clone())
}

#[tauri::command]
pub async fn save_mapping(
    state: State<'_, AppState>,
    mapping: midium_core::types::Mapping,
) -> Result<(), String> {
    let mut config = state.mappings_config.lock().await;

    let existing = config
        .mappings
        .iter()
        .position(|m| m.control == mapping.control);
    match existing {
        Some(idx) => config.mappings[idx] = mapping,
        None => config.mappings.push(mapping),
    }

    state
        .mapping_engine
        .lock()
        .await
        .load_mappings(config.mappings.clone());

    persist_mappings(&config)
}

#[tauri::command]
pub async fn delete_mapping(
    state: State<'_, AppState>,
    control: midium_core::types::ControlId,
) -> Result<(), String> {
    let mut config = state.mappings_config.lock().await;
    config.mappings.retain(|m| m.control != control);

    state
        .mapping_engine
        .lock()
        .await
        .load_mappings(config.mappings.clone());

    persist_mappings(&config)
}

#[tauri::command]
pub async fn export_mappings(state: State<'_, AppState>) -> Result<String, String> {
    let config = state.mappings_config.lock().await;
    toml::to_string(&*config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_mappings(
    state: State<'_, AppState>,
    content: String,
) -> Result<(), String> {
    if content.len() > MAX_IMPORT_SIZE {
        return Err("Import too large (max 1 MB)".into());
    }
    let new_config: MappingsConfig = toml::from_str(&content).map_err(|e| e.to_string())?;

    let mut config = state.mappings_config.lock().await;
    *config = new_config;

    state
        .mapping_engine
        .lock()
        .await
        .load_mappings(config.mappings.clone());

    persist_mappings(&config)
}

pub(crate) fn persist_mappings(config: &MappingsConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    let content = toml::to_string(config).map_err(|e| e.to_string())?;
    std::fs::write(config_dir().join("mappings.toml"), content).map_err(|e| e.to_string())
}
