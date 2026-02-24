use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use midir::{MidiInput, MidiInputConnection};
use tracing::{debug, error, info, warn};

use midium_core::event_bus::EventBus;
use midium_core::types::AppEvent;

use crate::parse::parse_midi;
use crate::profile::{match_profile, DeviceProfile};

/// Manages MIDI device discovery, connection, and event forwarding.
pub struct MidiManager {
    event_bus: EventBus,
    poll_interval: Duration,
    profiles: Arc<Vec<DeviceProfile>>,
    /// Track which ports we've already connected to (by name).
    connected_ports: Arc<Mutex<HashSet<String>>>,
    /// Keep connections alive — dropping them closes the port.
    _connections: Arc<Mutex<Vec<MidiInputConnection<()>>>>,
}

impl MidiManager {
    pub fn new(event_bus: EventBus, poll_interval_secs: u64, profiles: Vec<DeviceProfile>) -> Self {
        Self {
            event_bus,
            poll_interval: Duration::from_secs(poll_interval_secs),
            profiles: Arc::new(profiles),
            connected_ports: Arc::new(Mutex::new(HashSet::new())),
            _connections: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Start the device polling loop. Runs until the EventBus signals Shutdown.
    pub async fn run(self) {
        let mut shutdown_rx = self.event_bus.subscribe();

        info!(
            poll_interval = ?self.poll_interval,
            "MIDI manager started"
        );

        loop {
            self.scan_and_connect();

            tokio::select! {
                _ = tokio::time::sleep(self.poll_interval) => {}
                event = shutdown_rx.recv() => {
                    if matches!(event, Ok(AppEvent::Shutdown)) {
                        info!("MIDI manager shutting down");
                        break;
                    }
                }
            }
        }
    }

    fn scan_and_connect(&self) {
        let midi_in = match MidiInput::new("midium-scan") {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to create MIDI input for scanning: {e}");
                return;
            }
        };

        let ports = midi_in.ports();
        let mut connected = self.connected_ports.lock().unwrap();

        for port in &ports {
            let port_name = match midi_in.port_name(port) {
                Ok(name) => name,
                Err(_) => continue,
            };

            if connected.contains(&port_name) {
                continue;
            }

            // Match against loaded profiles for better logging
            let profile_name = match_profile(&port_name, &self.profiles)
                .map(|p| p.name.as_str())
                .unwrap_or("(generic)");

            info!(port = %port_name, profile = %profile_name, "Connecting to MIDI device");

            // midir requires a fresh MidiInput per connection
            let midi_in_for_port = match MidiInput::new(&format!("midium-{}", port_name)) {
                Ok(m) => m,
                Err(e) => {
                    warn!(port = %port_name, "Failed to create MIDI input: {e}");
                    continue;
                }
            };

            let ports_for_connect = midi_in_for_port.ports();
            let target_port = ports_for_connect.iter().find(|p| {
                midi_in_for_port
                    .port_name(p)
                    .map(|n| n == port_name)
                    .unwrap_or(false)
            });

            let target_port = match target_port {
                Some(p) => p,
                None => {
                    warn!(port = %port_name, "Port disappeared during connection");
                    continue;
                }
            };

            let bus = self.event_bus.clone();
            let name = port_name.clone();

            match midi_in_for_port.connect(
                target_port,
                &format!("midium-{}", port_name),
                move |_timestamp, message, _| {
                    if let Some(event) = parse_midi(&name, message) {
                        bus.publish(AppEvent::Midi(event));
                    }
                },
                (),
            ) {
                Ok(connection) => {
                    debug!(port = %port_name, "MIDI device connected successfully");
                    self.event_bus.publish(AppEvent::DeviceConnected {
                        device: port_name.clone(),
                    });
                    connected.insert(port_name.clone());
                    self._connections.lock().unwrap().push(connection);
                }
                Err(e) => {
                    warn!(port = %port_name, "Failed to connect: {e}");
                }
            }
        }
    }

    /// List currently visible MIDI input port names.
    pub fn list_ports() -> Vec<String> {
        let midi_in = match MidiInput::new("midium-list") {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        midi_in
            .ports()
            .iter()
            .filter_map(|p| midi_in.port_name(p).ok())
            .collect()
    }
}
