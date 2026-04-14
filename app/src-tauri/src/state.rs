use std::sync::Arc;

use tokio::sync::{oneshot, Mutex};

use midium_audio::backend::AudioBackend;
use midium_core::{
    config::{AppConfig, MappingsConfig},
    dispatch::ActionDispatcher,
    event_bus::EventBus,
    mapping::MappingEngine,
    types::MidiEvent,
};
use midium_midi::DeviceProfile;
use midium_plugins::PluginInfo;

pub struct AppState {
    pub event_bus: EventBus,
    pub audio: Arc<dyn AudioBackend>,
    pub mapping_engine: Arc<Mutex<MappingEngine>>,
    pub dispatcher: Arc<ActionDispatcher>,
    pub mappings_config: Arc<Mutex<MappingsConfig>>,
    pub app_config: Arc<Mutex<AppConfig>>,
    pub current_shortcut: Arc<Mutex<Option<String>>>,
    pub midi_learn_tx: Arc<Mutex<Option<oneshot::Sender<MidiEvent>>>>,
    pub plugin_list: Arc<Mutex<Vec<PluginInfo>>>,
    pub profiles: Arc<Vec<DeviceProfile>>,
}
