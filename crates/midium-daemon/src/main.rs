use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::signal;
use tracing::{error, info, warn};

use midium_audio::{backend::AudioBackend, create_backend, SharedAudio};
use midium_core::config::config_dir;
use midium_core::dispatch::ActionDispatcher;
use midium_core::event_bus::EventBus;
use midium_core::mapping::MappingEngine;
use midium_core::types::AppEvent;
use midium_midi::manager::MidiManager;
use midium_midi::profile::load_profiles;
use midium_midi::{GroupManager, ProfileWatcher};

/// Headless MIDI mixer daemon — maps MIDI controllers to system audio.
#[derive(Parser)]
#[command(name = "midium", version, about)]
struct Cli {
    /// Config directory (default: platform config dir)
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Profiles directory (default: ./profiles or config dir/profiles)
    #[arg(short, long, value_name = "PATH")]
    profiles: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let cfg_dir = cli.config.unwrap_or_else(config_dir);

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

    // Init tracing — console (human-readable) + file (JSON, daily rotation)
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| config.general.log_level.parse().unwrap_or_default());

    let log_dir = cfg_dir.join("logs");
    let file_appender = tracing_appender::rolling::daily(&log_dir, "midium.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking),
        )
        .init();

    info!("Midium daemon starting");
    info!(config_dir = %cfg_dir.display(), "Using config directory");

    // Load device profiles
    // Search order: --profiles flag > ./profiles (next to binary) > config dir/profiles
    let profiles_search: Vec<PathBuf> = [
        cli.profiles,
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

    let mut fader_groups = mappings_config.fader_groups;
    fader_groups.sort_by_key(|g| g.group);
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
            profiles_search,
        ))
}

async fn async_main(
    event_bus: EventBus,
    mut mapping_engine: MappingEngine,
    audio_backend: Box<dyn midium_audio::backend::AudioBackend>,
    profiles: Vec<midium_midi::profile::DeviceProfile>,
    fader_groups: Vec<midium_core::types::FaderGroup>,
    midi_poll_interval_secs: u64,
    profile_dirs: Vec<PathBuf>,
) -> anyhow::Result<()> {
    // Wrap in Arc so it can be shared between dispatcher and GroupManager.
    let audio_arc: Arc<dyn AudioBackend> = Arc::from(audio_backend);
    let dispatcher = Arc::new(
        ActionDispatcher::new(Box::new(SharedAudio(audio_arc.clone())))
            .with_device_lister(Box::new(SharedAudio(audio_arc.clone())))
            .with_event_bus(event_bus.clone()),
    );

    let profiles_arc = Arc::new(profiles);

    // Spawn GroupManager
    let group_manager = GroupManager::new(
        fader_groups,
        profiles_arc.clone(),
        Box::new(SharedAudio(audio_arc.clone())),
        event_bus.clone(),
    );
    tokio::spawn(async move {
        group_manager.run().await;
    });

    // Spawn profile watcher
    let watcher = ProfileWatcher::new(event_bus.clone(), profile_dirs);
    tokio::spawn(async move {
        watcher.run().await;
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
