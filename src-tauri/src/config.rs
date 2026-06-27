//! Application configuration and Ethernet interface listing.

use serde::{Deserialize, Serialize};

// ── Interface info (returned to frontend) ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub description: String,
    pub ip_addresses: Vec<String>,
    pub is_up: bool,
}

// ── App configuration ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Selected Ethernet interface name (e.g. "en0")
    pub interface_name: String,
    /// Remote DoIP server IP address
    pub remote_ip: String,
    /// Remote DoIP TCP port (default: 13400)
    pub remote_port: u16,
    /// Optional session identifier for logging
    pub session_code: String,
    /// Local WebSocket tunnel listen port (default: 8765)
    pub local_ws_port: u16,
    /// DoIP source (tester) logical address  (default: 0x0E00)
    pub doip_source_address: u16,
    /// DoIP target (ECU) logical address      (default: 0x0001)
    pub doip_target_address: u16,
    /// Relay server URL (impostato via Advanced Settings o env RELAY_URL)
    pub relay_url: String,
    /// Debug mode — abilita log verbose
    pub debug_mode: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            interface_name:      String::new(),
            remote_ip:           String::new(),
            remote_port:         13400,
            session_code:        String::new(),
            local_ws_port:       8765,
            doip_source_address: 0x0E00,
            doip_target_address: 0x0001,
            relay_url:           std::env::var("RELAY_URL")
                                    .unwrap_or_else(|_| "wss://autobridge-relay-production.up.railway.app".to_string()),
            debug_mode:          false,
        }
    }
}

// ── Interface enumeration ─────────────────────────────────────────────────

pub fn list_interfaces() -> Result<Vec<InterfaceInfo>, String> {
    let raw = if_addrs::get_if_addrs()
        .map_err(|e| format!("Cannot enumerate interfaces: {e}"))?;

    let mut seen = std::collections::HashSet::<String>::new();
    let mut result: Vec<InterfaceInfo> = Vec::new();

    for iface in raw {
        if iface.is_loopback() {
            continue;
        }

        // Only IPv4 addresses
        let ip = match &iface.addr {
            if_addrs::IfAddr::V4(v4) => v4.ip.to_string(),
            _ => continue,
        };

        if let Some(entry) = result.iter_mut().find(|e| e.name == iface.name) {
            entry.ip_addresses.push(ip);
        } else {
            seen.insert(iface.name.clone());
            result.push(InterfaceInfo {
                description:  iface.name.clone(),
                name:         iface.name,
                ip_addresses: vec![ip],
                is_up:        true, // if_addrs lists only active ifaces
            });
        }
    }

    Ok(result)
}
