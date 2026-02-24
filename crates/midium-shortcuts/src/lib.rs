use std::sync::mpsc;
use midium_core::dispatch::ShortcutExecutor;
use midium_core::types::Action;
use tracing::{debug, warn};

/// Sends shortcut/media-key actions to a dedicated worker thread that owns
/// the platform input simulation handle (enigo::Enigo is !Send on some platforms).
pub struct ShortcutHandler {
    tx: mpsc::SyncSender<Action>,
}

impl ShortcutHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel::<Action>(32);
        std::thread::Builder::new()
            .name("midium-shortcuts".into())
            .spawn(move || shortcut_thread(rx))
            .expect("failed to spawn shortcut thread");
        Self { tx }
    }
}

impl Default for ShortcutHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutExecutor for ShortcutHandler {
    fn execute(&self, action: &Action) {
        if let Err(e) = self.tx.send(action.clone()) {
            warn!("ShortcutHandler channel send failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Worker thread
// ---------------------------------------------------------------------------

fn shortcut_thread(rx: mpsc::Receiver<Action>) {
    use enigo::{Enigo, Settings};

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to initialise enigo (shortcut execution disabled): {e}");
            // Drain the channel so senders don't block.
            while rx.recv().is_ok() {}
            return;
        }
    };

    while let Ok(action) = rx.recv() {
        dispatch_action(&mut enigo, &action);
    }
}

fn dispatch_action(enigo: &mut enigo::Enigo, action: &Action) {
    use enigo::{Direction, Key, Keyboard};

    match action {
        Action::MediaPlayPause => {
            debug!("shortcut: MediaPlayPause");
            if let Err(e) = enigo.key(Key::MediaPlayPause, Direction::Click) {
                warn!("MediaPlayPause failed: {e}");
            }
        }
        Action::MediaNext => {
            debug!("shortcut: MediaNext");
            if let Err(e) = enigo.key(Key::MediaNextTrack, Direction::Click) {
                warn!("MediaNext failed: {e}");
            }
        }
        Action::MediaPrev => {
            debug!("shortcut: MediaPrev");
            if let Err(e) = enigo.key(Key::MediaPrevTrack, Direction::Click) {
                warn!("MediaPrev failed: {e}");
            }
        }
        Action::SendKeyboardShortcut { keys } => {
            debug!(?keys, "shortcut: SendKeyboardShortcut");
            send_key_combo(enigo, keys);
        }
        Action::CycleOutputDevices | Action::CycleInputDevices => {
            // Audio device cycling requires querying the audio backend; the
            // ActionDispatcher handles these via VolumeControl once implemented.
            warn!(?action, "CycleDevices not yet implemented in shortcut handler");
        }
        other => {
            warn!(?other, "ShortcutHandler received unexpected action type");
        }
    }
}

// ---------------------------------------------------------------------------
// Key combo helper
// ---------------------------------------------------------------------------

/// Parse a list of key name strings and press them as a chord (all modifiers
/// down, key down+up, modifiers up), e.g. `["ctrl", "z"]`.
fn send_key_combo(enigo: &mut enigo::Enigo, keys: &[String]) {
    use enigo::{Direction, Key, Keyboard};

    let parsed: Vec<Key> = keys.iter().filter_map(|s| parse_key(s)).collect();
    if parsed.is_empty() {
        warn!(?keys, "send_key_combo: no parseable keys");
        return;
    }

    // Press all keys in order.
    for k in &parsed {
        if let Err(e) = enigo.key(*k, Direction::Press) {
            warn!(?k, "key press failed: {e}");
        }
    }
    // Release in reverse order.
    for k in parsed.iter().rev() {
        if let Err(e) = enigo.key(*k, Direction::Release) {
            warn!(?k, "key release failed: {e}");
        }
    }
}

fn parse_key(s: &str) -> Option<enigo::Key> {
    use enigo::Key;
    match s.to_lowercase().as_str() {
        // Modifiers
        "ctrl" | "control"  => Some(Key::Control),
        "shift"              => Some(Key::Shift),
        "alt" | "option"     => Some(Key::Alt),
        "meta" | "cmd" | "super" | "win" | "command" => Some(Key::Meta),
        // Navigation
        "return" | "enter"  => Some(Key::Return),
        "escape" | "esc"    => Some(Key::Escape),
        "tab"               => Some(Key::Tab),
        "space"             => Some(Key::Space),
        "backspace"         => Some(Key::Backspace),
        "delete"            => Some(Key::Delete),
        "home"              => Some(Key::Home),
        "end"               => Some(Key::End),
        "pageup"            => Some(Key::PageUp),
        "pagedown"          => Some(Key::PageDown),
        "left"              => Some(Key::LeftArrow),
        "right"             => Some(Key::RightArrow),
        "up"                => Some(Key::UpArrow),
        "down"              => Some(Key::DownArrow),
        // Function keys
        "f1"  => Some(Key::F1),  "f2"  => Some(Key::F2),  "f3"  => Some(Key::F3),
        "f4"  => Some(Key::F4),  "f5"  => Some(Key::F5),  "f6"  => Some(Key::F6),
        "f7"  => Some(Key::F7),  "f8"  => Some(Key::F8),  "f9"  => Some(Key::F9),
        "f10" => Some(Key::F10), "f11" => Some(Key::F11), "f12" => Some(Key::F12),
        // Single printable characters
        s if s.len() == 1 => {
            s.chars().next().map(Key::Unicode)
        }
        other => {
            warn!("Unknown key name: {other}");
            None
        }
    }
}
