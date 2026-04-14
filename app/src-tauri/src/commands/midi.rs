use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;
use tracing::info;

use midium_core::types::MidiEvent;
use midium_midi::manager::MidiManager;

use crate::state::AppState;

#[tauri::command]
pub fn list_midi_ports() -> Vec<String> {
    MidiManager::list_ports()
}

#[tauri::command]
pub fn send_midi(state: State<AppState>, device: String, data: Vec<u8>) -> Result<(), String> {
    state
        .event_bus
        .publish(midium_core::types::AppEvent::SendMidi { device, data });
    Ok(())
}

#[tauri::command]
pub async fn start_midi_learn(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let (tx, rx) = oneshot::channel::<MidiEvent>();
    *state.midi_learn_tx.lock().await = Some(tx);
    info!("MIDI Learn activated");

    tauri::async_runtime::spawn(async move {
        if let Ok(event) = rx.await {
            info!(device = %event.device, "MIDI Learn captured event");
            let _ = app_handle.emit("midi-learn-result", &event);
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn cancel_midi_learn(state: State<'_, AppState>) -> Result<(), String> {
    *state.midi_learn_tx.lock().await = None;
    Ok(())
}
