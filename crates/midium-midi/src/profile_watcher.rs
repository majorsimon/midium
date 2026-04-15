use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{debug, info, warn};

use midium_core::event_bus::EventBus;
use midium_core::types::AppEvent;

use crate::profile::{bundled_profiles, merge_profiles};

const DEBOUNCE: Duration = Duration::from_millis(500);

/// Watches profile directories for filesystem changes and publishes
/// `ProfilesReloaded` events when TOML files are created, modified, or removed.
pub struct ProfileWatcher {
    event_bus: EventBus,
    dirs: Vec<PathBuf>,
}

impl ProfileWatcher {
    pub fn new(event_bus: EventBus, dirs: Vec<PathBuf>) -> Self {
        Self { event_bus, dirs }
    }

    /// Run the watcher loop. Blocks the current async task until shutdown.
    pub async fn run(self) {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

        let mut watcher = match RecommendedWatcher::new(tx, notify::Config::default()) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to create file watcher: {e}");
                return;
            }
        };

        for dir in &self.dirs {
            if dir.exists() {
                if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                    warn!(dir = %dir.display(), "Failed to watch directory: {e}");
                } else {
                    info!(dir = %dir.display(), "Watching profile directory for changes");
                }
            }
        }

        let mut shutdown_rx = self.event_bus.subscribe();
        let mut last_reload = Instant::now() - DEBOUNCE;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    let mut changed = false;
                    while let Ok(event) = rx.try_recv() {
                        if let Ok(event) = event {
                            if is_toml_change(&event) {
                                changed = true;
                            }
                        }
                    }
                    if changed && last_reload.elapsed() >= DEBOUNCE {
                        last_reload = Instant::now();
                        self.reload();
                    }
                }
                event = shutdown_rx.recv() => {
                    if matches!(event, Ok(AppEvent::Shutdown) | Err(_)) {
                        debug!("Profile watcher shutting down");
                        break;
                    }
                }
            }
        }
    }

    fn reload(&self) {
        info!("Profile directory changed — reloading profiles");
        let profiles = merge_profiles(bundled_profiles(), &self.dirs);
        info!(count = profiles.len(), "Profiles reloaded");
        self.event_bus
            .publish(AppEvent::ProfilesReloaded { profiles });
    }
}

fn is_toml_change(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event
        .paths
        .iter()
        .any(|p| p.extension().is_some_and(|e| e == "toml"))
}
