use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, header::AUTHORIZATION};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};

use crate::app::AppState;
use crate::cluster::traits::ConnectionId;
use crate::protocol::constants::{SERVER_READY, SYSTEM_SOURCE};
use crate::protocol::message::Message;
use crate::protocol::types::Priority;

use crate::cluster::traits::WsOutbound;

/// Global connection ID counter.
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// Generate a unique connection ID.
fn generate_connection_id() -> ConnectionId {
    NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed)
}

/// WebSocket upgrade handler.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    request: Request,
) -> impl IntoResponse {
    let token = extract_token_from_headers(request.headers());
    let client_ip = request
        .extensions()
        .get::<std::net::SocketAddr>()
        .map_or_else(|| "unknown".to_string(), std::string::ToString::to_string);

    tracing::debug!("WS: Received upgrade request from {}", client_ip);
    tracing::trace!("WS: Token extracted (length: {})", token.len());

    ws.on_upgrade(move |socket| {
        tracing::debug!("WS: Upgrade successful, handling socket from {}", client_ip);
        handle_socket(socket, token, state)
    })
}

/// Extract token from Authorization: Bearer header.
fn extract_token_from_headers(headers: &HeaderMap) -> String {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|header| {
            if header.starts_with("Bearer ") {
                header.strip_prefix("Bearer ")
            } else {
                None
            }
        })
        .unwrap_or_default()
        .to_string()
}

/// Handle a single WebSocket connection lifecycle.
async fn handle_socket(socket: WebSocket, token: String, state: AppState) {
    let (ws_sink, mut ws_stream) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();

    let conn_id = generate_connection_id();
    tracing::debug!("WS: Generated connection ID {}", conn_id);

    // Authenticate the connection
    tracing::trace!("WS: Authenticating connection {} with token", conn_id);
    let cluster = state
        .cluster_manager
        .handle_new_connection(conn_id, tx.clone(), &token)
        .await;

    let cluster = if let Some(c) = cluster {
        tracing::info!(
            "WS: Opened connection from {} as role {} (conn_id={})",
            c.name(),
            c.role_string(),
            conn_id
        );
        c
    } else {
        tracing::warn!(
            "WS: Invalid token used (conn_id={}) - connection rejected",
            conn_id
        );
        return;
    };

    // Send SERVER_READY
    tracing::trace!("WS: Sending SERVER_READY to {}", cluster.name());
    let msg = Message::new(SERVER_READY, Priority::Highest, SYSTEM_SOURCE);
    cluster.send_message(msg).await;

    // Spawn forwarder: channel -> WS sink
    let ws_sink = Arc::new(tokio::sync::Mutex::new(ws_sink));
    let ws_sink_clone = Arc::clone(&ws_sink);
    tracing::debug!("WS: Spawning forwarder task for connection {}", conn_id);
    let forwarder = tokio::spawn(async move {
        let mut message_count = 0u64;
        while let Some(outbound) = rx.recv().await {
            message_count += 1;
            let mut sink = ws_sink_clone.lock().await;
            let ws_msg = match outbound {
                WsOutbound::Binary(data) => {
                    tracing::trace!(
                        "WS: Sending binary message ({} bytes) to connection {}",
                        data.len(),
                        conn_id
                    );
                    WsMessage::Binary(data.into())
                }
                WsOutbound::Ping => {
                    tracing::trace!("WS: Sending ping to connection {}", conn_id);
                    WsMessage::Ping(vec![].into())
                }
            };
            if sink.send(ws_msg).await.is_err() {
                tracing::debug!(
                    "WS: Send failed for connection {} after {} messages - sink closed",
                    conn_id,
                    message_count
                );
                break;
            }
        }
        tracing::debug!(
            "WS: Forwarder task exiting for connection {} (sent {} messages)",
            conn_id,
            message_count
        );
    });

    // Read from WS stream
    tracing::debug!("WS: Starting read loop for connection {}", conn_id);
    let mut received_count = 0u64;
    while let Some(msg_result) = ws_stream.next().await {
        received_count += 1;
        match msg_result {
            Ok(WsMessage::Binary(data)) => {
                tracing::trace!(
                    "WS: Received binary message ({} bytes) from connection {}",
                    data.len(),
                    conn_id
                );
                let message = Message::from_bytes(data.to_vec());
                tracing::trace!(
                    "WS: Parsed message - ID: {}, Source: {}, Priority: {:?}",
                    message.id(),
                    message.source(),
                    message.priority()
                );
                cluster.handle_message(message).await;
            }
            Ok(WsMessage::Pong(_)) => {
                tracing::trace!("WS: Received pong from connection {}", conn_id);
                state.cluster_manager.handle_pong(conn_id);
            }
            Ok(WsMessage::Close(frame)) => {
                tracing::debug!(
                    "WS: Received close frame from connection {:?} - exiting read loop",
                    frame
                );
                break;
            }
            Ok(WsMessage::Text(text)) => {
                tracing::warn!(
                    "WS: Received unexpected text message from connection {}: {}",
                    conn_id,
                    text
                );
            }
            Ok(WsMessage::Ping(data)) => {
                tracing::trace!(
                    "WS: Received ping from connection {} ({} bytes)",
                    conn_id,
                    data.len()
                );
                // Axum automatically responds with pong
            }
            Err(e) => {
                tracing::warn!("WS: Error reading from connection {}: {}", conn_id, e);
                state
                    .cluster_manager
                    .report_websocket_error(Some(cluster.name()), format!("{e}"));
                break;
            }
        }
    }

    // Cleanup
    tracing::debug!(
        "WS: Cleaning up connection {} (received {} messages)",
        conn_id,
        received_count
    );
    state.cluster_manager.remove_connection(conn_id, true).await;
    forwarder.abort();
    tracing::info!(
        "WS: Closed connection with {} (conn_id={}, received={}, sent={})",
        cluster.name(),
        conn_id,
        received_count,
        forwarder.is_finished()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_connection_id_unique() {
        let id1 = generate_connection_id();
        let id2 = generate_connection_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_generate_connection_id_monotonic() {
        let id1 = generate_connection_id();
        let id2 = generate_connection_id();
        assert!(id2 > id1);
    }
}
