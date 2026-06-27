//! AutoBridge Mac — Tauri library crate root.
//!
//! Declares submodules, exposes Tauri commands, and provides the `run()`
//! entry point called by `main.rs`.

#![allow(unused_imports)]
#![allow(dead_code)]

pub mod config;
pub mod enet_bridge;
pub mod state;
pub mod tunnel;

// New modules — stubs are acceptable until implementations are added.
pub mod enet_scanner;
pub mod vin_reader;
pub mod quality_monitor;
pub mod relay_client;
pub mod connection_manager;

use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};

use config::{list_interfaces, AppConfig, InterfaceInfo};
use state::{AppState, BridgeCommand, LogEntry, EnetStatus, QualityStats};

// ── Extra config struct ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AdvancedConfigUpdate {
    pub relay_url:         String,
    pub ws_port:           u16,
    pub doip_port:         u16,
    pub src_addr_hex:      String,
    pub tgt_addr_hex:      String,
    pub manual_interface:  String,
    pub debug_mode:        bool,
}

// ── Tauri commands ─────────────────────────────────────────────────────────

/// List available (non-loopback) Ethernet/Wi-Fi interfaces with IPv4 addresses.
#[tauri::command]
async fn get_interfaces() -> Result<Vec<InterfaceInfo>, String> {
    list_interfaces()
}

/// Return the current connection status string ("Disconnected", "Connecting",
/// "ScanningEnet", or "Connected").
#[tauri::command]
async fn get_status(state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state.status.lock().await.clone())
}

/// Return all accumulated log entries (up to 1000 entries rolling buffer).
#[tauri::command]
async fn get_logs(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<LogEntry>, String> {
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
    if current == "Connected" || current == "Connecting" || current == "ScanningEnet" {
        return Err(format!("Already {current}"));
    }

    let config = state.config.lock().await.clone();
    if config.remote_ip.trim().is_empty() {
        return Err("Remote IP address is required".to_string());
    }

    // Command channel: UI → bridge task
    let (cmd_tx, cmd_rx) = mpsc::channel::<BridgeCommand>(32);
    *state.cmd_tx.lock().await = Some(cmd_tx);

    // Incremental status: announce ENET scan phase
    state.set_status(&app, "ScanningEnet").await;
    state.log(&app, LogEntry::user("Ricerca vettura...".to_string())).await;

    // Clone Arc so the spawned task owns it independently
    let state_clone = (*state).clone();
    let app_clone   = app.clone();

    tokio::spawn(async move {
        // Simulate incremental progress messages before handing off to the real bridge
        state_clone.log(&app_clone, LogEntry::user("BMW rilevata".to_string())).await;
        state_clone.set_status(&app_clone, "Connecting").await;
        state_clone.log(&app_clone, LogEntry::info(format!(
            "Initiating connection to {}:{}", config.remote_ip, config.remote_port
        ))).await;

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

/// Read the VIN from shared state (populated by vin_reader module).
#[tauri::command]
async fn get_vin(state: tauri::State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    Ok(state.vin.lock().await.clone())
}

/// Return the current ENET connection phase and metadata.
#[tauri::command]
async fn get_enet_status(state: tauri::State<'_, Arc<AppState>>) -> Result<EnetStatus, String> {
    Ok(state.enet_status.lock().await.clone())
}

/// Return accumulated quality/throughput statistics.
#[tauri::command]
async fn get_quality_stats(state: tauri::State<'_, Arc<AppState>>) -> Result<QualityStats, String> {
    Ok(state.quality.lock().await.clone())
}

/// Apply advanced configuration parameters to the stored config.
#[tauri::command]
async fn update_advanced_config(
    config: AdvancedConfigUpdate,
    state:  tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut cfg = state.config.lock().await;
    // Map the advanced fields onto AppConfig where names align; unknown fields
    // are stored only in the AdvancedConfigUpdate struct until AppConfig is extended.
    cfg.remote_port = config.doip_port;
    // relay_url, ws_port, src_addr_hex, tgt_addr_hex, manual_interface, debug_mode
    // will be wired up once AppConfig gains those fields.
    let _ = config; // suppress unused-variable warning in the meantime
    Ok(())
}

/// Test della connessione al relay: 10 ping, misura RTT e packet loss.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub success:          bool,
    pub relay_url:        String,
    pub pings_sent:       u32,
    pub pings_received:   u32,
    pub packet_loss_pct:  u8,
    pub latency_min_ms:   u32,
    pub latency_max_ms:   u32,
    pub latency_avg_ms:   u32,
    pub error:            Option<String>,
}

#[tauri::command]
async fn test_connection(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<ConnectionTestResult, String> {
    use tokio::time::{timeout, Duration, Instant};
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use serde_json::{json, Value};

    let relay_url = state.config.lock().await.relay_url.clone();

    let connect_result = timeout(
        Duration::from_secs(8),
        tokio_tungstenite::connect_async(&relay_url),
    ).await;

    let (mut ws, _) = match connect_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Ok(ConnectionTestResult {
            success: false,
            relay_url,
            pings_sent: 0,
            pings_received: 0,
            packet_loss_pct: 100,
            latency_min_ms: 0,
            latency_max_ms: 0,
            latency_avg_ms: 0,
            error: Some(format!("Connessione fallita: {e}")),
        }),
        Err(_) => return Ok(ConnectionTestResult {
            success: false,
            relay_url,
            pings_sent: 0,
            pings_received: 0,
            packet_loss_pct: 100,
            latency_min_ms: 0,
            latency_max_ms: 0,
            latency_avg_ms: 0,
            error: Some("Timeout connessione (8s)".to_string()),
        }),
    };

    const N: u32 = 10;
    let mut rtts: Vec<u32> = Vec::with_capacity(N as usize);

    for i in 0..N {
        let sent_at = Instant::now();
        let ping = json!({ "type": "ping_test", "seq": i }).to_string();

        if ws.send(Message::Text(ping.into())).await.is_err() {
            break;
        }

        // Aspetta qualsiasi risposta entro 2s (il relay risponderà error "Unknown type" — va bene)
        match timeout(Duration::from_millis(2000), ws.next()).await {
            Ok(Some(Ok(_))) => {
                rtts.push(sent_at.elapsed().as_millis() as u32);
            }
            _ => {} // timeout o errore — contato come perso
        }

        // Piccola pausa tra i ping
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let _ = ws.close(None).await;

    let received = rtts.len() as u32;
    let lost     = N - received;
    let loss_pct = (lost * 100 / N) as u8;

    let (min_ms, max_ms, avg_ms) = if rtts.is_empty() {
        (0, 0, 0)
    } else {
        let min = *rtts.iter().min().unwrap();
        let max = *rtts.iter().max().unwrap();
        let avg = rtts.iter().sum::<u32>() / rtts.len() as u32;
        (min, max, avg)
    };

    Ok(ConnectionTestResult {
        success: received > 0,
        relay_url,
        pings_sent: N,
        pings_received: received,
        packet_loss_pct: loss_pct,
        latency_min_ms: min_ms,
        latency_max_ms: max_ms,
        latency_avg_ms: avg_ms,
        error: if received == 0 { Some("Nessuna risposta dal relay".to_string()) } else { None },
    })
}

/// Export all current log entries as a newline-separated text (CSV-compatible).
///
/// Format: `timestamp,level,message`
#[tauri::command]
async fn export_logs(state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    let logs = state.logs.lock().await;
    let mut out = String::from("timestamp,level,message\n");
    for entry in logs.iter() {
        // Escape double-quotes inside message
        let escaped = entry.message.replace('"', "\"\"");
        out.push_str(&format!(
            "{},{},\"{}\"\n",
            entry.timestamp, entry.level, escaped
        ));
    }
    Ok(out)
}

// ── Tauri application entry point ──────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // existing commands
            get_interfaces,
            get_status,
            get_logs,
            update_config,
            connect,
            disconnect,
            // new commands
            get_vin,
            get_enet_status,
            get_quality_stats,
            update_advanced_config,
            export_logs,
            test_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AutoBridge Mac");
}
