use std::sync::{Arc, Mutex};

use libpulse_binding as pa;
use pa::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use pa::mainloop::threaded::Mainloop;
use pa::operation::State as OpState;
use pa::volume::{ChannelVolumes, Volume, VolumeLinear};
use tracing::{debug, warn};

use midium_core::dispatch::VolumeControl;
use midium_core::types::{AudioCapabilities, AudioDeviceInfo, AudioSessionInfo, AudioTarget};

use crate::backend::AudioBackend;

pub struct PulseAudioBackend;

impl PulseAudioBackend {
    pub fn new() -> anyhow::Result<Self> {
        // Test that we can connect
        let _conn = PulseConn::connect()?;
        debug!("PulseAudio backend initialized");
        Ok(Self)
    }

    fn volume_from_f64(value: f64) -> ChannelVolumes {
        let vol = Volume::from(VolumeLinear(value.clamp(0.0, 1.0)));
        let mut cvols = ChannelVolumes::default();
        cvols.set(2, vol);
        cvols
    }

    fn volume_to_f64(cvols: &ChannelVolumes) -> f64 {
        VolumeLinear::from(cvols.avg()).0.clamp(0.0, 1.0)
    }
}

/// A short-lived synchronous PulseAudio connection. Creates a threaded
/// mainloop, connects, and provides blocking wrappers for introspection.
/// Dropped automatically when it goes out of scope.
struct PulseConn {
    mainloop: Mainloop,
    /// PulseAudio context — accessed via `self.context.introspect()` under the mainloop lock.
    context: Context,
}

// SAFETY: PulseConn is created and consumed within a single method call
// (connect → use → drop) and is never shared between threads concurrently.
// The internal Rc<MainloopInner> inside libpulse's Context is never exposed
// or cloned outside of the mainloop lock, which is held during all
// Context/Introspect operations.
// TODO: consider wrapping PulseAudio ops in a dedicated thread to eliminate this unsafe impl
unsafe impl Send for PulseConn {}

impl PulseConn {
    fn connect() -> anyhow::Result<Self> {
        let mut mainloop =
            Mainloop::new().ok_or_else(|| anyhow::anyhow!("Failed to create PA mainloop"))?;
        let mut context = Context::new(&mainloop, "midium")
            .ok_or_else(|| anyhow::anyhow!("Failed to create PA context"))?;

        context
            .connect(None, ContextFlagSet::NOFLAGS, None)
            .map_err(|e| anyhow::anyhow!("PA context connect: {e:?}"))?;

        mainloop
            .start()
            .map_err(|e| anyhow::anyhow!("PA mainloop start: {e:?}"))?;

        // Wait for Ready
        mainloop.lock();
        loop {
            match context.get_state() {
                ContextState::Ready => break,
                ContextState::Failed | ContextState::Terminated => {
                    mainloop.unlock();
                    anyhow::bail!("PulseAudio context failed to connect");
                }
                _ => mainloop.wait(),
            }
        }
        mainloop.unlock();

        Ok(Self {
            mainloop,
            context,
        })
    }

    /// Run a blocking introspect callback that collects results into a Vec.
    fn list_sinks(&mut self) -> anyhow::Result<Vec<pa::context::introspect::SinkInfo<'static>>> {
        let result: Arc<Mutex<Option<Vec<_>>>> = Arc::new(Mutex::new(Some(Vec::new())));
        let result_c = result.clone();

        self.mainloop.lock();
        let introspect = self.context.introspect();

        let op = introspect.get_sink_info_list(move |item| {
            if let pa::callbacks::ListResult::Item(info) = item {
                if let Ok(mut guard) = result_c.lock() {
                    if let Some(v) = guard.as_mut() {
                        v.push(info.clone().to_owned());
                    }
                }
            }
        });

        loop {
            match op.get_state() {
                OpState::Running => self.mainloop.wait(),
                _ => break,
            }
        }
        self.mainloop.unlock();

        let items = result.lock().unwrap().take().unwrap_or_default();
        Ok(items)
    }

    fn list_sources(
        &mut self,
    ) -> anyhow::Result<Vec<pa::context::introspect::SourceInfo<'static>>> {
        let result: Arc<Mutex<Option<Vec<_>>>> = Arc::new(Mutex::new(Some(Vec::new())));
        let result_c = result.clone();

        self.mainloop.lock();
        let introspect = self.context.introspect();

        let op = introspect.get_source_info_list(move |item| {
            if let pa::callbacks::ListResult::Item(info) = item {
                if let Ok(mut guard) = result_c.lock() {
                    if let Some(v) = guard.as_mut() {
                        v.push(info.clone().to_owned());
                    }
                }
            }
        });

        loop {
            match op.get_state() {
                OpState::Running => self.mainloop.wait(),
                _ => break,
            }
        }
        self.mainloop.unlock();

        let items = result.lock().unwrap().take().unwrap_or_default();
        Ok(items)
    }

    fn list_sink_inputs(
        &mut self,
    ) -> anyhow::Result<Vec<pa::context::introspect::SinkInputInfo<'static>>> {
        let result: Arc<Mutex<Option<Vec<_>>>> = Arc::new(Mutex::new(Some(Vec::new())));
        let result_c = result.clone();

        self.mainloop.lock();
        let introspect = self.context.introspect();

        let op = introspect.get_sink_input_info_list(move |item| {
            if let pa::callbacks::ListResult::Item(info) = item {
                if let Ok(mut guard) = result_c.lock() {
                    if let Some(v) = guard.as_mut() {
                        v.push(info.clone().to_owned());
                    }
                }
            }
        });

        loop {
            match op.get_state() {
                OpState::Running => self.mainloop.wait(),
                _ => break,
            }
        }
        self.mainloop.unlock();

        let items = result.lock().unwrap().take().unwrap_or_default();
        Ok(items)
    }

    fn set_sink_volume_by_name(&mut self, name: &str, cvols: &ChannelVolumes) -> anyhow::Result<()> {
        self.mainloop.lock();
        let mut introspect = self.context.introspect();

        let op = introspect.set_sink_volume_by_name(name, cvols, None);
        loop {
            match op.get_state() {
                OpState::Running => self.mainloop.wait(),
                _ => break,
            }
        }
        self.mainloop.unlock();
        Ok(())
    }

    fn set_sink_input_volume(&mut self, index: u32, cvols: &ChannelVolumes) -> anyhow::Result<()> {
        self.mainloop.lock();
        let mut introspect = self.context.introspect();

        let op = introspect.set_sink_input_volume(index, cvols, None);
        loop {
            match op.get_state() {
                OpState::Running => self.mainloop.wait(),
                _ => break,
            }
        }
        self.mainloop.unlock();
        Ok(())
    }

    fn set_sink_mute_by_name(&mut self, name: &str, mute: bool) -> anyhow::Result<()> {
        self.mainloop.lock();
        let mut introspect = self.context.introspect();

        let op = introspect.set_sink_mute_by_name(name, mute, None);
        loop {
            match op.get_state() {
                OpState::Running => self.mainloop.wait(),
                _ => break,
            }
        }
        self.mainloop.unlock();
        Ok(())
    }

    fn get_server_info(&mut self) -> anyhow::Result<pa::context::introspect::ServerInfo<'static>> {
        let result: Arc<Mutex<Option<pa::context::introspect::ServerInfo<'static>>>> =
            Arc::new(Mutex::new(None));
        let result_c = result.clone();

        self.mainloop.lock();
        let introspect = self.context.introspect();

        let op = introspect.get_server_info(move |info: &pa::context::introspect::ServerInfo<'_>| {
            *result_c.lock().unwrap() = Some(info.clone().to_owned());
        });

        loop {
            match op.get_state() {
                OpState::Running => self.mainloop.wait(),
                _ => break,
            }
        }
        self.mainloop.unlock();

        let info = result
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get server info"))?;
        Ok(info)
    }
}

impl VolumeControl for PulseAudioBackend {
    fn set_volume(&self, target: &AudioTarget, volume: f64) -> anyhow::Result<()> {
        let cvols = Self::volume_from_f64(volume);
        let mut conn = PulseConn::connect()?;

        match target {
            AudioTarget::SystemMaster => {
                let server = conn.get_server_info()?;
                let sink_name = server
                    .default_sink_name
                    .as_deref()
                    .unwrap_or("@DEFAULT_SINK@")
                    .to_string();
                conn.set_sink_volume_by_name(&sink_name, &cvols)
            }
            AudioTarget::Device { id } => {
                conn.set_sink_volume_by_name(id, &cvols)
            }
            AudioTarget::Application { name } => {
                let inputs = conn.list_sink_inputs()?;
                let input = inputs
                    .iter()
                    .find(|i| {
                        i.name.as_deref().is_some_and(|n| {
                            n.to_lowercase().contains(&name.to_lowercase())
                        })
                    })
                    .ok_or_else(|| anyhow::anyhow!("Application not found: {name}"))?;
                conn.set_sink_input_volume(input.index, &cvols)
            }
            AudioTarget::FocusedApplication => {
                warn!("FocusedApplication volume not supported on Linux");
                Ok(())
            }
        }
    }

    fn set_mute(&self, target: &AudioTarget, muted: bool) -> anyhow::Result<()> {
        let mut conn = PulseConn::connect()?;
        match target {
            AudioTarget::SystemMaster => {
                let server = conn.get_server_info()?;
                let sink_name = server
                    .default_sink_name
                    .as_deref()
                    .unwrap_or("@DEFAULT_SINK@")
                    .to_string();
                conn.set_sink_mute_by_name(&sink_name, muted)
            }
            AudioTarget::Device { id } => conn.set_sink_mute_by_name(id, muted),
            AudioTarget::Application { .. } | AudioTarget::FocusedApplication => {
                warn!(?target, "Mute not supported for this target on Linux");
                Ok(())
            }
        }
    }

    fn is_muted(&self, target: &AudioTarget) -> anyhow::Result<bool> {
        let mut conn = PulseConn::connect()?;
        let sinks = conn.list_sinks()?;

        let sink_name = match target {
            AudioTarget::SystemMaster => {
                let server = conn.get_server_info()?;
                server
                    .default_sink_name
                    .as_deref()
                    .unwrap_or("")
                    .to_string()
            }
            AudioTarget::Device { id } => id.clone(),
            AudioTarget::Application { .. } | AudioTarget::FocusedApplication => {
                warn!(?target, "Mute query not supported for this target on Linux");
                return Ok(false);
            }
        };

        Ok(sinks
            .iter()
            .find(|s| s.name.as_deref() == Some(&sink_name))
            .map(|s| s.mute)
            .unwrap_or(false))
    }

    fn is_default_output(&self, device_id: &str) -> anyhow::Result<bool> {
        let mut conn = PulseConn::connect()?;
        let server = conn.get_server_info()?;
        Ok(server.default_sink_name.as_deref().unwrap_or("") == device_id)
    }

    fn set_default_output(&self, device_id: &str) -> anyhow::Result<()> {
        // pactl uses the sink name as device ID on Linux
        let output = std::process::Command::new("pactl")
            .args(["set-default-sink", device_id])
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "pactl set-default-sink failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn set_default_input(&self, device_id: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("pactl")
            .args(["set-default-source", device_id])
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "pactl set-default-source failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}

impl AudioBackend for PulseAudioBackend {
    fn list_output_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>> {
        let mut conn = PulseConn::connect()?;
        let server = conn.get_server_info()?;
        let default_name = server.default_sink_name.as_deref().unwrap_or("").to_string();
        let sinks = conn.list_sinks()?;

        Ok(sinks
            .into_iter()
            .filter(|s| !s.name.as_deref().unwrap_or("").contains(".monitor"))
            .map(|s| {
                let name = s.name.as_deref().unwrap_or("").to_string();
                AudioDeviceInfo {
                    is_default: name == default_name,
                    id: name,
                    name: s
                        .description
                        .as_deref()
                        .unwrap_or("(unknown)")
                        .to_string(),
                    is_input: false,
                }
            })
            .collect())
    }

    fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>> {
        let mut conn = PulseConn::connect()?;
        let server = conn.get_server_info()?;
        let default_name = server
            .default_source_name
            .as_deref()
            .unwrap_or("")
            .to_string();
        let sources = conn.list_sources()?;

        Ok(sources
            .into_iter()
            .filter(|s| !s.name.as_deref().unwrap_or("").contains(".monitor"))
            .map(|s| {
                let name = s.name.as_deref().unwrap_or("").to_string();
                AudioDeviceInfo {
                    is_default: name == default_name,
                    id: name,
                    name: s
                        .description
                        .as_deref()
                        .unwrap_or("(unknown)")
                        .to_string(),
                    is_input: true,
                }
            })
            .collect())
    }

    fn get_volume(&self, target: &AudioTarget) -> anyhow::Result<f64> {
        let mut conn = PulseConn::connect()?;
        let sinks = conn.list_sinks()?;

        let sink_name = match target {
            AudioTarget::SystemMaster => {
                let server = conn.get_server_info()?;
                server
                    .default_sink_name
                    .as_deref()
                    .unwrap_or("")
                    .to_string()
            }
            AudioTarget::Device { id } => id.clone(),
            _ => return Ok(0.0),
        };

        sinks
            .iter()
            .find(|s| s.name.as_deref() == Some(&sink_name))
            .map(|s| Self::volume_to_f64(&s.volume))
            .ok_or_else(|| anyhow::anyhow!("Sink not found: {sink_name}"))
    }

    fn list_sessions(&self) -> anyhow::Result<Vec<AudioSessionInfo>> {
        let mut conn = PulseConn::connect()?;
        let inputs = conn.list_sink_inputs()?;

        Ok(inputs
            .into_iter()
            .map(|i| AudioSessionInfo {
                name: i.name.as_deref().unwrap_or("(unknown)").to_string(),
                pid: i.proplist.get_str("application.process.id")
                    .and_then(|s| s.parse().ok()),
                volume: Self::volume_to_f64(&i.volume),
                muted: i.mute,
            })
            .collect())
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            per_app_volume: true,
            device_switching: true,
            input_device_switching: true,
        }
    }
}
