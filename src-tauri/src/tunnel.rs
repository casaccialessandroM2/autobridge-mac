//! Local WebSocket tunnel server — bidirectional DoIP↔WS bridge.
//!
//! External diagnostic tools connect to ws://127.0.0.1:{local_ws_port} and
//! exchange raw UDS/DoIP payload bytes as binary WebSocket frames:
//!   WS client → DoIP  : frames forwarded via `tun_tx` mpsc channel
//!   DoIP → WS clients : frames broadcast via `bcast_tx` broadcast channel

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tauri::AppHandle;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::state::{AppState, LogEntry};

/// Start the local WebSocket server on 127.0.0.1:{port}.
///
/// `tun_tx`   — mpsc sender: WS binary frames → DoIP bridge
/// `bcast_tx` — broadcast sender: DoIP responses → all WS clients
pub async fn run_ws_server(
    app:      AppHandle,
    state:    Arc<AppState>,
    port:     u16,
    tun_tx:   mpsc::Sender<Vec<u8>>,
    bcast_tx: broadcast::Sender<Vec<u8>>,
) -> Result<(), String> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).await
        .map_err(|e| format!("WS bind {addr}: {e}"))?;

    loop {
        let (tcp_stream, peer_addr) = match listener.accept().await {
            Ok(v)  => v,
            Err(e) => {
                state.log(&app, LogEntry::warn(format!("WS accept: {e}"))).await;
                continue;
            }
        };

        let tx     = tun_tx.clone();
        let rx     = bcast_tx.subscribe();
        let app_c  = app.clone();
        let st_c   = state.clone();

        tokio::spawn(async move {
            st_c.log(&app_c, LogEntry::info(
                format!("WS client connected: {peer_addr}")
            )).await;

            match tokio_tungstenite::accept_async(tcp_stream).await {
                Err(e) => {
                    st_c.log(&app_c, LogEntry::warn(
                        format!("WS handshake {peer_addr}: {e}")
                    )).await;
                }
                Ok(ws) => {
                    handle_ws_client(ws, peer_addr, tx, rx, &app_c, &st_c).await;
                }
            }

            st_c.log(&app_c, LogEntry::info(
                format!("WS client disconnected: {peer_addr}")
            )).await;
        });
    }
}

async fn handle_ws_client(
    ws:        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    peer_addr: std::net::SocketAddr,
    tun_tx:    mpsc::Sender<Vec<u8>>,
    mut bcast: broadcast::Receiver<Vec<u8>>,
    app:       &AppHandle,
    state:     &Arc<AppState>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();

    loop {
        tokio::select! {
            // ── WS client → DoIP bridge ──────────────────────────────────
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        state.log(app, LogEntry::debug(
                            format!("WS→DoIP [{peer_addr}] {} bytes", data.len())
                        )).await;
                        if tun_tx.send(data.to_vec()).await.is_err() {
                            break; // bridge gone
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {} // Text, Ping, Pong — ignore
                }
            }

            // ── DoIP bridge → WS client ──────────────────────────────────
            bcast_msg = bcast.recv() => {
                match bcast_msg {
                    Ok(data) => {
                        if ws_tx.send(Message::Binary(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        state.log(app, LogEntry::warn(
                            format!("WS client {peer_addr} lento: {n} frame persi")
                        )).await;
                    }
                }
            }
        }
    }
}
