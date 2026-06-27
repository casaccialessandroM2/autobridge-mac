//! AutoBridge Mac — Tauri library crate root.
//!
//! Declares submodules, exposes Tauri commands, and provides the `run()`
//! entry point called by `main.rs`.

pub mod config;
pub mod enet_bridge;
pub mod state;
pub mod tunnel;

use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::mpsc;

use config::{list_interfaces, AppConfig, InterfaceInfo};
use state::{AppState, BridgeCommand, LogEntry};

// ── Tauri commands ─────────────────────────────────────────────────────────

/// List available (non-loopback) Ethernet/Wi-Fi interfaces with IPv4 addresses.
#[tauri::command]
async fn get_interfaces() -> Result<Vec<InterfaceInfo>, String> {
    list_interfaces()
}

/// Return the current connection status string ("Disconnected", "Connecting",
/// or "Connected").
#[tauri::command]
async fn get_status(state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state.status.lock().await.clone())
}

/// Return all accumulated log entries (up to 1000 entries rolling buffer).
#[tauri::command]
async fn get_logs(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<state::LogEntry>, String> {
    Ok(state.logs.lock().await.clone())
}

/// Overwrite the stored app configuration.
#[tauri::command]
async fn update_config(
    config: AppConfig,
    state:  tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    *state.config.lock().await = config;
    Ok(())
}

/// Establish a DoIP TCP connection using the currently stored configuration.
///
/// Returns an error immediately if already connecting/connected, or if the
/// remote IP is empty.  Otherwise spawns the bridge task and returns `Ok(())`.
/// Status updates are pushed to the frontend via Tauri events.
#[tauri::command]
async fn connect(
    app:   AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // Guard against double-connect
    let current = state.status.lock().await.clone();
    if current == "Connected" || current == "Connecting" {
        return Err(format!("Already {current}"));
    }

    let config = state.config.lock().await.clone();
    if config.remote_ip.trim().is_empty() {
        return Err("Remote IP address is required".to_string());
    }

    // Command channel: UI → bridge task
    let (cmd_tx, cmd_rx) = mpsc::channel::<BridgeCommand>(32);
    *state.cmd_tx.lock().await = Some(cmd_tx);

    state.set_status(&app, "Connecting").await;
    state.log(&app, LogEntry::info(format!(
        "Initiating connection to {}:{}", config.remote_ip, config.remote_port
    ))).await;

    // Clone Arc so the spawned task owns it independently
    let state_clone = (*state).clone();
    let app_clone   = app.clone();

    tokio::spawn(async move {
        match enet_bridge::run_bridge(
            app_clone.clone(),
            state_clone.clone(),
            config,
            cmd_rx,
        ).await {
            Ok(()) => {}
            Err(e) => {
                state_clone
                    .log(&app_clone, LogEntry::error(format!("Bridge error: {e}")))
                    .await;
                state_clone.set_status(&app_clone, "Disconnected").await;
            }
        }
        // Clear sender — future disconnect() calls will get "Not connected"
        *state_clone.cmd_tx.lock().await = None;
    });

    Ok(())
}

/// Send a Disconnect command to the active bridge task.
///
/// Returns an error if there is no active connection.
#[tauri::command]
async fn disconnect(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    let maybe_tx = state.cmd_tx.lock().await.clone();
    match maybe_tx {
        Some(tx) => {
            tx.send(BridgeCommand::Disconnect)
                .await
                .map_err(|_| "Command channel already closed".to_string())
        }
        None => Err("Not connected".to_string()),
    }
}

// ── Tauri application entry point ──────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_interfaces,
            get_status,
            get_logs,
            update_config,
            connect,
            disconnect,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AutoBridge Mac");
}
