use tauri::{
    menu::{IsMenuItem, Menu, MenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use tracing::warn;

use crate::state::AppState;

pub fn build_tray_menu(app: &AppHandle) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let devices = state.audio.list_output_devices().unwrap_or_default();

    let show = MenuItem::with_id(app, "show", "Show Midium", true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", "Toggle Mute", true, None::<&str>)?;

    let current_label = devices
        .iter()
        .find(|d| d.is_default)
        .map(|d| format!("Output: {}", d.name))
        .unwrap_or_else(|| "Output: (none)".to_string());
    let output_status =
        MenuItem::with_id(app, "output_status", &current_label, false, None::<&str>)?;

    let mut device_items: Vec<MenuItem<tauri::Wry>> = Vec::new();
    if devices.is_empty() {
        device_items.push(MenuItem::with_id(
            app,
            "device:none",
            "  (no output devices)",
            false,
            None::<&str>,
        )?);
    } else {
        for dev in &devices {
            let label = if dev.is_default {
                format!("● {}", dev.name)
            } else {
                format!("  {}", dev.name)
            };
            let item = MenuItem::with_id(
                app,
                format!("device:{}", dev.id),
                &label,
                true,
                None::<&str>,
            )?;
            device_items.push(item);
        }
    }

    let device_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = device_items
        .iter()
        .map(|i| i as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    let device_submenu = Submenu::with_items(app, "Output Device", true, &device_refs)?;

    let quit = MenuItem::with_id(app, "quit", "Quit Midium", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &show,
            &mute,
            &output_status,
            &device_submenu,
            &quit,
        ],
    )
    .map_err(|e| e.into())
}

pub fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_tray_menu(app.handle())?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("Midium — MIDI Audio Controller")
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            if id.starts_with("device:") {
                let device_id = id.strip_prefix("device:").unwrap_or("");
                if !device_id.is_empty() && device_id != "none" {
                    let state = app.state::<AppState>();
                    if let Err(e) = state.audio.set_default_output(device_id) {
                        warn!("Failed to switch output device: {e}");
                    }
                }
                if let Ok(menu) = build_tray_menu(app) {
                    if let Some(tray) = app.tray_by_id("main") {
                        let _ = tray.set_menu(Some(menu));
                    }
                }
                return;
            }
            match id {
                "show" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
                "mute" => {
                    let state = app.state::<AppState>();
                    let target = midium_core::types::AudioTarget::SystemMaster;
                    if let Ok(muted) = state.audio.is_muted(&target) {
                        let _ = state.audio.set_mute(&target, !muted);
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(win) = tray.app_handle().get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}
