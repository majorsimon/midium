use tauri::State;

use midium_plugins::PluginInfo;

use crate::state::AppState;

#[tauri::command]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    Ok(state.plugin_list.lock().await.clone())
}
