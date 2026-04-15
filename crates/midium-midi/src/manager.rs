use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection, SendError};
use tracing::{debug, error, info, warn};

use midium_core::event_bus::EventBus;
use midium_core::types::{AppEvent, DeviceProfile};

use crate::parse::parse_midi;
use crate::profile::match_profile;

/// Manages MIDI device discovery, connection, and event forwarding.
pub struct MidiManager {
    event_bus: EventBus,
    poll_interval: Duration,
    profiles: Arc<Vec<DeviceProfile>>,
    /// Track which ports we've already connected to (by name).
    connected_ports: Arc<Mutex<HashSet<String>>>,
    /// Keep input connections alive — dropping them closes the port.
    input_connections: Arc<Mutex<HashMap<String, MidiInputConnection<()>>>>,
    /// Output connections keyed by port name — used for LED feedback.
    out_connections: Arc<Mutex<HashMap<String, MidiOutputConnection>>>,
}

impl MidiManager {
    pub fn new(event_bus: EventBus, poll_interval_secs: u64, profiles: Vec<DeviceProfile>) -> Self {
        Self {
            event_bus,
            poll_interval: Duration::from_secs(poll_interval_secs),
            profiles: Arc::new(profiles),
            connected_ports: Arc::new(Mutex::new(HashSet::new())),
            input_connections: Arc::new(Mutex::new(HashMap::new())),
            out_connections: Arc::new(Mutex::new(HashMap::<String, MidiOutputConnection>::new())),
        }
    }

    /// Start the device polling loop. Runs until the EventBus signals Shutdown.
    pub async fn run(mut self) {
        let mut shutdown_rx = self.event_bus.subscribe();
        let out_conns = self.out_connections.clone();

        // Spawn a dedicated task that drains SendMidi events from the bus.
        let mut midi_rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match midi_rx.recv().await {
                    Ok(AppEvent::SendMidi { device, data }) => {
                        let mut conns = out_conns.lock().unwrap();
                        if let Some(conn) = conns.get_mut(&device) {
                            if let Err(e) = conn.send(&data) {
                                let e: SendError = e;
                                debug!(port = %device, "MIDI output send failed: {e}");
                            }
                        }
                    }
                    Ok(AppEvent::Shutdown) | Err(_) => break,
                    _ => {}
                }
            }
        });

        info!(
            poll_interval = ?self.poll_interval,
            "MIDI manager started"
        );

        loop {
            self.scan_and_connect();

            tokio::select! {
                _ = tokio::time::sleep(self.poll_interval) => {}
                event = shutdown_rx.recv() => {
                    match event {
                        Ok(AppEvent::ProfilesReloaded { profiles }) => {
                            info!(count = profiles.len(), "MidiManager: profiles reloaded");
                            self.profiles = Arc::new(profiles);
                        }
                        Ok(AppEvent::Shutdown) | Err(_) => {
                            info!("MIDI manager shutting down");
                            break;
                        }
                        _ => {}
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
        let current_port_names: HashSet<String> = ports
            .iter()
            .filter_map(|p| midi_in.port_name(p).ok())
            .collect();

        // Detect disconnected devices: any port in connected_ports that is no
        // longer visible has been unplugged.
        let mut connected = self.connected_ports.lock().unwrap();
        let stale: Vec<String> = connected
            .iter()
            .filter(|name| !current_port_names.contains(*name))
            .cloned()
            .collect();

        for name in stale {
            info!(port = %name, "MIDI device disconnected");
            connected.remove(&name);
            self.input_connections.lock().unwrap().remove(&name);
            self.out_connections.lock().unwrap().remove(&name);
            self.event_bus.publish(AppEvent::DeviceDisconnected {
                device: name,
            });
        }

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

            // --- Input connection ---
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
                    // --- Output connection (best-effort, established BEFORE DeviceConnected
                    //     so that LED sync triggered by the event can actually send data) ---
                    match MidiOutput::new(&format!("midium-out-{}", port_name)) {
                        Ok(midi_out) => {
                            let out_ports = midi_out.ports();
                            let out_target = out_ports.iter().find(|p| {
                                midi_out
                                    .port_name(p)
                                    .map(|n| n == port_name)
                                    .unwrap_or(false)
                            });

                            if let Some(out_port) = out_target {
                                match midi_out
                                    .connect(out_port, &format!("midium-out-{}", port_name))
                                {
                                    Ok(out_conn) => {
                                        debug!(port = %port_name, "MIDI output connected");
                                        self.out_connections
                                            .lock()
                                            .unwrap()
                                            .insert(port_name.clone(), out_conn);
                                    }
                                    Err(e) => {
                                        debug!(port = %port_name, "MIDI output connection failed (LED feedback unavailable): {e}");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!(port = %port_name, "Could not open MIDI output: {e}");
                        }
                    }

                    debug!(port = %port_name, "MIDI input connected");
                    self.event_bus.publish(AppEvent::DeviceConnected {
                        device: port_name.clone(),
                    });
                    connected.insert(port_name.clone());
                    self.input_connections.lock().unwrap().insert(port_name.clone(), connection);
                }
                Err(e) => {
                    warn!(port = %port_name, "Failed to connect MIDI input: {e}");
                    continue;
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
