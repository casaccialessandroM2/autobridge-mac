//! ENET / DoIP bridge — ISO 13400-2 TCP implementation.
//!
//! Architecture
//! ───────────
//!   [UI commands]  ──cmd_rx──┐
//!   [WS tunnel data]──tun_rx─┤
//!                             ├──► main select! loop ──► TCP writer half
//!   [TCP reader task] ───────┘
//!
//! A dedicated Tokio task reads raw DoIP frames from TCP and forwards them
//! through a channel, avoiding cancellation-safety issues in select!.

use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;
use tauri::AppHandle;

use crate::config::AppConfig;
use crate::state::{AppState, BridgeCommand, LogEntry};
use crate::tunnel;

// ── DoIP protocol constants ────────────────────────────────────────────────

const DOIP_VERSION:     u8  = 0x02; // ISO 13400-2:2019
const DOIP_INV_VERSION: u8  = 0xFD;
const HEADER_LEN:       usize = 8;
const MAX_PAYLOAD:      usize = 1 << 20; // 1 MiB sanity limit

// Payload types
const PT_ROUTING_ACT_REQ:  u16 = 0x0005;
const PT_ROUTING_ACT_RES:  u16 = 0x0006;
const PT_ALIVE_CHECK_REQ:  u16 = 0x0007;
const PT_ALIVE_CHECK_RES:  u16 = 0x0008;
const PT_DIAG_MSG:         u16 = 0x4001;
const PT_DIAG_POS_ACK:     u16 = 0x4002;
const PT_DIAG_NEG_ACK:     u16 = 0x4003;

// Routing activation response codes
const RC_ROUTING_OK:            u8 = 0x10;
const RC_ROUTING_ALREADY_ACTIVE:u8 = 0x11;

// ── Frame types ────────────────────────────────────────────────────────────

struct DoipFrame {
    payload_type: u16,
    payload:      Vec<u8>,
}

// ── Frame builders ─────────────────────────────────────────────────────────

fn make_header(payload_type: u16, payload_len: usize) -> [u8; 8] {
    let mut h = [0u8; 8];
    h[0] = DOIP_VERSION;
    h[1] = DOIP_INV_VERSION;
    h[2..4].copy_from_slice(&payload_type.to_be_bytes());
    h[4..8].copy_from_slice(&(payload_len as u32).to_be_bytes());
    h
}

/// Routing Activation Request (type 0x0005, default activation).
fn build_routing_activation(src_addr: u16) -> Vec<u8> {
    // Payload: src_addr (2) + activation_type (1) + reserved (4)
    let mut payload = Vec::with_capacity(7);
    payload.extend_from_slice(&src_addr.to_be_bytes());
    payload.push(0x00); // Default activation type
    payload.extend_from_slice(&[0u8; 4]); // Reserved
    let mut msg = Vec::with_capacity(HEADER_LEN + payload.len());
    msg.extend_from_slice(&make_header(PT_ROUTING_ACT_REQ, payload.len()));
    msg.extend_from_slice(&payload);
    msg
}

/// Alive Check Response (type 0x0008, empty payload).
fn build_alive_check_response() -> Vec<u8> {
    make_header(PT_ALIVE_CHECK_RES, 0).to_vec()
}

/// Diagnostic Message (type 0x4001).
fn build_diagnostic(src: u16, tgt: u16, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4 + data.len());
    payload.extend_from_slice(&src.to_be_bytes());
    payload.extend_from_slice(&tgt.to_be_bytes());
    payload.extend_from_slice(data);
    let mut msg = Vec::with_capacity(HEADER_LEN + payload.len());
    msg.extend_from_slice(&make_header(PT_DIAG_MSG, payload.len()));
    msg.extend_from_slice(&payload);
    msg
}

// ── Frame reader (runs in its own task) ───────────────────────────────────

async fn reader_task(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    frame_tx: mpsc::Sender<Result<DoipFrame, String>>,
) {
    loop {
        // Read 8-byte header
        let mut header = [0u8; HEADER_LEN];
        if let Err(e) = reader.read_exact(&mut header).await {
            let _ = frame_tx.send(Err(format!("TCP read error: {e}"))).await;
            return;
        }

        let payload_type = u16::from_be_bytes([header[2], header[3]]);
        let payload_len  = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

        if payload_len > MAX_PAYLOAD {
            let _ = frame_tx
                .send(Err(format!("DoIP payload too large: {payload_len} bytes")))
                .await;
            return;
        }

        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            if let Err(e) = reader.read_exact(&mut payload).await {
                let _ = frame_tx.send(Err(format!("TCP payload read error: {e}"))).await;
                return;
            }
        }

        if frame_tx
            .send(Ok(DoipFrame { payload_type, payload }))
            .await
            .is_err()
        {
            // Receiver dropped; bridge is shutting down
            return;
        }
    }
}

// ── Public bridge entry point ──────────────────────────────────────────────

/// Connect to a DoIP server, exchange Routing Activation, and enter the
/// forwarding loop.  Runs until a Disconnect command is received or an
/// unrecoverable error occurs.
pub async fn run_bridge(
    app:     AppHandle,
    state:   Arc<AppState>,
    config:  AppConfig,
    mut cmd_rx: mpsc::Receiver<BridgeCommand>,
) -> Result<(), String> {

    let addr = format!("{}:{}", config.remote_ip, config.remote_port);

    state.log(&app, LogEntry::info(format!(
        "Connecting to {addr}  [iface: {}  session: {}]",
        if config.interface_name.is_empty() { "auto" } else { &config.interface_name },
        if config.session_code.is_empty()   { "none" } else { &config.session_code  },
    ))).await;

    // ── TCP connect (10 s timeout) ─────────────────────────────────────────
    let stream = timeout(Duration::from_secs(10), TcpStream::connect(&addr))
        .await
        .map_err(|_| format!("Connection timeout to {addr}"))?
        .map_err(|e| format!("TCP connect failed: {e}"))?;

    state.log(&app, LogEntry::info(format!("TCP connected to {addr}"))).await;

    let (reader, mut writer) = stream.into_split();

    // ── Routing Activation Request ─────────────────────────────────────────
    let ra_req = build_routing_activation(config.doip_source_address);
    writer.write_all(&ra_req).await
        .map_err(|e| format!("Send Routing Activation Request: {e}"))?;

    state.log(&app, LogEntry::doip(format!(
        "→ Routing Activation Request  [src=0x{:04X}  type=Default]",
        config.doip_source_address,
    ))).await;

    // ── Routing Activation Response (5 s timeout) ──────────────────────────
    {
        let mut tmp_reader = reader;
        let mut hdr_buf = [0u8; HEADER_LEN];

        timeout(Duration::from_secs(5), tmp_reader.read_exact(&mut hdr_buf))
            .await
            .map_err(|_| "Timeout waiting for Routing Activation Response".to_string())?
            .map_err(|e| format!("Read RA response header: {e}"))?;

        // Valida versione DoIP
        if hdr_buf[0] != DOIP_VERSION || hdr_buf[1] != DOIP_INV_VERSION {
            return Err(format!(
                "Invalid DoIP version in RA response: {:02X}/{:02X}",
                hdr_buf[0], hdr_buf[1]
            ));
        }

        let pt  = u16::from_be_bytes([hdr_buf[2], hdr_buf[3]]);
        let len = u32::from_be_bytes([hdr_buf[4], hdr_buf[5], hdr_buf[6], hdr_buf[7]]) as usize;

        if pt != PT_ROUTING_ACT_RES {
            return Err(format!(
                "Expected Routing Activation Response (0x{PT_ROUTING_ACT_RES:04X}), got 0x{pt:04X}"
            ));
        }

        if len > MAX_PAYLOAD {
            return Err(format!("RA response payload too large: {len} bytes"));
        }

        let mut ra_payload = vec![0u8; len];
        if len > 0 {
            timeout(Duration::from_secs(5), tmp_reader.read_exact(&mut ra_payload))
                .await
                .map_err(|_| "Timeout reading RA payload".to_string())?
                .map_err(|e| format!("Read RA payload: {e}"))?;
        }

        let rc = ra_payload.get(4).copied().unwrap_or(0xFF);
        match rc {
            RC_ROUTING_OK => {
                state.log(&app, LogEntry::doip(
                    "← Routing Activation Response: Successfully activated (0x10)",
                )).await;
            }
            RC_ROUTING_ALREADY_ACTIVE => {
                state.log(&app, LogEntry::doip(
                    "← Routing Activation Response: Already active (0x11) — OK",
                )).await;
            }
            _ => {
                return Err(format!("Routing activation denied: response code 0x{rc:02X}"));
            }
        }

        // Hand reader back to the spawned reader task
        let (frame_tx, frame_rx) = mpsc::channel::<Result<DoipFrame, String>>(64);
        tokio::spawn(reader_task(tmp_reader, frame_tx));

        state.set_status(&app, "Connected").await;
        state.log(&app, LogEntry::info(format!(
            "Bridge active  [src=0x{:04X}  tgt=0x{:04X}]",
            config.doip_source_address, config.doip_target_address,
        ))).await;

        // ── WebSocket tunnel server ────────────────────────────────────────
        let (tun_tx, tun_rx) = mpsc::channel::<Vec<u8>>(64);
        // broadcast channel: DoIP responses → all connected WS clients (capacity 64 frames)
        // Manteniamo un receiver dummy per evitare SendError quando nessun client WS è connesso
        let (bcast_tx, bcast_dummy) = broadcast::channel::<Vec<u8>>(64);
        tokio::spawn(async move {
            let mut rx = bcast_dummy;
            while rx.recv().await.is_ok() { /* dummy — mantiene vivo il sender */ }
        });
        let ws_port   = config.local_ws_port;
        let app_cl    = app.clone();
        let state_cl  = state.clone();
        let bcast_tx2 = bcast_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tunnel::run_ws_server(app_cl.clone(), state_cl.clone(), ws_port, tun_tx, bcast_tx2).await {
                state_cl.log(&app_cl, LogEntry::error(format!("WS server: {e}"))).await;
            }
        });

        state.log(&app, LogEntry::info(format!(
            "WebSocket tunnel listening on ws://127.0.0.1:{ws_port}"
        ))).await;

        // ── Main forwarding loop ───────────────────────────────────────────
        run_forward_loop(
            &app,
            &state,
            &config,
            &mut writer,
            frame_rx,
            tun_rx,
            bcast_tx,
            &mut cmd_rx,
        ).await;
    }

    state.set_status(&app, "Disconnected").await;
    state.log(&app, LogEntry::info("Disconnected from DoIP server")).await;
    Ok(())
}

// ── Forwarding loop ────────────────────────────────────────────────────────

async fn run_forward_loop(
    app:       &AppHandle,
    state:     &Arc<AppState>,
    config:    &AppConfig,
    writer:    &mut tokio::net::tcp::OwnedWriteHalf,
    mut frame_rx: mpsc::Receiver<Result<DoipFrame, String>>,
    mut tun_rx:   mpsc::Receiver<Vec<u8>>,
    bcast_tx:  broadcast::Sender<Vec<u8>>,
    cmd_rx:    &mut mpsc::Receiver<BridgeCommand>,
) {
    let src = config.doip_source_address;
    let tgt = config.doip_target_address;

    loop {
        tokio::select! {
            // ── Command from UI ──────────────────────────────────────────
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(BridgeCommand::Disconnect) | None => {
                        state.log(app, LogEntry::info("Disconnect requested")).await;
                        break;
                    }
                    Some(BridgeCommand::SendDiagnostic(data)) => {
                        let msg = build_diagnostic(src, tgt, &data);
                        if writer.write_all(&msg).await.is_err() {
                            state.log(app, LogEntry::error("Write error — closing")).await;
                            break;
                        }
                        state.log(app, LogEntry::doip(format!(
                            "→ Diagnostic [src=0x{src:04X} tgt=0x{tgt:04X} len={}]",
                            data.len()
                        ))).await;
                    }
                }
            }

            // ── Data from WebSocket tunnel ────────────────────────────────
            data = tun_rx.recv() => {
                if let Some(data) = data {
                    let msg = build_diagnostic(src, tgt, &data);
                    if writer.write_all(&msg).await.is_err() {
                        state.log(app, LogEntry::error("Write error (WS→DoIP) — closing")).await;
                        break;
                    }
                    state.log(app, LogEntry::doip(format!(
                        "→ [WS→DoIP] Diagnostic [{} bytes]", data.len()
                    ))).await;
                }
            }

            // ── Frame from remote DoIP server ─────────────────────────────
            frame_result = frame_rx.recv() => {
                match frame_result {
                    Some(Ok(frame)) => {
                        handle_incoming_frame(app, state, writer, &frame, &bcast_tx).await;
                    }
                    Some(Err(e)) => {
                        state.log(app, LogEntry::error(format!("Connection lost: {e}"))).await;
                        break;
                    }
                    None => {
                        state.log(app, LogEntry::error("Connection lost: reader task ended")).await;
                        break;
                    }
                }
            }
        }
    }
}

// ── Incoming frame handler ─────────────────────────────────────────────────

async fn handle_incoming_frame(
    app:      &AppHandle,
    state:    &Arc<AppState>,
    writer:   &mut tokio::net::tcp::OwnedWriteHalf,
    frame:    &DoipFrame,
    bcast_tx: &broadcast::Sender<Vec<u8>>,
) {
    match frame.payload_type {
        PT_ALIVE_CHECK_REQ => {
            state.log(app, LogEntry::doip("← Alive Check Request")).await;
            let resp = build_alive_check_response();
            if writer.write_all(&resp).await.is_err() {
                state.log(app, LogEntry::warn("Failed to send Alive Check Response")).await;
            } else {
                state.log(app, LogEntry::doip("→ Alive Check Response")).await;
            }
        }
        PT_DIAG_MSG if frame.payload.len() >= 4 => {
            let src = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
            let tgt = u16::from_be_bytes([frame.payload[2], frame.payload[3]]);
            let data = &frame.payload[4..];
            let preview_len = data.len().min(8);
            state.log(app, LogEntry::doip(format!(
                "← Diagnostic [src=0x{src:04X} tgt=0x{tgt:04X} len={}]  {:02X?}{}",
                data.len(),
                &data[..preview_len],
                if data.len() > preview_len { "…" } else { "" }
            ))).await;
            // Forward raw UDS payload (without DoIP header/addresses) to all WS clients
            let _ = bcast_tx.send(data.to_vec());
        }
        PT_DIAG_POS_ACK => {
            state.log(app, LogEntry::doip("← Diagnostic Positive ACK")).await;
        }
        PT_DIAG_NEG_ACK => {
            let code = frame.payload.get(4).copied().unwrap_or(0xFF);
            state.log(app, LogEntry::warn(format!(
                "← Diagnostic Negative ACK [code=0x{code:02X}]"
            ))).await;
        }
        pt => {
            state.log(app, LogEntry::debug(format!(
                "← Unknown payload type 0x{pt:04X} [{} bytes]",
                frame.payload.len()
            ))).await;
        }
    }
}
