use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mlua::prelude::*;
use serde::Serialize;
use tracing::{debug, info, warn};

use midium_audio::backend::AudioBackend;
use midium_core::event_bus::EventBus;
use midium_core::types::{Action, AppEvent, AudioTarget, MidiMessage};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A custom action registered by a plugin via `midium.register_action`.
#[derive(Debug, Clone, Serialize)]
pub struct RegisteredAction {
    pub plugin: String,
    pub name: String,
    pub description: String,
}

/// Summary info about one loaded plugin (serialisable for IPC).
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub action_count: usize,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Internal: one sandboxed Lua VM per plugin file
// ---------------------------------------------------------------------------

type StateStore = Arc<Mutex<HashMap<String, String>>>;

struct PluginRuntime {
    name: String,
    lua: Lua,
    actions: Arc<Mutex<Vec<RegisteredAction>>>,
}

impl PluginRuntime {
    fn new(path: &Path, audio: Arc<dyn AudioBackend>) -> LuaResult<Self> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_owned();

        let lua = Lua::new();

        // 16 MB memory ceiling
        lua.set_memory_limit(16 * 1024 * 1024)?;

        // Strip dangerous stdlib entries
        apply_sandbox(&lua)?;

        let actions: Arc<Mutex<Vec<RegisteredAction>>> = Arc::new(Mutex::new(vec![]));
        let state: StateStore = Arc::new(Mutex::new(HashMap::new()));

        let api = build_api(&lua, &name, audio, actions.clone(), state)?;
        lua.globals().set("midium", api)?;

        // Load plugin source. `eval` captures any return value (module pattern).
        let source = std::fs::read_to_string(path).map_err(LuaError::external)?;
        let ret: LuaValue = lua
            .load(&source)
            .set_name(path.to_string_lossy().as_ref())
            .eval()?;

        if let LuaValue::Table(module) = ret {
            lua.globals().set("_module", module)?;
        }

        Ok(Self { name, lua, actions })
    }

    // ---- lifecycle hooks ---------------------------------------------------

    fn on_load(&self) {
        match self.call_hook("on_load", ()) {
            Ok(()) => info!(plugin = %self.name, "on_load ok"),
            Err(e) => warn!(plugin = %self.name, "on_load: {e}"),
        }
    }

    fn on_unload(&self) {
        if let Err(e) = self.call_hook("on_unload", ()) {
            debug!(plugin = %self.name, "on_unload: {e}");
        }
    }

    fn on_midi_event(&self, event: &midium_core::types::MidiEvent) {
        let build: LuaResult<LuaTable> = (|| {
            let t = self.lua.create_table()?;
            t.set("device", event.device.clone())?;
            t.set("channel", event.channel)?;

            let msg = self.lua.create_table()?;
            match &event.message {
                MidiMessage::ControlChange { control, value } => {
                    let cc = self.lua.create_table()?;
                    cc.set("control", *control)?;
                    cc.set("value", *value)?;
                    msg.set("cc", cc)?;
                }
                MidiMessage::NoteOn { note, velocity } => {
                    let n = self.lua.create_table()?;
                    n.set("note", *note)?;
                    n.set("velocity", *velocity)?;
                    n.set("on", true)?;
                    msg.set("note", n)?;
                }
                MidiMessage::NoteOff { note, velocity } => {
                    let n = self.lua.create_table()?;
                    n.set("note", *note)?;
                    n.set("velocity", *velocity)?;
                    n.set("on", false)?;
                    msg.set("note", n)?;
                }
                MidiMessage::PitchBend { value } => {
                    // Normalise to -1.0 .. +1.0
                    msg.set("pitch_bend", *value as f64 / 8192.0 - 1.0)?;
                }
            }
            t.set("message", msg)?;
            Ok(t)
        })();

        match build {
            Ok(table) => {
                if let Err(e) = self.call_hook("on_midi_event", table) {
                    debug!(plugin = %self.name, "on_midi_event: {e}");
                }
            }
            Err(e) => debug!(plugin = %self.name, "build midi table: {e}"),
        }
    }

    fn dispatch_action(&self, action_name: &str, value: f64) {
        let result: LuaResult<()> = self
            .lua
            .globals()
            .get::<LuaTable>("_action_handlers")
            .and_then(|handlers| handlers.get::<LuaFunction>(action_name))
            .and_then(|f| f.call(value));

        if let Err(e) = result {
            warn!(plugin = %self.name, action = action_name, "dispatch: {e}");
        }
    }

    // ---- hook resolution ---------------------------------------------------

    /// Find a hook function (module pattern takes priority, then global).
    fn get_hook(&self, name: &str) -> Option<LuaFunction> {
        let g = self.lua.globals();
        if let Ok(module) = g.get::<LuaTable>("_module") {
            if let Ok(f) = module.get::<LuaFunction>(name) {
                return Some(f);
            }
        }
        g.get::<LuaFunction>(name).ok()
    }

    fn call_hook(&self, hook: &str, args: impl IntoLuaMulti) -> LuaResult<()> {
        if let Some(f) = self.get_hook(hook) {
            f.call::<()>(args)
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Sandboxing
// ---------------------------------------------------------------------------

fn apply_sandbox(lua: &Lua) -> LuaResult<()> {
    let g = lua.globals();

    // Remove dangerous stdlib modules and functions.
    // `package`/`require` would allow loading arbitrary Lua/C modules from disk.
    for key in ["io", "debug", "loadfile", "dofile", "package", "require"] {
        g.set(key, LuaNil)?;
    }

    // Restrict `os` to a safe subset — only `clock` and `time` are kept.
    // Excluded: execute, getenv, remove, rename, tmpname, setlocale, exit.
    if let Ok(os_orig) = g.get::<LuaTable>("os") {
        let os_safe = lua.create_table()?;
        for key in ["clock", "time"] {
            if let Ok(v) = os_orig.get::<LuaValue>(key) {
                os_safe.set(key, v)?;
            }
        }
        g.set("os", os_safe)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// API injection
// ---------------------------------------------------------------------------

fn build_api(
    lua: &Lua,
    plugin_name: &str,
    audio: Arc<dyn AudioBackend>,
    actions: Arc<Mutex<Vec<RegisteredAction>>>,
    state: StateStore,
) -> LuaResult<LuaTable> {
    let api = lua.create_table()?;

    // midium.log(msg)
    {
        let pname = plugin_name.to_owned();
        api.set(
            "log",
            lua.create_function(move |_, msg: String| {
                info!(plugin = %pname, "{msg}");
                Ok(())
            })?,
        )?;
    }

    // midium.audio.*
    let audio_t = lua.create_table()?;

    macro_rules! audio_fn {
        ($table:expr, $key:expr, $closure:expr) => {
            $table.set($key, lua.create_function($closure)?)?;
        };
    }

    {
        let a = audio.clone();
        audio_fn!(audio_t, "get_volume", move |_, target: String| {
            a.get_volume(&parse_target(&target)).map_err(LuaError::external)
        });
    }
    {
        let a = audio.clone();
        audio_fn!(audio_t, "set_volume", move |_, (target, vol): (String, f64)| {
            a.set_volume(&parse_target(&target), vol).map_err(LuaError::external)
        });
    }
    {
        let a = audio.clone();
        audio_fn!(audio_t, "is_muted", move |_, target: String| {
            a.is_muted(&parse_target(&target)).map_err(LuaError::external)
        });
    }
    {
        let a = audio.clone();
        audio_fn!(audio_t, "set_mute", move |_, (target, muted): (String, bool)| {
            a.set_mute(&parse_target(&target), muted).map_err(LuaError::external)
        });
    }
    {
        let a = audio.clone();
        audio_fn!(audio_t, "list_sessions", move |lua, ()| {
            let sessions = a.list_sessions().map_err(LuaError::external)?;
            let list = lua.create_table()?;
            for (i, s) in sessions.iter().enumerate() {
                let st = lua.create_table()?;
                st.set("name", s.name.clone())?;
                st.set("volume", s.volume)?;
                st.set("muted", s.muted)?;
                list.set(i + 1, st)?;
            }
            Ok(list)
        });
    }
    {
        let a = audio.clone();
        audio_fn!(audio_t, "list_devices", move |lua, ()| {
            let devices = a.list_output_devices().map_err(LuaError::external)?;
            let list = lua.create_table()?;
            for (i, d) in devices.iter().enumerate() {
                let dt = lua.create_table()?;
                dt.set("id", d.id.clone())?;
                dt.set("name", d.name.clone())?;
                dt.set("is_default", d.is_default)?;
                list.set(i + 1, dt)?;
            }
            Ok(list)
        });
    }
    api.set("audio", audio_t)?;

    // midium.state.*
    let state_t = lua.create_table()?;
    {
        let s = state.clone();
        state_t.set(
            "get",
            lua.create_function(move |_, key: String| Ok(s.lock().unwrap().get(&key).cloned()))?,
        )?;
    }
    {
        let s = state.clone();
        state_t.set(
            "set",
            lua.create_function(move |_, (key, val): (String, String)| {
                s.lock().unwrap().insert(key, val);
                Ok(())
            })?,
        )?;
    }
    api.set("state", state_t)?;

    // midium.register_action(name, description, fn)
    {
        let pname = plugin_name.to_owned();
        let acts = actions.clone();
        api.set(
            "register_action",
            lua.create_function(
                move |lua, (action_name, desc, handler): (String, Option<String>, LuaFunction)| {
                    // Persist handler in a Lua-side table so GC keeps it alive.
                    let handlers: LuaTable = lua
                        .globals()
                        .get::<LuaTable>("_action_handlers")
                        .or_else(|_| lua.create_table())?;
                    handlers.set(action_name.clone(), handler)?;
                    lua.globals().set("_action_handlers", handlers)?;

                    acts.lock().unwrap().push(RegisteredAction {
                        plugin: pname.clone(),
                        name: action_name,
                        description: desc.unwrap_or_default(),
                    });
                    Ok(())
                },
            )?,
        )?;
    }

    Ok(api)
}

// ---------------------------------------------------------------------------
// Audio target string → enum
// ---------------------------------------------------------------------------

fn parse_target(s: &str) -> AudioTarget {
    match s {
        "master" | "system" => AudioTarget::SystemMaster,
        "focused" => AudioTarget::FocusedApplication,
        s if s.starts_with("app:") => AudioTarget::Application { name: s[4..].to_owned() },
        s if s.starts_with("device:") => AudioTarget::Device { id: s[7..].to_owned() },
        _ => AudioTarget::SystemMaster,
    }
}

// ---------------------------------------------------------------------------
// PluginManager — public interface
// ---------------------------------------------------------------------------

pub struct PluginManager;

impl PluginManager {
    /// Load plugins from `dirs`, call `on_load`, then hand off to a dedicated
    /// OS thread for the event loop.
    ///
    /// Returns a snapshot of plugin info (populated after `on_load` runs).
    ///
    /// # Why a dedicated thread?
    /// `mlua::Lua` uses `Rc` internally, making it `!Send`. We subscribe to
    /// the `EventBus` before spawning (the `Receiver` is `Send`), then create
    /// all Lua VMs inside the new thread so they never cross a thread boundary.
    pub fn spawn(
        plugin_dirs: Vec<PathBuf>,
        audio: Arc<dyn AudioBackend>,
        event_bus: EventBus,
    ) -> Vec<PluginInfo> {
        // Subscribe before spawning — Receiver<AppEvent> is Send.
        let mut rx = event_bus.subscribe();

        // One-shot channel: thread sends back plugin info after on_load.
        let (info_tx, info_rx) = std::sync::mpsc::sync_channel::<Vec<PluginInfo>>(1);

        std::thread::spawn(move || {
            // --- Load all .lua files ---
            let mut runtimes: Vec<PluginRuntime> = Vec::new();
            for dir in &plugin_dirs {
                if !dir.is_dir() {
                    continue;
                }
                let Ok(rd) = std::fs::read_dir(dir) else { continue };
                for entry in rd.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("lua") {
                        continue;
                    }
                    match PluginRuntime::new(&path, audio.clone()) {
                        Ok(rt) => {
                            info!(path = %path.display(), "Plugin loaded");
                            runtimes.push(rt);
                        }
                        Err(e) => warn!(path = %path.display(), "Plugin load failed: {e}"),
                    }
                }
            }

            // --- on_load ---
            for rt in &runtimes {
                rt.on_load();
            }

            // --- Send info snapshot back before blocking ---
            let info: Vec<PluginInfo> = runtimes
                .iter()
                .map(|rt| PluginInfo {
                    name: rt.name.clone(),
                    action_count: rt.actions.lock().unwrap().len(),
                    enabled: true,
                })
                .collect();
            let _ = info_tx.send(info);

            // --- Async event loop on a current_thread runtime ---
            // block_on doesn't require Send, so !Send Lua values are fine.
            let tokio_rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("plugin thread runtime");

            info!(count = runtimes.len(), "Plugin event loop started");

            tokio_rt.block_on(async move {
                loop {
                    match rx.recv().await {
                        Err(_) => break,
                        Ok(AppEvent::Shutdown) => break,
                        Ok(AppEvent::Midi(ref event)) => {
                            for rt in &runtimes {
                                rt.on_midi_event(event);
                            }
                        }
                        Ok(AppEvent::ActionTriggered {
                            action: Action::RunPluginAction { ref plugin, ref action },
                            value,
                        }) => {
                            for rt in &runtimes {
                                if rt.name == *plugin {
                                    rt.dispatch_action(action, value);
                                    break;
                                }
                            }
                        }
                        Ok(_) => {}
                    }
                }
                for rt in &runtimes {
                    rt.on_unload();
                }
                info!("Plugin event loop shut down");
            });
        });

        // Wait up to 5s for on_load to complete (it's synchronous and fast).
        info_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or_default()
    }
}
