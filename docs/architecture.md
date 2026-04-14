# Midium — Architecture and Implementation Reference

---

## 1. What Midium Is

Midium is a cross-platform desktop application that maps MIDI controller inputs (faders, knobs, buttons, encoders) to system audio controls: volume, mute, device switching, and per-app volume. It ships in two forms:

- A **Tauri 2 GUI application** with a Svelte 4 frontend
- A **headless CLI daemon** (`midium` binary) for scripted or server use

The backend is written in Rust; the frontend is TypeScript/Svelte compiled to a static bundle consumed by Tauri.

---

## 2. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Svelte Frontend                          │
│  +page.svelte (tab shell)                                   │
│  Mixer · MappingEditor · FaderGroupEditor                   │
│  Devices · Settings · PluginManager                         │
└───────────────┬─────────────────────────────────────────────┘
                │  invoke() / listen()  (Tauri IPC)
┌───────────────▼─────────────────────────────────────────────┐
│              Tauri Bridge  (app/src-tauri/src/)             │
│  lib.rs (setup orchestration + event bridge)                │
│  state.rs (AppState with tokio::sync::Mutex fields)        │
│  tray.rs (tray icon + menu)                                 │
│  commands/ (30 async IPC handlers across 7 modules)         │
└───┬───────────────────────────────────────────────────────┬─┘
    │                                                       │
┌───▼──────────────┐   ┌──────────────────────────────────┐ │
│  midium-core     │   │  midium-midi                     │ │
│  types, config,  │◄──┤  parse, manager, profiles,       │ │
│  event_bus,      │   │  group_manager                   │ │
│  mapping,        │   └──────────────────────────────────┘ │
│  dispatch        │   ┌──────────────────────────────────┐ │
└───────────────── ┤   │  midium-audio                    │ │
                   │◄──┤  CoreAudio · PulseAudio · WASAPI │ │
                   │   └──────────────────────────────────┘ │
                   │   ┌──────────────────────────────────┐ │
                   │◄──┤  midium-plugins  (Lua sandbox)   │ │
                   │   └──────────────────────────────────┘ │
                   │   ┌──────────────────────────────────┐ │
                   └───┤  midium-shortcuts  (enigo)       │ │
                       └──────────────────────────────────┘ │
                       ┌──────────────────────────────────┐ │
                       │  midium-daemon  (headless CLI)   ├─┘
                       └──────────────────────────────────┘
```

All runtime communication between Rust components flows through a single `**EventBus**` — a thin wrapper around `tokio::sync::broadcast`. Components publish events; each subscriber processes what it cares about and ignores the rest.

---

## 3. Rust Workspace Structure

```
Cargo.toml                       workspace root
crates/
  midium-core/                   shared kernel
    src/types.rs                 domain types (incl. MidiMessage::raw_value())
    src/event_bus.rs             broadcast channel wrapper (capacity 2048)
    src/mapping.rs               MIDI → Action resolver
    src/dispatch.rs              Action → subsystem router
    src/config.rs                TOML config + persistence
  midium-midi/
    src/parse.rs                 raw bytes → MidiEvent
    src/manager.rs               device discovery + connection
    src/profile.rs               device profile system
    src/group_manager.rs         fader group LED feedback
  midium-audio/
    src/backend.rs               AudioBackend trait (+ register_event_bus)
    src/shared.rs                SharedAudio adapter (Arc<dyn AudioBackend> → VolumeControl + DeviceLister)
    src/macos.rs                 CoreAudio backend (+ property change listeners)
    src/macos_tap.rs             macOS 14.2+ per-app tap API
    src/macos_utils.rs           shared CFString conversion helpers
    src/linux.rs                 PulseAudio backend
    src/windows.rs               WASAPI backend
  midium-plugins/
    src/runtime.rs               Lua VM + sandbox + API
  midium-shortcuts/
    src/lib.rs                   enigo key simulation
  midium-daemon/
    src/main.rs                  headless CLI entry point (clap + tracing-appender)
app/
  src-tauri/src/
    lib.rs                       setup orchestration + event bridge (~290 lines)
    state.rs                     AppState struct definition
    tray.rs                      tray icon + menu construction
    commands/
      mod.rs                     re-exports all command modules
      audio.rs                   audio query + control IPC handlers
      midi.rs                    MIDI port + learn IPC handlers
      config.rs                  config + shortcut + autostart IPC handlers
      mappings.rs                mapping CRUD + import/export IPC handlers
      fader_groups.rs            fader group CRUD IPC handlers
      profiles.rs                profile list + import/export IPC handlers
      plugins.rs                 plugin list IPC handler
    main.rs                      Tauri entry point (1 line)
  src/routes/+page.svelte        tab shell + event listeners
  src/lib/                       Svelte components
  src/lib/types.ts               TypeScript mirror of Rust types (incl. AppConfig)
  src/lib/types.test.ts          vitest unit tests for types.ts
  src/lib/stores.ts              Svelte writable stores for push-based state
```

---

## 4. Crate-by-Crate Reference

### 4.1 `midium-core`

The shared kernel. All other crates depend on it; it depends on nothing internal.

#### `types.rs`

Defines all domain types:


| Type                                                         | Purpose                                                                                                                                                                                                                                           |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `MidiEvent`                                                  | A parsed inbound MIDI event (device name, channel, message)                                                                                                                                                                                       |
| `MidiMessage`                                                | `ControlChange` / `NoteOn` / `NoteOff` / `PitchBend`                                                                                                                                                                                              |
| `ControlId`                                                  | Identifies a physical control: device + channel + `ControlType`                                                                                                                                                                                   |
| `ControlType`                                                | `CC(u8)` / `Note(u8)` / `PitchBend`                                                                                                                                                                                                               |
| `Action`                                                     | 11 variants: `SetVolume`, `ToggleMute`, `SetDefaultOutput`, `SetDefaultInput`, `CycleOutputDevices`, `CycleInputDevices`, `MediaPlayPause`, `MediaNext`, `MediaPrev`, `RunPluginAction`, `SendKeyboardShortcut`, `SendMidiMessage`, `ActionGroup` |
| `AudioTarget`                                                | `SystemMaster` / `Device { id }` / `Application { name }` / `FocusedApplication`                                                                                                                                                                  |
| `ValueTransform`                                             | `Linear` / `Logarithmic` / `RelativeEncoder { sensitivity }` / `Toggle` / `Momentary`                                                                                                                                                             |
| `FaderGroup`                                                 | Links a profile channel strip to an audio target                                                                                                                                                                                                  |
| `Mapping`                                                    | A persisted mapping: `ControlId` + `Action` + `ValueTransform`                                                                                                                                                                                    |
| `AppEvent`                                                   | 10 variants on the event bus (see §5)                                                                                                                                                                                                             |
| `AudioDeviceInfo` / `AudioSessionInfo` / `AudioCapabilities` | Audio introspection                                                                                                                                                                                                                               |


`ValueTransform::apply(raw: u8, current: f64) -> Option<f64>` normalises a raw 0–127 MIDI byte to a 0.0–1.0 float. Returns `None` to suppress the action (e.g. button release for Toggle/Momentary). The logarithmic curve is `x²` (providing more resolution at low volumes). The relative encoder uses values 1–63 as positive deltas and 65–127 as negative deltas, scaled by `sensitivity`.

#### `event_bus.rs`

```rust
pub struct EventBus {
    sender: broadcast::Sender<AppEvent>,  // capacity: 2048
}
```

All components hold a cloned `EventBus`. Publishing is fire-and-forget (`let _ = self.sender.send(event)`). Lagged subscribers receive `RecvError::Lagged(n)` and drop those events. The capacity was increased from 256 to 2048 to prevent message drops under high-throughput MIDI input combined with slow plugin subscribers.

#### `mapping.rs`

`MappingEngine` is a `HashMap<ControlId, (Action, ValueTransform)>` loaded from the persisted mappings. On each MIDI event it performs a two-phase lookup:

1. Exact key lookup (O(1))
2. Fuzzy O(n) scan: checks if the incoming device name contains the mapping's device name as a case-insensitive substring

When a match is found, it calls `transform.apply()` and publishes `AppEvent::ActionTriggered`.

A per-`ControlId` state map tracks the last transformed value, required by `RelativeEncoder` and `Toggle` transforms.

#### `dispatch.rs`

`ActionDispatcher` receives `AppEvent::ActionTriggered` from the event loop and routes actions:


| Action type                                                                      | Routed to                                                   |
| -------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `SetVolume`, `ToggleMute`                                                        | `Box<dyn VolumeControl>`                                    |
| `SetDefaultOutput`, `SetDefaultInput`, `CycleOutputDevices`, `CycleInputDevices` | `Box<dyn VolumeControl>` + optional `Box<dyn DeviceLister>` |
| `MediaPlayPause`, `MediaNext`, `MediaPrev`, `SendKeyboardShortcut`               | `Box<dyn ShortcutExecutor>`                                 |
| `SendMidiMessage`                                                                | `EventBus` (→ `AppEvent::SendMidi`)                         |
| `ActionGroup`                                                                    | Recursive dispatch                                          |
| `RunPluginAction`                                                                | No-op (handled by plugin thread's own subscription)         |


The `CycleOutputDevices`/`CycleInputDevices` actions skip when `value == 0.0` to suppress button-release events.

#### `config.rs`

Two TOML files:

- `**config.toml**` → `AppConfig` (general, midi, audio, plugins sections)
- `**mappings.toml**` → `MappingsConfig` (arrays of mappings and fader groups)

`config_dir()` is hand-rolled using `$HOME` / `$XDG_CONFIG_HOME` / `$APPDATA` env vars rather than using the `dirs` crate:


| Platform | Path                                              |
| -------- | ------------------------------------------------- |
| macOS    | `~/Library/Application Support/midium/`           |
| Linux    | `$XDG_CONFIG_HOME/midium/` or `~/.config/midium/` |
| Windows  | `%APPDATA%\midium\`                               |


---

### 4.2 `midium-midi`

#### `parse.rs`

Converts raw MIDI byte slices into `MidiEvent`. Handles:

- `0x80`: Note Off
- `0x90`: Note On (velocity 0 → treated as Note Off, per MIDI convention)
- `0xB0`: Control Change
- `0xE0`: Pitch Bend (14-bit value reconstructed as `(MSB << 7) | LSB`)
- All other status bytes (SysEx, Clock, Active Sensing, etc.) → `None`

7 unit tests cover normal cases and edge cases.

#### `manager.rs`

`MidiManager` runs an async loop that polls for MIDI ports every `poll_interval_secs` seconds using `midir`. On each scan:

1. Compares current port list against `connected_ports` set
2. Publishes `AppEvent::DeviceDisconnected` for ports that disappeared
3. For new ports: opens an input connection (raw bytes → `parse_midi` → `AppEvent::Midi`) and a best-effort output connection for LED feedback
4. LED output: a separate task drains `AppEvent::SendMidi` from the event bus and writes raw bytes to the named output port

The output connection is opened **before** publishing `AppEvent::DeviceConnected` so that LED sync triggered by the connection event can actually send data.

`MidiManager::list_ports()` is a static method that creates a transient `MidiInput` just to enumerate port names — used by the IPC `list_midi_ports` command.

#### `profile.rs`

Device profiles describe the physical layout of a MIDI controller:

```toml
name = "Korg nanoKONTROL2"
vendor = "Korg"
model = "nanoKONTROL2"
match_patterns = ["nanoKONTROL2", "nanoKontrol2"]

[[controls]]
label = "Fader 1"
control_type = "slider"
channel = 0
number = 0
group = 1
section = "Faders"

[[controls]]
label = "M1"
control_type = "button"
button_role = "mute"
channel = 0
number = 33
group = 1
section = "Buttons"
```

Five profiles are compiled into the binary via `include_str!` (`bundled_profiles()`). The filesystem loader (`load_profiles(dir)`) overlays these at runtime — a filesystem profile with the same `name` replaces the bundled one.

`match_profile(port_name, profiles)` does a case-insensitive substring search across all `match_patterns`.

#### `group_manager.rs`

`GroupManager` runs on the event bus loop and handles fader groups without going through the mapping engine:

- **Fader movement** (slider/encoder control): calls `audio.set_volume(target, transformed_value)`
- **M button press**: calls `audio.is_muted()` + `audio.set_mute(!muted)`, then sends S/M/R LEDs
- **S button**: consumed silently (S LED = always on when a group is assigned)
- **R button** (Device target only): calls `audio.set_default_output(id)`, then syncs all LEDs for the device

`resolve_group()` looks up profile controls for the group number to find which CC/Note numbers correspond to the fader, mute, solo, and record buttons. `device_matches()` uses bidirectional fuzzy matching: `actual.contains(pattern) || pattern.contains(actual)`.

LED state logic:

- S LED: always on (strip has an assignment)
- M LED: on when target is muted
- R LED: for `Device` targets → on when it is the default output; for other targets → on when not muted

---

### 4.3 `midium-audio`

#### `backend.rs`

```rust
pub trait AudioBackend: VolumeControl + Send + Sync {
    fn list_output_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>>;
    fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDeviceInfo>>;
    fn get_volume(&self, target: &AudioTarget) -> anyhow::Result<f64>;
    fn list_sessions(&self) -> anyhow::Result<Vec<AudioSessionInfo>>;
    fn capabilities(&self) -> AudioCapabilities;
    fn register_event_bus(&self, _event_bus: EventBus) {}  // default no-op
}
```

`create_backend()` selects the platform implementation at compile time via `#[cfg(target_os)]`.

#### `shared.rs` — SharedAudio adapter

`SharedAudio(pub Arc<dyn AudioBackend>)` is a newtype that implements both `VolumeControl` and `DeviceLister` by delegating to the inner `AudioBackend`. Both the Tauri app and the daemon import this single type instead of defining their own duplicate wrappers.

#### `macos_utils.rs` — CoreFoundation string helpers

Shared `cfstring_to_string(CFStringRef) -> String` and `cfstring_from_str(&str) -> CFStringRef` functions used by both `macos.rs` and `macos_tap.rs`, eliminating duplicate implementations.

#### `macos.rs` — CoreAudio backend

Uses raw `AudioObjectGetPropertyData` / `AudioObjectSetPropertyData` calls from `coreaudio-sys`. Volume is read from `kAudioDevicePropertyVolumeScalar`, trying the master channel first, then channel 1. Device switching uses `kAudioHardwarePropertyDefaultOutputDevice`. Per-app operations delegate to `AudioTapManager` when available.

Implements `register_event_bus()` to install `AudioObjectAddPropertyListener` callbacks for:

- `kAudioDevicePropertyVolumeScalar` → publishes `AppEvent::VolumeChanged`
- `kAudioDevicePropertyMute` → publishes `AppEvent::MuteChanged`
- `kAudioHardwarePropertyDefaultOutputDevice` → publishes `AppEvent::DefaultDeviceChanged`

These push-based notifications replace the need for frontend polling of audio state.

#### `macos_tap.rs` — macOS 14.2+ Audio Tap API

This is the most technically complex part of the codebase. Key implementation details:

- **Version detection**: `sysctlbyname("kern.osproductversion")` parses the OS version string to check `major > 14 || (major == 14 && minor >= 2)`.
- **Process discovery**: Uses `kAudioHardwarePropertyProcessObjectList` (FourCC `prs#`) to get `AudioObjectID` values for active audio processes, then `kAudioProcessPropertyBundleId` to get bundle IDs, then `NSRunningApplication.localizedName` via Objective-C runtime (`objc2`) to get human-readable names.
- **Tap creation**: Creates a `CATapDescription` via `[CATapDescription initStereoMixdownOfProcesses:pidArray]` using raw Objective-C `msg_send!`. Calls the private `AudioHardwareCreateProcessTap` FFI function.
- **Aggregate device**: Creates an `AudioHardwareCreateAggregateDevice` that includes the tap, using CoreFoundation dictionary construction.
- **IO proc**: Installs an `AudioDeviceCreateIOProcID` callback that multiplies all audio samples by the volume multiplier. Volume and mute state are accessed lock-free via `AtomicU64` with `f64::to_bits()` / `f64::from_bits()`.
- **Cleanup**: `TapState::drop()` stops the IO proc, destroys the aggregate device, destroys the process tap, and deallocates the `IoContext` (stored as `io_context_ptr: *mut c_void` in `TapState`, freed via `Box::from_raw`).

#### `linux.rs` — PulseAudio backend

Uses `libpulse-binding`. Each operation creates a new `PulseConn` (threaded mainloop + context), connects, performs one blocking operation, and drops the connection. Application volume uses `sink-input` streams matched by display name. Default device switching spawns `pactl set-default-sink/source`.

#### `windows.rs` — WASAPI backend

Uses `windows-rs` crate. COM is initialized on first use. Device volume via `IAudioEndpointVolume`. Per-app volume via `IAudioSessionManager2` + `ISimpleAudioVolume`, matching sessions by display name substring. Default device switching uses the undocumented `IPolicyConfig` COM interface (CLSID `870AF99C-…`) with `SetDefaultEndpointRole` called for all three roles (`eConsole`, `eMultimedia`, `eCommunications`). The `set_default_endpoint` function includes structured error handling with `tracing::warn!` logging when the undocumented COM object is unavailable, providing clear diagnostics rather than generic COM errors. `FocusedApplication` target uses `GetForegroundWindow` + `GetWindowThreadProcessId` to find the frontmost process's audio session.

---

### 4.4 `midium-plugins`

One Lua 5.4 VM per plugin file (`mlua` with vendored Lua). The VM runs on a dedicated OS thread because `mlua::Lua` is `!Send`. A `current_thread` tokio runtime on that thread allows using the async `EventBus` receiver.

**Sandbox**: removes `io`, `debug`, `loadfile`, `dofile`, `package`, `require`; replaces `os` with a safe subset (`clock`, `time`). 16 MB memory ceiling.

`**midium` API table** injected as a global:

```lua
midium.log(msg)
midium.audio.get_volume(target)    -- "master"|"focused"|"app:Name"|"device:id"
midium.audio.set_volume(target, v)
midium.audio.is_muted(target)
midium.audio.set_mute(target, muted)
midium.audio.list_sessions()       -- [{name, volume, muted}]
midium.audio.list_devices()        -- [{id, name, is_default}]
midium.state.get(key)              -- per-plugin string KV store
midium.state.set(key, value)
midium.register_action(name, desc, fn)
```

**Lifecycle hooks** (called as globals or from module return table):

```lua
function on_load() end
function on_midi_event(event) end  -- event.device, event.channel, event.message.{cc,note,pitch_bend}
function on_unload() end
```

Registered action handlers are stored in a `_action_handlers` Lua table to prevent GC. Dispatched when `AppEvent::ActionTriggered { action: RunPluginAction { plugin, action } }` matches the plugin name.

`PluginManager::spawn()` blocks for up to 5 seconds on a `SyncChannel` to receive post-`on_load` plugin info, then returns. The thread continues running the event loop independently.

---

### 4.5 `midium-shortcuts`

`ShortcutHandler` wraps `enigo::Enigo` on a dedicated OS thread (via `SyncSender<Action>`). `ShortcutExecutor::execute()` sends the action to the thread; the thread calls `enigo.key(k, Direction::Click)` for media keys or a press-all / release-all-in-reverse sequence for key combos.

`parse_key()` maps string names (`"ctrl"`, `"shift"`, `"alt"`, `"meta"`, function keys, navigation keys, single characters) to `enigo::Key`.

---

### 4.6 `midium-app` (Tauri)

The Tauri application is split across multiple modules (~870 lines total):


| File                       | Lines | Purpose                                                |
| -------------------------- | ----- | ------------------------------------------------------ |
| `lib.rs`                   | ~290  | Setup orchestration, event bridge, global shortcut     |
| `state.rs`                 | ~27   | `AppState` struct definition                           |
| `tray.rs`                  | ~130  | Tray icon and menu construction                        |
| `commands/audio.rs`        | ~78   | Audio query + control IPC handlers                     |
| `commands/midi.rs`         | ~45   | MIDI port + learn IPC handlers                         |
| `commands/config.rs`       | ~82   | Config + shortcut + autostart IPC handlers             |
| `commands/mappings.rs`     | ~90   | Mapping CRUD + import/export (with 1MB size limit)     |
| `commands/fader_groups.rs` | ~56   | Fader group CRUD (sorted insert)                       |
| `commands/profiles.rs`     | ~56   | Profile list + import/export (re-serialises on import) |
| `commands/plugins.rs`      | ~10   | Plugin list                                            |


`**AppState**` (defined in `state.rs`, managed state accessible in all IPC handlers):

```rust
pub struct AppState {
    pub event_bus: EventBus,
    pub audio: Arc<dyn AudioBackend>,
    pub mapping_engine: Arc<tokio::sync::Mutex<MappingEngine>>,
    pub dispatcher: Arc<ActionDispatcher>,
    pub mappings_config: Arc<tokio::sync::Mutex<MappingsConfig>>,
    pub app_config: Arc<tokio::sync::Mutex<AppConfig>>,
    pub current_shortcut: Arc<tokio::sync::Mutex<Option<String>>>,
    pub midi_learn_tx: Arc<tokio::sync::Mutex<Option<oneshot::Sender<MidiEvent>>>>,
    pub plugin_list: Arc<tokio::sync::Mutex<Vec<PluginInfo>>>,
    pub profiles: Arc<Vec<DeviceProfile>>,
}
```

All mutex-guarded fields use `tokio::sync::Mutex` to avoid blocking tokio worker threads. All IPC handlers that access these fields are `async fn` and use `.lock().await`.

**IPC commands** (30 total):


| Category      | Commands                                                                                                    |
| ------------- | ----------------------------------------------------------------------------------------------------------- |
| Audio query   | `get_capabilities`, `list_output_devices`, `list_input_devices`, `list_sessions`, `get_volume`, `get_muted` |
| Audio control | `set_volume`, `toggle_mute`, `set_default_output`, `set_default_input`                                      |
| MIDI          | `list_midi_ports`, `send_midi`, `start_midi_learn`, `cancel_midi_learn`                                     |
| Mappings      | `get_mappings`, `save_mapping`, `delete_mapping`, `export_mappings`, `import_mappings`                      |
| Fader groups  | `get_fader_groups`, `save_fader_group`, `delete_fader_group`                                                |
| Profiles      | `list_profiles`, `export_profile`, `import_profile`                                                         |
| Config        | `get_config`, `save_config`, `get_shortcut`, `set_shortcut`, `get_autostart`, `set_autostart`               |
| Plugins       | `list_plugins`                                                                                              |


**Setup sequence** (inside `.setup()` closure):

1. Init tracing subscriber
2. Load `AppConfig` and `MappingsConfig` from disk
3. Load profiles: bundled → filesystem overlays (`./profiles/`, `config_dir/profiles/`, `exe_dir/profiles/`)
4. Create `EventBus`
5. Create platform audio backend
6. Build `ActionDispatcher` with `SharedAudio` (bridges `Arc<dyn AudioBackend>` to `Box<dyn VolumeControl>`)
7. Create `MappingEngine`, load mappings
8. Spawn plugin system on dedicated OS thread
9. Register `AppState` with Tauri
10. Register global shortcut if configured
11. Set autostart via `tauri-plugin-autostart`
12. Build tray icon
13. Install close-to-tray window event handler
14. Spawn `MidiManager` async task
15. Spawn `GroupManager` async task
16. Spawn EventBus-to-Tauri emit bridge async task

**EventBus→Tauri bridge** (the central processing loop in `lib.rs`):

```
AppEvent::Midi      → MappingEngine.process_midi_event()
                    → MIDI Learn oneshot capture
                    → emit("midi-event")
AppEvent::ActionTriggered     → dispatcher.dispatch()
AppEvent::VolumeChanged       → emit("volume-changed")
AppEvent::MuteChanged         → emit("mute-changed")
AppEvent::DeviceConnected     → emit("device-connected")
AppEvent::DeviceDisconnected  → emit("device-disconnected")
AppEvent::DefaultDeviceChanged → emit("default-device-changed")
                               → rebuild + set tray menu (auto-refresh)
AppEvent::Shutdown / Err      → break
```

**Tray icon** (in `tray.rs`): Shows current default output device name. Submenu lists all output devices; clicking one calls `set_default_output`. Left-click on tray icon shows/focuses the main window. "Toggle Mute" item calls `audio.set_mute(SystemMaster, !muted)`. The tray menu auto-refreshes when `DefaultDeviceChanged` is received via the event bridge.

**Global shortcut** (`set_shortcut` command): Unregisters the old shortcut, registers the new one. On registration failure, restores the old shortcut. The shortcut toggles the main window's `visible` state.

---

### 4.7 `midium-daemon` (~270 lines)

Headless variant. Uses `clap` derive for argument parsing (`--config`, `--profiles`, `--help`, `--version`). Startup:

1. Parse CLI args via `Cli::parse()`
2. Load config (from explicit path or `config_dir()`)
3. Init dual-layer tracing: human-readable to stderr + structured JSON to `<config_dir>/logs/midium.log` with daily rotation via `tracing-appender`
4. Load profiles (search: `--profiles` flag → `./profiles/` → `config_dir/profiles/`)
5. Create EventBus, mapping engine, audio backend
6. Sort fader groups by group number on load
7. Build tokio multi-thread runtime
8. In async: create `ActionDispatcher` with `SharedAudio` (imported from `midium-audio`), spawn `GroupManager`, spawn `MidiManager`, run event loop
9. Wait for `Ctrl+C`, publish `AppEvent::Shutdown`, await tasks with 3s/1s timeouts

---

## 5. Event Bus Reference

All inter-component communication uses `AppEvent` variants on the shared broadcast channel (capacity 2048):


| Event                               | Publisher                                            | Subscribers                                                                                |
| ----------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `Midi(MidiEvent)`                   | `MidiManager`                                        | EventBus bridge (→ `MappingEngine`, MIDI Learn, frontend), `GroupManager`, `PluginManager` |
| `ActionTriggered { action, value }` | `MappingEngine`                                      | EventBus bridge (→ `ActionDispatcher`)                                                     |
| `VolumeChanged { target, volume }`  | `CoreAudioBackend` (via property listener)           | EventBus bridge (→ frontend `volume-changed` → Svelte stores)                              |
| `MuteChanged { target, muted }`     | `CoreAudioBackend` (via property listener)           | EventBus bridge (→ frontend `mute-changed` → Svelte stores)                                |
| `DeviceConnected { device }`        | `MidiManager`                                        | EventBus bridge, `GroupManager` (→ LED sync)                                               |
| `DeviceDisconnected { device }`     | `MidiManager`                                        | EventBus bridge                                                                            |
| `SendMidi { device, data }`         | `ActionDispatcher`, `GroupManager`, frontend via IPC | `MidiManager` SendMidi drain task                                                          |
| `DefaultDeviceChanged`              | `ActionDispatcher`, `CoreAudioBackend` (via listener) | EventBus bridge (→ tray rebuild), `GroupManager` (→ LED sync)                             |
| `GroupsChanged { groups }`          | IPC `save_fader_group` / `delete_fader_group`        | `GroupManager` (hot-reloads group config)                                                  |
| `Shutdown`                          | Signal handler                                       | All subscribers (break their loops)                                                        |


---

## 6. Data Flow: MIDI Input → Audio Control

```
MIDI Hardware
    │  raw bytes (e.g. [0xB0, 7, 100])
    ▼
MidiManager.scan_and_connect() callback
    │  parse_midi("nanoKONTROL2 MIDI 1", bytes)
    │  → MidiEvent { device, channel: 0, message: ControlChange { control: 7, value: 100 } }
    ▼
EventBus.publish(AppEvent::Midi(event))
    │
    ├─► GroupManager.handle_midi()
    │     if event matches a fader group:
    │       transform.apply(raw, fader_state) → Some(0.62)
    │       audio.set_volume(target, 0.62)
    │       → LED feedback via AppEvent::SendMidi
    │
    └─► EventBus bridge (lib.rs async task)
          MappingEngine.process_midi_event()
            exact lookup: ControlId { "nanoKONTROL2 MIDI 1", 0, CC(7) }
            (or fuzzy: "nanoKONTROL2" substring match)
            transform.apply(100, 0.0) → Some(0.62)
            EventBus.publish(ActionTriggered { SetVolume { SystemMaster }, 0.62 })

          AppEvent::ActionTriggered received:
            ActionDispatcher.dispatch(SetVolume { SystemMaster }, 0.62)
              audio.set_volume(&SystemMaster, 0.62)
                → CoreAudio: AudioObjectSetPropertyData(kAudioDevicePropertyVolumeScalar, 0.62)

          AppEvent::Midi forwarded to frontend:
            app_handle.emit("midi-event", &event)
              → +page.svelte lastMidiEvent updated
              → Devices.svelte lastValues[key] updated
```

---

## 7. Frontend Architecture

### 7.1 Routing and Layout

SvelteKit with `adapter-static`. Single route (`/`). `prerender = true`, `ssr = false`. The layout is minimal — just imports `app.css`. All navigation is client-side state in `+page.svelte`:

```svelte
let activeTab: Tab = "mixer";
// Tab = "mixer" | "groups" | "mappings" | "devices" | "plugins" | "settings"
```

### 7.2 Component Responsibilities


| Component                 | State ownership                                     | Key behaviour                                                                            |
| ------------------------- | --------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `+page.svelte`            | `activeTab`, `connectedDevices`, `lastMidiEvent`    | Global MIDI device event listeners; tab routing; `open-mapping` cross-tab navigation     |
| `Mixer.svelte`            | Subscribes to Svelte stores; local group state      | Push-based via stores; 30s fallback poll; LED sync; channel strips                       |
| `ChannelStrip.svelte`     | `localVol`, `dragging`                              | Vertical range fader; S/M/R buttons; drag-to-prevent-external-update pattern             |
| `MappingEditor.svelte`    | `mappings`, form state, learn state                 | MIDI Learn via oneshot; two-panel add form; inline delete confirmation                   |
| `FaderGroupEditor.svelte` | `faderGroups`, form state                           | Profile-aware group picker; device auto-detect via MIDI Learn                            |
| `Devices.svelte`          | `profiles`, `selectedDevice`, `lastValues`          | Real-time CC value display; click control → `open-mapping` event                         |
| `Settings.svelte`         | `config: AppConfig \| null`, shortcut state, modals | Typed config; TOML export (textarea + download); paste-to-import                         |
| `PluginManager.svelte`    | `plugins` (read-only list)                          | Display-only; plugin management via `config.toml`                                        |


### 7.3 Cross-Tab Communication

The only cross-tab data flow is from `Devices` → `MappingEditor`. When the user clicks a control in the Devices schematic, `Devices.svelte` dispatches a `"open-mapping"` custom event. `+page.svelte` handles it, sets `mappingPrefill`, and switches `activeTab` to `"mappings"`. `MappingEditor` reacts to the `bind:prefill` prop change in a `$:` reactive block.

### 7.4 State Update Patterns

The frontend uses three mechanisms to stay current:

1. **Initial load on mount** — each component calls relevant `invoke()` commands in `onMount()`
2. **Push-based Svelte stores** — `stores.ts` exports `writable` stores for `masterVolume`, `masterMuted`, `focusedVolume`, `focusedMuted`, and `sessions`. `initStoreListeners()` subscribes to Tauri `volume-changed` and `mute-changed` events (pushed by the CoreAudio property listeners) and updates the stores reactively. Components subscribe to stores via `$masterVolume` etc.
3. **30-second fallback polling** — `Mixer.svelte` runs a reduced-frequency poll as a safety net for any events missed by the push path (e.g., focused app changes)
4. **Event listeners** — `listen()` on `midi-event`, `device-connected`, `device-disconnected`, `volume-changed`, `mute-changed`, `default-device-changed`, `midi-learn-result`. Listeners are properly cleaned up via `onDestroy` with stored `UnlistenFn` references.

The `ChannelStrip` component uses a `dragging` flag to decouple the slider from parent prop updates during user interaction, preventing fights between user input and store updates.

### 7.5 CSS Architecture

All design tokens are CSS custom properties in `:root` in `app.css`:

```css
--bg: #1a1a1f       --surface: #24242c     --surface2: #2e2e38
--border: #383845   --text: #e8e8f0        --text-muted: #888899
--accent: #7c6af7   --accent-hover: #9585ff
--danger: #e05555   --success: #55c47a     --warning: #e0a055
--fader: #4a4a5a    --fader-fill: #7c6af7
--radius: 6px       --radius-lg: 10px
```

Global element resets and base styles are in `app.css`. Each component has a scoped `<style>` block. The `color-mix(in srgb, …)` function is used extensively for tinted backgrounds, requiring a modern browser (Chromium 111+ — shipped in Tauri's WebKit wrapper on macOS 13+/Windows 10+).

---

## 8. Configuration and Persistence

### 8.1 `config.toml`

```toml
[general]
autostart = true
log_level = "info"
shortcut = "CmdOrCtrl+Shift+M"

[midi]
poll_interval_secs = 2
auto_connect = true

[audio]
refresh_interval_secs = 5

[plugins]
enabled = []
plugin_dirs = []
```

### 8.2 `mappings.toml`

```toml
[[mappings]]
[mappings.control]
device = "nanoKONTROL2"
channel = 0
control_type = { CC = 0 }
[mappings.action]
SetVolume = { target = "SystemMaster" }
transform = "Logarithmic"

[[fader_groups]]
device = "nanoKONTROL2"
group = 1
target = "SystemMaster"
transform = "Logarithmic"
```

`persist_mappings()` writes atomically: creates the directory if needed, serialises the full `MappingsConfig` (mappings + fader groups), writes to the same file. There is no atomic rename — a write failure could leave the file partially written.

### 8.3 Device Profiles

```toml
name = "Korg nanoKONTROL2"
vendor = "Korg"
model = "nanoKONTROL2"
match_patterns = ["nanoKONTROL2"]

[[controls]]
label = "Fader 1"
control_type = "slider"   # slider | knob | button | encoder
midi_type = "cc"          # cc | note | pitch_bend (default: cc)
channel = 0
number = 0
min_value = 0
max_value = 127
group = 1                 # links S/M/R buttons to the fader
button_role = "solo"      # solo | mute | record (buttons only)
section = "Faders"        # UI grouping label
```

Bundled profiles (compiled into binary): `korg_nanokontrol2`, `behringer_xtouch_mini`, `akai_midimix`, `arturia_beatstep`, `generic`.

---

## 9. CI/CD

### Test Workflow (`.github/workflows/test.yml`)

Runs on push/PR to `main`/`master` across a 3-platform matrix (ubuntu, macos, windows):

1. `cargo clippy --all-targets -- -D warnings`
2. `cargo test --workspace`
3. `npx svelte-check`

Audio tests in `crates/midium-audio/tests/audio_tests.rs` skip gracefully when no audio daemon is available (`backend_or_skip!` macro). Hardware-dependent tests are marked `#[ignore]` and must be run manually.

Frontend unit tests use `vitest` (`npm test` / `vitest run`). Currently covers `types.ts` pure functions (`controlLabel`, `actionLabel`, `targetLabel`) with 15 test cases.

### PR Build Workflow (`.github/workflows/pr-build.yml`)

Builds a Windows NSIS bundle to verify the Tauri build succeeds.

### Release Workflow (`.github/workflows/release.yml`)

Produces bundles for all three platforms on tag push.

### Daemon Workflow (`.github/workflows/daemon.yml`)

Builds the daemon binary and verifies `--help` exits cleanly.

---

## 10. Notable Quirks and Implementation Details

- `**Mapping` equality** uses `ControlId` as the key. `ControlId` derives `PartialEq + Eq + Hash`. Two mappings with the same control are considered duplicates; saving one overwrites the other.
- **The `send_midi` IPC command** publishes `AppEvent::SendMidi` to the event bus rather than calling the MIDI manager directly. The MIDI manager's drain task picks it up. This means there is a small latency (one async task scheduling cycle) for LED feedback triggered from the UI.
- **Profile import** re-serialises the validated `DeviceProfile` struct before writing to disk (rather than writing raw user input), ensuring files on disk are canonical TOML. Imports are size-limited to 1MB. Profile import does not reload profiles at runtime — the import note says "Restart to apply." This is because `AppState.profiles` is `Arc<Vec<DeviceProfile>>` (immutable after setup) rather than `Arc<Mutex<Vec<…>>>`.
- **The `midi_learn_tx` oneshot** is consumed by `take()` on the first MIDI event received while active. Cancelling (`cancel_midi_learn`) simply drops the `Some(tx)`, which drops the sender; the async task waiting on `rx.await` will then return `Err(RecvError)` and exit silently.
- **Svelte's `#each` keying** — `FaderGroupEditor` uses `(g.device + g.group)` as the `#each` key, which is string concatenation and could produce collisions (e.g., device `"a"` + group `12` == device `"a1"` + group `2`). In practice, this is unlikely to cause visible bugs given the constraint that device+group pairs must be unique in config.
- `**color-mix(in srgb, …)`** usage in CSS requires Chromium 111+ / Safari 16.2+. Tauri's bundled WebKit on macOS supports this; on older Windows it may not render correctly.

