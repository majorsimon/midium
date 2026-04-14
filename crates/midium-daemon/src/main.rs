use std::path::PathBuf;
use std::sync::Arc;

use tokio::signal;
use tracing::{error, info, warn};

use midium_audio::{backend::AudioBackend, create_backend};
use midium_core::config::config_dir;
use midium_core::dispatch::{ActionDispatcher, DeviceLister, VolumeControl};
use midium_core::event_bus::EventBus;
use midium_core::mapping::MappingEngine;
use midium_core::types::{AppEvent, AudioTarget};
use midium_midi::manager::MidiManager;
use midium_midi::profile::load_profiles;
use midium_midi::GroupManager;

/// Thin adapter so `Arc<dyn AudioBackend>` can be shared between dispatcher
/// and GroupManager as a `Box<dyn VolumeControl>`.
struct ArcAudio(Arc<dyn AudioBackend>);

impl VolumeControl for ArcAudio {
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
    fn is_default_output(&self, id: &str) -> anyhow::Result<bool> {
        self.0.is_default_output(id)
    }
}

impl DeviceLister for ArcAudio {
    fn list_output_device_ids(&self) -> Vec<(String, bool)> {
        self.0.list_output_devices()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.is_default))
            .collect()
    }
    fn list_input_device_ids(&self) -> Vec<(String, bool)> {
        self.0.list_input_devices()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.is_default))
            .collect()
    }
}

fn main() -> anyhow::Result<()> {
    // Very lightweight CLI arg parsing — no external dep needed for two flags.
    let args: Vec<String> = std::env::args().collect();
    let mut config_path: Option<PathBuf> = None;
    let mut profiles_dir: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--config" | "-c" => {
                i += 1;
                config_path = args.get(i).map(PathBuf::from);
            }
            "--profiles" | "-p" => {
                i += 1;
                profiles_dir = args.get(i).map(PathBuf::from);
            }
            "--help" | "-h" => {
                eprintln!("Usage: midium [OPTIONS]");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  -c, --config <PATH>    Config directory (default: platform config dir)");
                eprintln!("  -p, --profiles <PATH>  Profiles directory (default: ./profiles)");
                eprintln!("  -h, --help             Show this help");
                return Ok(());
            }
            other => {
                eprintln!("Unknown argument: {other}. Use --help for usage.");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Resolve config directory
    let cfg_dir = config_path.unwrap_or_else(config_dir);

    // Load app config (uses resolved dir override if set)
    let config = if cfg_dir == config_dir() {
        midium_core::config::load_config()?
    } else {
        let path = cfg_dir.join("config.toml");
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            toml::from_str(&content)?
        } else {
            midium_core::config::AppConfig::default()
        }
    };

    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.general.log_level.parse().unwrap_or_default()),
        )
        .init();

    info!("Midium daemon starting");
    info!(config_dir = %cfg_dir.display(), "Using config directory");

    // Load device profiles
    // Search order: --profiles flag > ./profiles (next to binary) > config dir/profiles
    let profiles_search: Vec<PathBuf> = [
        profiles_dir,
        // Relative to the current working directory (convenient for dev)
        Some(PathBuf::from("profiles")),
        Some(cfg_dir.join("profiles")),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mut all_profiles = Vec::new();
    for dir in &profiles_search {
        let found = load_profiles(dir);
        if !found.is_empty() {
            info!(dir = %dir.display(), count = found.len(), "Loaded device profiles");
            all_profiles.extend(found);
            break; // use first directory that has profiles
        }
    }
    if all_profiles.is_empty() {
        warn!("No device profiles found. MIDI devices will show as (generic).");
        warn!("Profile search dirs: {:?}", profiles_search);
    }

    // Create the central event bus
    let event_bus = EventBus::new();

    // Load mappings
    let mappings_config = {
        let path = cfg_dir.join("mappings.toml");
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            toml::from_str::<midium_core::config::MappingsConfig>(&content)?
        } else {
            // Fall back to the standard config dir location
            midium_core::config::load_mappings()?
        }
    };
    info!(count = mappings_config.mappings.len(), "Loaded mappings");
    info!(count = mappings_config.fader_groups.len(), "Loaded fader groups");

    // Set up the mapping engine
    let mut mapping_engine = MappingEngine::new(event_bus.clone());
    mapping_engine.load_mappings(mappings_config.mappings);

    // Set up the audio backend
    let audio_backend = create_backend()?;
    let caps = audio_backend.capabilities();
    info!(
        per_app_volume = caps.per_app_volume,
        device_switching = caps.device_switching,
        "Audio backend initialized"
    );

    // List audio output devices on startup
    match audio_backend.list_output_devices() {
        Ok(devices) if !devices.is_empty() => {
            info!("Output devices:");
            for dev in &devices {
                info!(
                    "  [{}{}] {} (id={})",
                    if dev.is_default { "default, " } else { "" },
                    "output",
                    dev.name,
                    dev.id,
                );
            }
        }
        Ok(_) => info!("No output devices found"),
        Err(e) => error!("Failed to list output devices: {e}"),
    }

    // List per-app sessions if supported
    if caps.per_app_volume {
        match audio_backend.list_sessions() {
            Ok(sessions) if !sessions.is_empty() => {
                info!("Audio sessions (per-app):");
                for s in &sessions {
                    info!(
                        "  {} (vol={:.0}%{})",
                        s.name,
                        s.volume * 100.0,
                        if s.muted { ", muted" } else { "" }
                    );
                }
            }
            Ok(_) => {}
            Err(e) => warn!("Failed to list sessions: {e}"),
        }
    }

    // List MIDI ports
    let midi_ports = MidiManager::list_ports();
    if midi_ports.is_empty() {
        info!("No MIDI devices found yet (will keep scanning every {}s)", config.midi.poll_interval_secs);
    } else {
        info!("MIDI ports:");
        for port in &midi_ports {
            info!("  {}", port);
        }
    }

    // Build the tokio runtime and run async code
    let fader_groups = mappings_config.fader_groups;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main(
            event_bus,
            mapping_engine,
            audio_backend,
            all_profiles,
            fader_groups,
            config.midi.poll_interval_secs,
        ))
}

async fn async_main(
    event_bus: EventBus,
    mut mapping_engine: MappingEngine,
    audio_backend: Box<dyn midium_audio::backend::AudioBackend>,
    profiles: Vec<midium_midi::profile::DeviceProfile>,
    fader_groups: Vec<midium_core::types::FaderGroup>,
    midi_poll_interval_secs: u64,
) -> anyhow::Result<()> {
    // Wrap in Arc so it can be shared between dispatcher and GroupManager.
    let audio_arc: Arc<dyn AudioBackend> = Arc::from(audio_backend);
    let dispatcher = Arc::new(
        ActionDispatcher::new(Box::new(ArcAudio(audio_arc.clone())))
            .with_device_lister(Box::new(ArcAudio(audio_arc.clone())))
            .with_event_bus(event_bus.clone()),
    );

    let profiles_arc = Arc::new(profiles);

    // Spawn GroupManager
    let group_manager = GroupManager::new(
        fader_groups,
        profiles_arc.clone(),
        Box::new(ArcAudio(audio_arc.clone())),
        event_bus.clone(),
    );
    tokio::spawn(async move {
        group_manager.run().await;
    });

    // Spawn MIDI manager
    let midi_manager = MidiManager::new(event_bus.clone(), midi_poll_interval_secs, (*profiles_arc).clone());
    let midi_handle = tokio::spawn(async move {
        midi_manager.run().await;
    });

    // Spawn event processing loop
    let bus_for_events = event_bus.clone();
    let event_handle = tokio::spawn(async move {
        let mut rx = bus_for_events.subscribe();
        loop {
            match rx.recv().await {
                Ok(AppEvent::Midi(midi_event)) => {
                    mapping_engine.process_midi_event(&midi_event);
                }
                Ok(AppEvent::ActionTriggered { action, value }) => {
                    dispatcher.dispatch(&action, value);
                }
                Ok(AppEvent::DeviceConnected { device }) => {
                    info!(device = %device, "MIDI device connected");
                }
                Ok(AppEvent::DeviceDisconnected { device }) => {
                    info!(device = %device, "MIDI device disconnected");
                }
                Ok(AppEvent::Shutdown) => {
                    info!("Event loop shutting down");
                    break;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Event bus lagged — some events dropped");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    info!("Midium daemon running. Press Ctrl+C to stop.");
    signal::ctrl_c().await?;
    info!("Shutting down...");

    event_bus.publish(AppEvent::Shutdown);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), midi_handle).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), event_handle).await;

    info!("Goodbye.");
    Ok(())
}
