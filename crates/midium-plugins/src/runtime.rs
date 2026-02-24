use midium_core::event_bus::EventBus;
use tracing::info;

/// Manages Lua plugin lifecycle — stub for Phase 4.
pub struct PluginManager;

impl PluginManager {
    pub fn new(_event_bus: EventBus) -> Self {
        info!("Plugin system initialized (no plugins loaded — Phase 4)");
        Self
    }
}
