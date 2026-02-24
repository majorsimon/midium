use tokio::sync::broadcast;
use crate::types::AppEvent;

const CHANNEL_CAPACITY: usize = 256;

/// Central event bus backed by a tokio broadcast channel.
///
/// All components (MIDI, Audio, Mapping, Plugins, GUI) publish and subscribe
/// through this bus, keeping them fully decoupled.
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<AppEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { sender }
    }

    /// Publish an event to all subscribers.
    pub fn publish(&self, event: AppEvent) {
        // Ignore errors — means no active receivers, which is fine during
        // startup/shutdown.
        let _ = self.sender.send(event);
    }

    /// Get a new receiver to subscribe to events.
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
