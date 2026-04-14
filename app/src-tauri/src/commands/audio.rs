use tauri::State;

use midium_core::types::AudioTarget;

use crate::state::AppState;

#[tauri::command]
pub fn get_capabilities(state: State<AppState>) -> serde_json::Value {
    let caps = state.audio.capabilities();
    serde_json::json!({
        "per_app_volume": caps.per_app_volume,
        "device_switching": caps.device_switching,
        "input_device_switching": caps.input_device_switching,
    })
}

#[tauri::command]
pub fn list_output_devices(
    state: State<AppState>,
) -> Result<Vec<midium_core::types::AudioDeviceInfo>, String> {
    state.audio.list_output_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_input_devices(
    state: State<AppState>,
) -> Result<Vec<midium_core::types::AudioDeviceInfo>, String> {
    state.audio.list_input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_sessions(
    state: State<AppState>,
) -> Result<Vec<midium_core::types::AudioSessionInfo>, String> {
    state.audio.list_sessions().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_volume(state: State<AppState>, target: AudioTarget) -> Result<f64, String> {
    state.audio.get_volume(&target).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_muted(state: State<AppState>, target: AudioTarget) -> Result<bool, String> {
    state.audio.is_muted(&target).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_volume(state: State<AppState>, target: AudioTarget, volume: f64) -> Result<(), String> {
    state
        .dispatcher
        .dispatch(&midium_core::types::Action::SetVolume { target }, volume);
    Ok(())
}

#[tauri::command]
pub fn toggle_mute(state: State<AppState>, target: AudioTarget) -> Result<(), String> {
    state
        .dispatcher
        .dispatch(&midium_core::types::Action::ToggleMute { target }, 1.0);
    Ok(())
}

#[tauri::command]
pub fn set_default_output(state: State<AppState>, device_id: String) -> Result<(), String> {
    state
        .audio
        .set_default_output(&device_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_default_input(state: State<AppState>, device_id: String) -> Result<(), String> {
    state
        .audio
        .set_default_input(&device_id)
        .map_err(|e| e.to_string())
}
