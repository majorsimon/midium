mod commands;
mod state;
mod tray;

use std::sync::Arc;

use tauri::{Emitter, Manager};
use tokio::sync::Mutex;
use tracing::info;

use midium_audio::{create_backend, SharedAudio};
use midium_core::{
    config::{config_dir, load_config, load_mappings},
    dispatch::ActionDispatcher,
    event_bus::EventBus,
    mapping::MappingEngine,
    types::{AppEvent, MidiEvent},
};
use midium_midi::{manager::MidiManager, profile::load_profiles, GroupManager};
use midium_plugins::PluginManager;
use midium_shortcuts::ShortcutHandler;

use state::AppState;

pub(crate) fn register_toggle_shortcut(
    app: tauri::AppHandle,
    shortcut_str: &str,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

    let shortcut: Shortcut = shortcut_str
        .parse::<Shortcut>()
        .map_err(|e| e.to_string())?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app_handle, _scut, event| {
            if event.state == ShortcutState::Pressed {
                if let Some(win) = app_handle.get_webview_window("main") {
                    if win.is_visible().unwrap_or(false) {
                        let _ = win.hide();
                    } else {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            }
        })
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".parse().unwrap()),
                )
                .init();

            let app_handle = app.handle().clone();

            let app_config = load_config().unwrap_or_default();
            let mut mappings_config = load_mappings().unwrap_or_default();

            let profiles = {
                let mut profiles = midium_midi::profile::bundled_profiles();

                let mut fs_dirs = vec![
                    std::path::PathBuf::from("profiles"),
                    config_dir().join("profiles"),
                ];
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(parent) = exe.parent() {
                        fs_dirs.push(parent.join("profiles"));
                    }
                }
                for p in fs_dirs.iter().flat_map(|d| load_profiles(d)) {
                    if let Some(pos) = profiles.iter().position(|b| b.name == p.name) {
                        profiles[pos] = p;
                    } else {
                        profiles.push(p);
                    }
                }
                profiles
            };

            let event_bus = EventBus::new();
            let audio_box = create_backend().expect("Failed to initialize audio backend");
            let audio: Arc<dyn midium_audio::backend::AudioBackend> = Arc::from(audio_box);
            audio.register_event_bus(event_bus.clone());

            let dispatcher = Arc::new(
                ActionDispatcher::with_shortcuts(
                    Box::new(SharedAudio(audio.clone())),
                    Box::new(ShortcutHandler::new()),
                )
                .with_device_lister(Box::new(SharedAudio(audio.clone())))
                .with_event_bus(event_bus.clone()),
            );

            let mapping_engine = Arc::new(Mutex::new(MappingEngine::new(event_bus.clone())));
            mapping_engine
                .blocking_lock()
                .load_mappings(mappings_config.mappings.clone());

            let midi_learn_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<MidiEvent>>>> =
                Arc::new(Mutex::new(None));

            let plugin_dirs: Vec<std::path::PathBuf> = {
                let mut dirs = vec![
                    std::path::PathBuf::from("plugins"),
                    std::path::PathBuf::from("../../plugins"),
                ];
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(parent) = exe.parent() {
                        dirs.push(parent.join("plugins"));
                    }
                }
                dirs.push(config_dir().join("plugins"));
                dirs
            };

            let plugin_list =
                PluginManager::spawn(plugin_dirs, audio.clone(), event_bus.clone());

            let profiles_arc = Arc::new(profiles);
            mappings_config.fader_groups.sort_by_key(|g| g.group);
            let initial_fader_groups = mappings_config.fader_groups.clone();

            app.manage(AppState {
                event_bus: event_bus.clone(),
                audio: audio.clone(),
                mapping_engine: mapping_engine.clone(),
                dispatcher: dispatcher.clone(),
                mappings_config: Arc::new(Mutex::new(mappings_config)),
                app_config: Arc::new(Mutex::new(app_config.clone())),
                current_shortcut: Arc::new(Mutex::new(app_config.general.shortcut.clone())),
                midi_learn_tx: midi_learn_tx.clone(),
                plugin_list: Arc::new(Mutex::new(plugin_list)),
                profiles: profiles_arc.clone(),
            });

            if let Some(ref shortcut_str) = app_config.general.shortcut {
                if let Err(e) = register_toggle_shortcut(app.handle().clone(), shortcut_str) {
                    tracing::warn!("Failed to register global shortcut: {e}");
                }
            }

            {
                use tauri_plugin_autostart::ManagerExt;
                let autostart_manager = app.autolaunch();
                if app_config.general.autostart {
                    let _ = autostart_manager.enable();
                } else {
                    let _ = autostart_manager.disable();
                }
            }

            tray::setup_tray(app)?;

            let win = app.get_webview_window("main").unwrap();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = app_handle.get_webview_window("main").map(|w| w.hide());
                }
            });

            // Spawn MIDI manager
            let bus_midi = event_bus.clone();
            let poll = app_config.midi.poll_interval_secs;
            let profiles_for_midi = (*profiles_arc).clone();
            tauri::async_runtime::spawn(async move {
                MidiManager::new(bus_midi, poll, profiles_for_midi)
                    .run()
                    .await;
            });

            // Spawn GroupManager
            {
                let group_audio = Box::new(SharedAudio(audio.clone()));
                let group_manager = GroupManager::new(
                    initial_fader_groups,
                    profiles_arc.clone(),
                    group_audio,
                    event_bus.clone(),
                );
                tauri::async_runtime::spawn(async move {
                    group_manager.run().await;
                });
            }

            // EventBus -> Tauri event bridge
            let bus = event_bus.clone();
            let app_handle3 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut rx = bus.subscribe();
                loop {
                    match rx.recv().await {
                        Ok(AppEvent::Midi(midi_event)) => {
                            mapping_engine.lock().await.process_midi_event(&midi_event);

                            if let Some(tx) = midi_learn_tx.lock().await.take() {
                                let _ = tx.send(midi_event.clone());
                            }

                            let _ = app_handle3.emit("midi-event", &midi_event);
                        }
                        Ok(AppEvent::ActionTriggered { action, value }) => {
                            dispatcher.dispatch(&action, value);
                        }
                        Ok(AppEvent::VolumeChanged { target, volume }) => {
                            let _ = app_handle3.emit(
                                "volume-changed",
                                serde_json::json!({ "target": target, "volume": volume }),
                            );
                        }
                        Ok(AppEvent::MuteChanged { target, muted }) => {
                            let _ = app_handle3.emit(
                                "mute-changed",
                                serde_json::json!({ "target": target, "muted": muted }),
                            );
                        }
                        Ok(AppEvent::DeviceConnected { device }) => {
                            info!(%device, "MIDI device connected");
                            let _ = app_handle3.emit("device-connected", &device);
                        }
                        Ok(AppEvent::DeviceDisconnected { device }) => {
                            let _ = app_handle3.emit("device-disconnected", &device);
                        }
                        Ok(AppEvent::DefaultDeviceChanged) => {
                            let _ = app_handle3.emit("default-device-changed", ());
                            if let Ok(menu) = tray::build_tray_menu(&app_handle3) {
                                if let Some(tray) = app_handle3.tray_by_id("main") {
                                    let _ = tray.set_menu(Some(menu));
                                }
                            }
                        }
                        Ok(AppEvent::Shutdown) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            });

            info!("Midium app setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::audio::get_capabilities,
            commands::audio::list_output_devices,
            commands::audio::list_input_devices,
            commands::audio::list_sessions,
            commands::audio::get_volume,
            commands::audio::get_muted,
            commands::audio::set_volume,
            commands::audio::toggle_mute,
            commands::audio::set_default_output,
            commands::audio::set_default_input,
            commands::midi::list_midi_ports,
            commands::mappings::get_mappings,
            commands::mappings::save_mapping,
            commands::mappings::delete_mapping,
            commands::midi::start_midi_learn,
            commands::midi::cancel_midi_learn,
            commands::config::get_config,
            commands::config::save_config,
            commands::config::get_shortcut,
            commands::config::set_shortcut,
            commands::config::get_autostart,
            commands::config::set_autostart,
            commands::plugins::list_plugins,
            commands::profiles::list_profiles,
            commands::midi::send_midi,
            commands::mappings::export_mappings,
            commands::mappings::import_mappings,
            commands::profiles::export_profile,
            commands::profiles::import_profile,
            commands::fader_groups::get_fader_groups,
            commands::fader_groups::save_fader_group,
            commands::fader_groups::delete_fader_group,
        ])
        .run(tauri::generate_context!())
        .expect("error running midium");
}
