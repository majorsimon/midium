use std::sync::{Arc, Mutex};

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State,
};
use tokio::sync::oneshot;
use tracing::info;

use midium_audio::{backend::AudioBackend, create_backend};
use midium_core::{
    config::{config_dir, load_config, load_mappings, AppConfig, MappingsConfig},
    dispatch::{ActionDispatcher, VolumeControl},
    event_bus::EventBus,
    mapping::MappingEngine,
    types::{AppEvent, AudioTarget, MidiEvent},
};
use midium_midi::{manager::MidiManager, profile::load_profiles};
use midium_plugins::{PluginInfo, PluginManager};

// ---------------------------------------------------------------------------
// Shared audio adapter
// Wraps Arc<dyn AudioBackend> as VolumeControl so ActionDispatcher and
// AppState can hold the same backend instance.
// ---------------------------------------------------------------------------
struct SharedAudio(Arc<dyn AudioBackend>);

impl VolumeControl for SharedAudio {
    fn set_volume(&self, t: &AudioTarget, v: f64) -> anyhow::Result<()> {
        self.0.set_volume(t, v)
    }
    fn set_mute(&self, t: &AudioTarget, m: bool) -> anyhow::Result<()> {
        self.0.set_mute(t, m)
    }
    fn is_muted(&self, t: &AudioTarget) -> anyhow::Result<bool> {
        self.0.is_muted(t)
    }
    fn set_default_output(&self, id: &str) -> anyhow::Result<()> {
        self.0.set_default_output(id)
    }
    fn set_default_input(&self, id: &str) -> anyhow::Result<()> {
        self.0.set_default_input(id)
    }
}

// ---------------------------------------------------------------------------
// Managed state
// ---------------------------------------------------------------------------
pub struct AppState {
    pub event_bus: EventBus,
    pub audio: Arc<dyn AudioBackend>,
    pub mapping_engine: Arc<Mutex<MappingEngine>>,
    pub dispatcher: Arc<ActionDispatcher>,
    pub mappings_config: Arc<Mutex<MappingsConfig>>,
    pub app_config: Arc<Mutex<AppConfig>>,
    /// When Some, the next MIDI event is forwarded for MIDI Learn.
    pub midi_learn_tx: Arc<Mutex<Option<oneshot::Sender<MidiEvent>>>>,
    /// Snapshot of loaded plugin info (populated at startup).
    pub plugin_list: Arc<Mutex<Vec<PluginInfo>>>,
}

// ---------------------------------------------------------------------------
// IPC Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_capabilities(state: State<AppState>) -> serde_json::Value {
    let caps = state.audio.capabilities();
    serde_json::json!({
        "per_app_volume": caps.per_app_volume,
        "device_switching": caps.device_switching,
        "input_device_switching": caps.input_device_switching,
    })
}

#[tauri::command]
fn list_output_devices(
    state: State<AppState>,
) -> Result<Vec<midium_core::types::AudioDeviceInfo>, String> {
    state.audio.list_output_devices().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_sessions(
    state: State<AppState>,
) -> Result<Vec<midium_core::types::AudioSessionInfo>, String> {
    state.audio.list_sessions().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_volume(state: State<AppState>, target: AudioTarget) -> Result<f64, String> {
    state.audio.get_volume(&target).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_volume(state: State<AppState>, target: AudioTarget, volume: f64) -> Result<(), String> {
    state
        .dispatcher
        .dispatch(&midium_core::types::Action::SetVolume { target }, volume);
    Ok(())
}

#[tauri::command]
fn toggle_mute(state: State<AppState>, target: AudioTarget) -> Result<(), String> {
    state
        .dispatcher
        .dispatch(&midium_core::types::Action::ToggleMute { target }, 1.0);
    Ok(())
}

#[tauri::command]
fn set_default_output(state: State<AppState>, device_id: String) -> Result<(), String> {
    state
        .audio
        .set_default_output(&device_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_midi_ports() -> Vec<String> {
    MidiManager::list_ports()
}

#[tauri::command]
fn get_mappings(state: State<AppState>) -> Vec<midium_core::types::Mapping> {
    state.mappings_config.lock().unwrap().mappings.clone()
}

#[tauri::command]
fn save_mapping(
    state: State<AppState>,
    mapping: midium_core::types::Mapping,
) -> Result<(), String> {
    let mut config = state.mappings_config.lock().unwrap();

    let existing = config.mappings.iter().position(|m| m.control == mapping.control);
    match existing {
        Some(idx) => config.mappings[idx] = mapping,
        None => config.mappings.push(mapping),
    }

    state
        .mapping_engine
        .lock()
        .unwrap()
        .load_mappings(config.mappings.clone());

    persist_mappings(&config)
}

#[tauri::command]
fn delete_mapping(
    state: State<AppState>,
    control: midium_core::types::ControlId,
) -> Result<(), String> {
    let mut config = state.mappings_config.lock().unwrap();
    config.mappings.retain(|m| m.control != control);

    state
        .mapping_engine
        .lock()
        .unwrap()
        .load_mappings(config.mappings.clone());

    persist_mappings(&config)
}

#[tauri::command]
fn list_plugins(state: State<AppState>) -> Vec<PluginInfo> {
    state.plugin_list.lock().unwrap().clone()
}

fn persist_mappings(config: &MappingsConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    let content = toml::to_string(config).map_err(|e| e.to_string())?;
    std::fs::write(config_dir().join("mappings.toml"), content)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn start_midi_learn(state: State<AppState>, app_handle: AppHandle) -> Result<(), String> {
    let (tx, rx) = oneshot::channel::<MidiEvent>();
    *state.midi_learn_tx.lock().unwrap() = Some(tx);
    info!("MIDI Learn activated");

    tauri::async_runtime::spawn(async move {
        if let Ok(event) = rx.await {
            info!(device = %event.device, "MIDI Learn captured event");
            let _ = app_handle.emit("midi-learn-result", &event);
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_midi_learn(state: State<AppState>) {
    *state.midi_learn_tx.lock().unwrap() = None;
}

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.app_config.lock().unwrap().clone()
}

#[tauri::command]
fn save_config(state: State<AppState>, config: AppConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    let content = toml::to_string(&config).map_err(|e| e.to_string())?;
    std::fs::write(config_dir().join("config.toml"), content)
        .map_err(|e| e.to_string())?;
    *state.app_config.lock().unwrap() = config;
    Ok(())
}

// ---------------------------------------------------------------------------
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".parse().unwrap()),
                )
                .init();

            let app_handle = app.handle().clone();

            let app_config = load_config().unwrap_or_default();
            let mappings_config = load_mappings().unwrap_or_default();

            let profiles = {
                let from_cwd = load_profiles(&std::path::PathBuf::from("profiles"));
                if !from_cwd.is_empty() {
                    from_cwd
                } else if let Ok(exe_dir) = std::env::current_exe()
                    .map(|p| p.parent().unwrap_or(&p.clone()).to_path_buf())
                {
                    load_profiles(&exe_dir.join("profiles"))
                } else {
                    vec![]
                }
            };

            let event_bus = EventBus::new();
            let audio_box = create_backend().expect("Failed to initialize audio backend");
            let audio: Arc<dyn AudioBackend> = Arc::from(audio_box);

            let dispatcher = Arc::new(ActionDispatcher::new(Box::new(SharedAudio(audio.clone()))));

            let mapping_engine = Arc::new(Mutex::new(MappingEngine::new(event_bus.clone())));
            mapping_engine
                .lock()
                .unwrap()
                .load_mappings(mappings_config.mappings.clone());

            let midi_learn_tx: Arc<Mutex<Option<oneshot::Sender<MidiEvent>>>> =
                Arc::new(Mutex::new(None));

            // Load plugins from: ./plugins/ → ../../plugins/ → exe_dir/plugins/ → config_dir/plugins/
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

            // Spawn plugin system on its own thread (mlua::Lua is !Send).
            // Returns plugin info after on_load completes.
            let plugin_list =
                PluginManager::spawn(plugin_dirs, audio.clone(), event_bus.clone());

            app.manage(AppState {
                event_bus: event_bus.clone(),
                audio: audio.clone(),
                mapping_engine: mapping_engine.clone(),
                dispatcher: dispatcher.clone(),
                mappings_config: Arc::new(Mutex::new(mappings_config)),
                app_config: Arc::new(Mutex::new(app_config.clone())),
                midi_learn_tx: midi_learn_tx.clone(),
                plugin_list: Arc::new(Mutex::new(plugin_list)),
            });

            setup_tray(app)?;

            // Minimise to tray on close
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
            tauri::async_runtime::spawn(async move {
                MidiManager::new(bus_midi, poll, profiles).run().await;
            });

            // EventBus → Tauri event bridge
            let bus = event_bus.clone();
            let app_handle3 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut rx = bus.subscribe();
                loop {
                    match rx.recv().await {
                        Ok(AppEvent::Midi(midi_event)) => {
                            mapping_engine.lock().unwrap().process_midi_event(&midi_event);

                            if let Some(tx) = midi_learn_tx.lock().unwrap().take() {
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
                        Ok(AppEvent::DeviceConnected { device }) => {
                            info!(%device, "MIDI device connected");
                            let _ = app_handle3.emit("device-connected", &device);
                        }
                        Ok(AppEvent::DeviceDisconnected { device }) => {
                            let _ = app_handle3.emit("device-disconnected", &device);
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
            get_capabilities,
            list_output_devices,
            list_sessions,
            get_volume,
            set_volume,
            toggle_mute,
            set_default_output,
            list_midi_ports,
            get_mappings,
            save_mapping,
            delete_mapping,
            start_midi_learn,
            cancel_midi_learn,
            get_config,
            save_config,
            list_plugins,
        ])
        .run(tauri::generate_context!())
        .expect("error running midium");
}

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show Midium", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Midium", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("Midium — MIDI Audio Controller")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
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
