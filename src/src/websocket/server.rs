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
    ws.on_upgrade(move |socket| handle_socket(socket, token, state))
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

    // Authenticate the connection
    let cluster = state
        .cluster_manager
        .handle_new_connection(conn_id, tx.clone(), &token)
        .await;

    let cluster = if let Some(c) = cluster {
        tracing::info!(
            "WS: Opened connection from {} as role {}",
            c.name(),
            c.role_string()
        );
        c
    } else {
        tracing::warn!("WS: Invalid token used (conn_id={})", conn_id);
        return;
    };

    // Send SERVER_READY
    let msg = Message::new(SERVER_READY, Priority::Highest, SYSTEM_SOURCE);
    cluster.send_message(msg).await;

    // Spawn forwarder: channel -> WS sink
    let ws_sink = Arc::new(tokio::sync::Mutex::new(ws_sink));
    let ws_sink_clone = Arc::clone(&ws_sink);
    let forwarder = tokio::spawn(async move {
        while let Some(outbound) = rx.recv().await {
            let mut sink = ws_sink_clone.lock().await;
            let ws_msg = match outbound {
                WsOutbound::Binary(data) => WsMessage::Binary(data.into()),
                WsOutbound::Ping => WsMessage::Ping(vec![].into()),
            };
            if sink.send(ws_msg).await.is_err() {
                break;
            }
        }
    });

    // Read from WS stream
    while let Some(msg_result) = ws_stream.next().await {
        match msg_result {
            Ok(WsMessage::Binary(data)) => {
                let message = Message::from_bytes(data.to_vec());
                cluster.handle_message(message).await;
            }
            Ok(WsMessage::Pong(_)) => {
                state.cluster_manager.handle_pong(conn_id);
            }
            Ok(WsMessage::Close(_)) => break,
            Err(e) => {
                state
                    .cluster_manager
                    .report_websocket_error(Some(cluster.name()), format!("{e}"));
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    state.cluster_manager.remove_connection(conn_id, true).await;
    forwarder.abort();

    tracing::info!("WS: Closed connection with {}", cluster.name());
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
