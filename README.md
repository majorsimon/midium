# Midium

A cross-platform desktop app for controlling system audio with physical MIDI controllers. Map any MIDI knob, fader, button, or encoder to volume, mute, media keys, or device switching — with real-time visual feedback and LED light control for supported hardware.

Built with Rust + Tauri v2 + Svelte.

---

## Features

- **Mixer view** — visual channel strips derived from your MIDI mappings; faders, mute buttons, and active indicators update in real-time
- **Device browser** — schematic view of known controllers; click any control to pre-fill the mapping form
- **MIDI Learn** — press a physical control to capture it, then assign an action in a few clicks
- **Actions**: Set Volume, Toggle Mute, Set Default Output/Input, Play/Pause, Next/Prev Track, arbitrary keyboard shortcuts
- **Multi-target groups** — map a single fader to multiple apps or devices simultaneously
- **Value transforms** — Linear, Logarithmic, Relative Encoder, Toggle, Momentary
- **LED feedback** — lights up S/M/R buttons on supported controllers (nanoKONTROL2, X-Touch Mini) to reflect mute state and active status
- **Lua plugin system** — extend with scripts; full audio/MIDI/shortcut API
- **Export / Import** — share mappings and device profiles as TOML files
- **Bundled device profiles** — Korg nanoKONTROL2, Behringer X-Touch Mini, Akai MIDImix, Arturia BeatStep, Generic fallback
- **System tray** — runs in the background; click to show/hide

---

## Screenshots

> _Add screenshots here once the UI is stable._

---

## Requirements

| Platform | Audio | MIDI |
|----------|-------|------|
| macOS 12+ | CoreAudio (master + device); Audio Tap API (per-app, 14.2+) | CoreMIDI via midir |
| Linux | PulseAudio / PipeWire | ALSA sequencer via midir |
| Windows 10+ | WASAPI | WinMM via midir |

Per-application volume is supported on **macOS 14.2+** (Audio Tap API), **Linux** (PulseAudio sink inputs), and **Windows** (WASAPI session manager). On macOS < 14.2, only master and device-level control is available.

---

## Getting Started

### Install from release

Download the latest release for your platform from the [Releases](../../releases) page:

- **macOS**: `.dmg`
- **Linux**: `.AppImage` or `.deb`
- **Windows**: `.msi`

### Build from source

Prerequisites: [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 18+, and platform audio/MIDI libraries (see below).

```bash
# Clone
git clone https://github.com/majorsimon/midium
cd midium

# Install frontend deps
cd app && npm install && cd ..

# Run in development
cd app && npm run tauri dev

# Build a release bundle
cd app && npm run tauri build
```

**Platform build deps:**

- macOS: Xcode Command Line Tools
- Linux: `libasound2-dev libpulse-dev libgtk-3-dev libwebkit2gtk-4.1-dev`
- Windows: Visual Studio Build Tools

---

## Configuration

Config files are stored in the platform config directory:

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/midium/` |
| Linux | `~/.config/midium/` |
| Windows | `%APPDATA%\midium\` |

### `config.toml`

```toml
[general]
log_level = "info"        # error | warn | info | debug | trace

[midi]
poll_interval_secs = 2    # how often to scan for new devices
auto_connect = true

[audio]
refresh_interval_secs = 5 # how often to poll session volume state
```

### `mappings.toml`

Mappings are managed via the GUI but can also be hand-edited:

```toml
# Map a fader (CC 0, channel 0) to master volume with a log curve
[[mappings]]
transform = "Logarithmic"
action = { SetVolume = { target = "SystemMaster" } }

[mappings.control]
device = "nanoKONTROL2 MIDI 1"
channel = 0
control_type = { CC = 0 }

# Map a button (Note 48) to toggle mute — Toggle fires only on press
[[mappings]]
transform = "Toggle"
action = { ToggleMute = { target = "SystemMaster" } }

[mappings.control]
device = "nanoKONTROL2 MIDI 1"
channel = 0
control_type = { Note = 48 }

# Map a fader to two apps simultaneously (ActionGroup)
[[mappings]]
transform = "Logarithmic"

[mappings.control]
device = "nanoKONTROL2 MIDI 1"
channel = 0
control_type = { CC = 1 }

[mappings.action.ActionGroup]
actions = [
  { SetVolume = { target = { Application = { name = "Spotify" } } } },
  { SetVolume = { target = { Application = { name = "Music" } } } },
]
```

#### Audio targets

| TOML | Description |
|------|-------------|
| `"SystemMaster"` | System master output volume |
| `"FocusedApplication"` | Frontmost application |
| `{ Application = { name = "Spotify" } }` | Specific app by name |
| `{ Device = { id = "123456" } }` | Audio device by CoreAudio/WASAPI ID |

#### Actions

| Action | Description |
|--------|-------------|
| `{ SetVolume = { target = … } }` | Set volume for a target (0–1) |
| `{ ToggleMute = { target = … } }` | Toggle mute for a target |
| `{ SetDefaultOutput = { device_id = "…" } }` | Switch default output device |
| `{ SetDefaultInput = { device_id = "…" } }` | Switch default input device |
| `"MediaPlayPause"` | Play / pause |
| `"MediaNext"` | Next track |
| `"MediaPrev"` | Previous track |
| `{ SendKeyboardShortcut = { keys = ["ctrl","z"] } }` | Send a key combination |
| `{ ActionGroup = { actions = […] } }` | Execute multiple actions at once |

#### Value transforms

| Transform | Best for |
|-----------|----------|
| `"Linear"` | Any control; raw 0–127 → 0.0–1.0 |
| `"Logarithmic"` | Volume faders; more resolution at low levels |
| `{ RelativeEncoder = { sensitivity = 0.01 } }` | Endless rotary encoders |
| `"Toggle"` | Buttons that should latch on/off |
| `"Momentary"` | Buttons that fire once per press |

---

## Device Profiles

Profiles describe a controller's physical layout and MIDI map. They are used for:
- **LED feedback** — lighting up mute/solo/record buttons to reflect mixer state
- **Device Browser** — schematic view with click-to-map

Bundled profiles (compiled into the binary):

| File | Device |
|------|--------|
| `profiles/korg_nanokontrol2.toml` | Korg nanoKONTROL2 |
| `profiles/behringer_xtouch_mini.toml` | Behringer X-Touch Mini |
| `profiles/akai_midimix.toml` | Akai MIDImix |
| `profiles/arturia_beatstep.toml` | Arturia BeatStep |
| `profiles/generic.toml` | Generic fallback (no controls defined) |

### Profile format

```toml
name = "My Controller"
vendor = "Acme"
model = "Faderboard Pro"
match_patterns = ["faderboard", "acme fb"]   # case-insensitive substrings

[[controls]]
label = "Fader 1"
control_type = "slider"     # slider | knob | encoder | button
midi_type = "cc"            # cc (default) | note | pitch_bend
channel = 0
number = 0                  # CC number or note number
section = "Faders"          # UI grouping label
group = 1                   # links related controls on same channel strip

[[controls]]
label = "Mute 1"
control_type = "button"
midi_type = "note"
channel = 0
number = 48
button_role = "mute"        # solo | mute | record — used for LED feedback
group = 1
section = "Buttons"
```

### Custom profiles

Place a TOML file matching the format above in:
- `<config_dir>/profiles/` — persists across updates
- `profiles/` next to the binary — useful for development

Or import via **Settings → Import → Device Profile**.

Filesystem profiles override bundled profiles with the same `name` field.

---

## Lua Plugins

Drop `.lua` files into `<config_dir>/plugins/` and enable them in **Settings → Plugins**.

### API reference

```lua
-- Logging
midium.log("message")                               -- logs at info level with plugin name

-- Audio
-- Target strings: "master", "system", "focused", "app:<name>", "device:<id>"
midium.audio.get_volume("master")                   -- returns 0.0–1.0
midium.audio.set_volume("master", 0.8)
midium.audio.is_muted("master")                     -- returns bool
midium.audio.set_mute("master", true)
midium.audio.list_devices()                         -- returns table of {id, name, is_default}
midium.audio.list_sessions()                        -- returns table of {name, volume, muted}

-- State (per-plugin key/value store, strings only)
midium.state.set("key", "value")
midium.state.get("key")                             -- returns string or nil

-- Register a custom action (appears in the Mappings action dropdown)
midium.register_action("my_action", "Description", function(value)
  midium.log("triggered with value " .. value)
end)

-- Lifecycle hooks (define as globals or return in a module table)
function on_load()   end                            -- called once after plugin loads
function on_unload() end                            -- called on shutdown
function on_midi_event(event) end                   -- event: { device, channel, message }
```

### Example plugin

```lua
-- mute_toggle_log.lua
-- Logs a message whenever a MIDI note-on arrives and toggles system mute.

function on_load()
  midium.log("mute_toggle_log plugin loaded")
end

function on_midi_event(event)
  if event.message.note and event.message.note.on then
    local muted = midium.audio.is_muted("master")
    midium.audio.set_mute("master", not muted)
    midium.log("Toggled system mute → " .. tostring(not muted))
  end
end
```

---

## Architecture

```
MIDI Device
    ↓ midir
MidiManager  ──────────────── EventBus (tokio broadcast)
    ↓                              ↓               ↓
MappingEngine              PluginManager     Tauri frontend
    ↓                                              ↓
ActionDispatcher ──────────────────────────── IPC commands
    ↓                ↓
AudioBackend   ShortcutExecutor
(CoreAudio /   (enigo)
 PulseAudio /
 WASAPI)
```

### Crate structure

| Crate | Role |
|-------|------|
| `midium-core` | Shared types, EventBus, MappingEngine, ActionDispatcher, VolumeControl/ShortcutExecutor traits |
| `midium-midi` | Device discovery, connection management, MIDI parsing, device profiles, LED feedback |
| `midium-audio` | AudioBackend trait + CoreAudio (macOS), PulseAudio (Linux), WASAPI (Windows) implementations |
| `midium-shortcuts` | Media key and keyboard shortcut execution via `enigo`; owns a worker thread |
| `midium-plugins` | Lua 5.4 runtime via `mlua`; plugin loading, sandboxing, API surface |
| `midium-daemon` | Headless CLI runner (no GUI) for server/embedded use |
| `app/src-tauri` | Tauri application: IPC commands, system tray, app state wiring |

---

## Export and Import

### Mappings

**Export**: Settings → Export → Export mappings.toml — downloads the current `mappings.toml` as a file, or copy to clipboard.

**Import**: Settings → Import → Import mappings… — paste TOML content and click Import. This replaces the current mappings entirely.

### Device profiles

**Export**: Settings → Export → click any profile name — downloads the profile TOML.

**Import**: Settings → Import → Import profile… — paste TOML content. The profile is saved to `<config_dir>/profiles/` and takes effect on next app launch.

---

## Headless daemon

For use without a GUI (e.g. on a home server or Raspberry Pi):

```bash
cargo run --bin midium -- --config path/to/config-dir/ --profiles path/to/profiles-dir/
```

The daemon loads `config.toml` and `mappings.toml` from the given config directory (or the platform default) and runs the full MIDI→audio pipeline without opening any window. Use `--help` for all options.

---

## CI/CD & Releases

Automated builds and releases are handled via GitHub Actions. Three workflows run on every push/PR:

1. **Test** (`.github/workflows/test.yml`) — runs on all platforms
   - `cargo test --workspace`
   - `svelte-check`
   - **Triggers**: push to `main`/`master`, all PRs

2. **Daemon Build** (`.github/workflows/daemon.yml`) — validates headless binary
   - `cargo build --bin midium-daemon --release`
   - Runs on macOS, Linux, Windows
   - **Triggers**: push to `main`/`master`, all PRs

3. **Release** (`.github/workflows/release.yml`) — builds and bundles for distribution
   - Runs on all platforms in parallel
   - Builds macOS: `.dmg` + `.app`
   - Builds Linux: `.AppImage` + `.deb`
   - Builds Windows: `.msi` + `.exe`
   - **Triggers**: tag push (e.g., `git tag v0.1.0 && git push --tags`)
   - Creates GitHub Release with all platform artifacts

### Creating a release

```bash
# Bump version in Cargo.toml and app/src-tauri/tauri.conf.json
# Commit changes
git add Cargo.toml app/src-tauri/tauri.conf.json
git commit -m "chore: bump version to v0.1.0"

# Create tag and push
git tag v0.1.0
git push origin main --tags
```

GitHub Actions will automatically build, package, and create a GitHub Release with all installers attached.

---

## Contributing

Contributions are welcome. A few notes:

- New device profiles are especially valued — copy an existing TOML from `profiles/` and submit a PR
- The `profiles/` directory contains community-maintained mappings; the `crates/` directory contains core logic
- Run `cargo test` and `cd app && npx svelte-check` before submitting
- Avoid adding platform-specific code outside `midium-audio` and `midium-shortcuts`

### Adding a device profile

1. Create `profiles/<vendor>_<model>.toml`
2. Set `match_patterns` to substrings of the port name your device reports (run with `RUST_LOG=debug` to see port names)
3. Add `[[controls]]` entries for all controls with accurate `channel`, `number`, and `control_type`
4. For controllers with S/M/R buttons grouped per channel strip, set `group` and `button_role` for LED feedback to work
5. Open a PR — profiles are bundled into the binary so users benefit immediately

---

## License

MIT
