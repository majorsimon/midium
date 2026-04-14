use tauri::State;

use midium_core::types::{AppEvent, FaderGroup};

use crate::state::AppState;
use super::mappings::persist_mappings;

#[tauri::command]
pub async fn get_fader_groups(state: State<'_, AppState>) -> Result<Vec<FaderGroup>, String> {
    Ok(state.mappings_config.lock().await.fader_groups.clone())
}

#[tauri::command]
pub async fn save_fader_group(
    state: State<'_, AppState>,
    group: FaderGroup,
) -> Result<(), String> {
    let mut config = state.mappings_config.lock().await;
    let existing = config
        .fader_groups
        .iter()
        .position(|g| g.device == group.device && g.group == group.group);
    match existing {
        Some(idx) => config.fader_groups[idx] = group,
        None => {
            let insert_at = config
                .fader_groups
                .partition_point(|g| g.group <= group.group);
            config.fader_groups.insert(insert_at, group);
        }
    }
    state
        .event_bus
        .publish(AppEvent::GroupsChanged {
            groups: config.fader_groups.clone(),
        });
    persist_mappings(&config)
}

#[tauri::command]
pub async fn delete_fader_group(
    state: State<'_, AppState>,
    device: String,
    group: u8,
) -> Result<(), String> {
    let mut config = state.mappings_config.lock().await;
    config
        .fader_groups
        .retain(|g| !(g.device == device && g.group == group));
    state
        .event_bus
        .publish(AppEvent::GroupsChanged {
            groups: config.fader_groups.clone(),
        });
    persist_mappings(&config)
}
