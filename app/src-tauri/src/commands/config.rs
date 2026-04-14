use tauri::State;

use midium_core::config::{config_dir, AppConfig};

use crate::state::AppState;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.app_config.lock().await.clone())
}

#[tauri::command]
pub async fn save_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    let content = toml::to_string(&config).map_err(|e| e.to_string())?;
    std::fs::write(config_dir().join("config.toml"), content).map_err(|e| e.to_string())?;
    *state.app_config.lock().await = config;
    Ok(())
}

#[tauri::command]
pub async fn get_shortcut(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.current_shortcut.lock().await.clone())
}

#[tauri::command]
pub async fn set_shortcut(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    shortcut: Option<String>,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

    let old_shortcut = {
        let mut guard = state.current_shortcut.lock().await;
        let old = guard.clone();
        *guard = shortcut.clone();
        old
    };

    if let Some(ref old_str) = old_shortcut {
        if let Ok(old_parsed) = old_str.parse::<Shortcut>() {
            let _ = app.global_shortcut().unregister(old_parsed);
        }
    }

    if let Some(ref new_str) = shortcut {
        if let Err(e) = crate::register_toggle_shortcut(app.clone(), new_str) {
            if let Some(ref restore) = old_shortcut {
                let _ = crate::register_toggle_shortcut(app.clone(), restore);
            }
            *state.current_shortcut.lock().await = old_shortcut;
            return Err(e);
        }
    }

    let mut config = state.app_config.lock().await;
    config.general.shortcut = shortcut;
    let content = toml::to_string(&*config).map_err(|e| e.to_string())?;
    drop(config);
    std::fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    std::fs::write(config_dir().join("config.toml"), content).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())
    } else {
        manager.disable().map_err(|e| e.to_string())
    }
}
