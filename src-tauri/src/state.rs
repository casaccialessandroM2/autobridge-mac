//! Shared application state, log types, and inter-task commands.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tauri::{AppHandle, Emitter};
use serde::{Deserialize, Serialize};
use chrono::Local;

use crate::config::AppConfig;

// ── Bridge commands sent from UI → connection task ─────────────────────────

#[derive(Debug, Clone)]
pub enum BridgeCommand {
    Disconnect,
    SendDiagnostic(Vec<u8>),
}

// ── Log entry (serialised to frontend via Tauri event) ─────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

impl LogEntry {
    fn new(level: &str, message: impl Into<String>) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S%.3f").to_string(),
            level: level.to_string(),
            message: message.into(),
        }
    }

    pub fn info(msg: impl Into<String>)  -> Self { Self::new("INFO",  msg) }
    pub fn warn(msg: impl Into<String>)  -> Self { Self::new("WARN",  msg) }
    pub fn error(msg: impl Into<String>) -> Self { Self::new("ERROR", msg) }
    pub fn doip(msg: impl Into<String>)  -> Self { Self::new("DOIP",  msg) }
    pub fn debug(msg: impl Into<String>) -> Self { Self::new("DEBUG", msg) }
}

// ── Shared mutable application state ──────────────────────────────────────

pub struct AppState {
    pub config:  Mutex<AppConfig>,
    pub status:  Mutex<String>,
    pub logs:    Mutex<Vec<LogEntry>>,
    /// Sender half to the active bridge task; None when disconnected.
    pub cmd_tx:  Mutex<Option<mpsc::Sender<BridgeCommand>>>,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            config:  Mutex::new(AppConfig::default()),
            status:  Mutex::new("Disconnected".to_string()),
            logs:    Mutex::new(Vec::new()),
            cmd_tx:  Mutex::new(None),
        })
    }

    /// Append a log entry and emit it to the frontend.
    pub async fn log(&self, app: &AppHandle, entry: LogEntry) {
        {
            let mut logs = self.logs.lock().await;
            if logs.len() >= 1000 {
                logs.drain(0..100);
            }
            logs.push(entry.clone());
        }
        let _ = app.emit("log_entry", &entry);
    }

    /// Overwrite status and notify the frontend.
    pub async fn set_status(&self, app: &AppHandle, status: &str) {
        *self.status.lock().await = status.to_string();
        let _ = app.emit("connection_status", status);
    }
}
