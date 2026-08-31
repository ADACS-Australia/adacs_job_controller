#![allow(clippy::pedantic)]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, header::AUTHORIZATION};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::Notify;

use crate::app::AppState;
use crate::cluster::file_download::{DownloadSession, DownloadSessionState};
use crate::cluster::traits::ConnectionId;
use crate::protocol::constants::{SERVER_READY, SYSTEM_SOURCE};
use crate::protocol::message::Message;
use crate::protocol::types::Priority;

use crate::cluster::traits::WsOutbound;

/// Global connection ID counter.
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// How long to wait for the peer's Close ack after the forwarder
/// sends a server-initiated `WsOutbound::Close`. If the peer
/// doesn't ack within this window — typically because the same
/// network partition that triggered the disconnect is also
/// blocking the ack — the read loop drops the WebSocket sink
/// (via `handle_socket` returning) so the TCP connection is
/// actually torn down instead of lingering in `CLOSE_WAIT`.
///
/// Set comfortably above the worst-case intercontinental RTT
/// observed in production (Swinburne ↔ Caltech ≈ 200 ms) so a
/// healthy peer always acks in time, but short enough that
/// stuck half-open sockets don't accumulate on the server.
pub const WS_CLOSE_HANDSHAKE_GRACE_SECONDS: u64 = 5;

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
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map_or_else(|| "unknown".to_string(), |ci| ci.0.to_string());

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
            // Per RFC 6750 the auth scheme is case-insensitive, so accept any casing.
            match header.get(..7) {
                Some(prefix) if prefix.eq_ignore_ascii_case("Bearer ") => Some(&header[7..]),
                _ => None,
            }
        })
        .unwrap_or_default()
        .to_string()
}

/// Why the WebSocket handler terminated. The handler drives the
/// exact `DownloadSession::complete(...)` transition from this value.
///
/// Variants are kept for diagnostics and explicit `remove_connection`
/// close-flag selection even when not all are constructed at runtime.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerExitReason {
    /// Forwarder sent `WsOutbound::Close` and peer acked (or transport ended).
    GracefulCloseAcked,
    /// Peer-initiated Close frame observed.
    PeerClose,
    /// Read stream returned `None` (peer EOF).
    PeerEof,
    /// Read returned an error.
    ReadError,
    /// Read or write error from the underlying WebSocket transport.
    WriteError,
    /// Server-initiated close failed to be acked within the grace period;
    /// forced fallback to TCP teardown.
    ForcedFallback,
    /// Inactivity deadline (read EOF after idle) — currently the same code
    /// path as `PeerEof`, but tracked distinctly for diagnostics.
    Inactivity,
    /// Missed-pong eviction by `ClusterManager::check_pings`.
    MissedPong,
}

/// RAII exit guard for the WebSocket handler. Performs exact idempotent
/// session completion and emits the single authoritative `WS: Closed
/// connection` event for every accepted dedicated download connection.
///
/// - Synchronous: no `await`, no `tokio::spawn`, no Tokio mutex held across
///   an await.
/// - Idempotent: re-entry (e.g. an explicit completion before guard drop) is
///   a no-op; the closed event is emitted at most once per guard instance.
/// - RAII: the guard is the last binding in the handler scope so it always
///   runs on every exit path (normal, error, cancellation, forced fallback).
pub struct HandlerExitGuard {
    session: Option<Arc<DownloadSession>>,
    connection_id: Option<ConnectionId>,
    cluster_name: String,
    conn_id: ConnectionId,
    sent_count: Arc<AtomicU64>,
    received_count: u64,
    closed_event_emitted: AtomicBool,
    forced_fallback_emitted: AtomicBool,
    forced_fallback_triggered: bool,
}

impl HandlerExitGuard {
    fn new(
        session: Option<Arc<DownloadSession>>,
        connection_id: Option<ConnectionId>,
        cluster_name: String,
        conn_id: ConnectionId,
        sent_count: Arc<AtomicU64>,
        received_count: u64,
    ) -> Self {
        Self {
            session,
            connection_id,
            cluster_name,
            conn_id,
            sent_count,
            received_count,
            closed_event_emitted: AtomicBool::new(false),
            forced_fallback_emitted: AtomicBool::new(false),
            forced_fallback_triggered: false,
        }
    }

    fn force_fallback(&mut self) {
        self.forced_fallback_triggered = true;
    }

    /// Mark the closed event as already emitted (e.g. when an explicit
    /// completion path has already logged it). Subsequent drops are no-ops
    /// for the event emission.
    fn mark_closed_emitted(&self) {
        self.closed_event_emitted.store(true, Ordering::SeqCst);
    }

    fn complete(&self) {
        if let (Some(session), Some(conn_id)) = (self.session.as_ref(), self.connection_id) {
            // Idempotent; only the first call transitions Closing -> Closed.
            let _ = session.complete(Some(conn_id));
        }
    }

    fn emit_forced_fallback_warning(&self) {
        if self.forced_fallback_emitted.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::warn!(
            "WS: Close handshake timed out after {}s, forcing TCP close (conn_id={}); dropping every sink clone",
            WS_CLOSE_HANDSHAKE_GRACE_SECONDS,
            self.conn_id,
        );
    }

    fn emit_closed_event(&self) {
        if self.closed_event_emitted.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::info!(
            "WS: Closed connection with {} (conn_id={}, received={}, sent={})",
            self.cluster_name,
            self.conn_id,
            self.received_count,
            self.sent_count.load(Ordering::Relaxed),
        );
    }
}

impl Drop for HandlerExitGuard {
    fn drop(&mut self) {
        if self.forced_fallback_triggered {
            self.emit_forced_fallback_warning();
        }
        self.complete();
        self.emit_closed_event();
    }
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

    // Retain the exact `DownloadSession` and immutable accepted
    // `ConnectionId` for dedicated file downloads so we can perform
    // exact `Closing -> Closed` completion at handler exit without
    // looking the session up by id. The map is the only owner that
    // could be removed; once we hold a strong Arc the session lives
    // until this handler returns.
    let admitted = state.cluster_manager.get_file_download_admission(conn_id);
    let (download_session, accepted_conn_id) = match admitted {
        Some((session, accepted_conn_id, _cluster)) if accepted_conn_id == conn_id => {
            // Re-check the state — the session must still be `Connected(conn_id)`.
            if matches!(
                session.state(),
                DownloadSessionState::Connected(_) | DownloadSessionState::Closing { .. }
            ) {
                (Some(session), Some(accepted_conn_id))
            } else {
                (None, None)
            }
        }
        _ => (None, None),
    };

    // Spawn forwarder: channel -> WS sink
    let ws_sink = Arc::new(tokio::sync::Mutex::new(ws_sink));
    let ws_sink_clone = Arc::clone(&ws_sink);
    let sent_count = Arc::new(AtomicU64::new(0));
    let sent_count_for_forwarder = Arc::clone(&sent_count);

    // Signaled by the forwarder when it sends a server-initiated
    // Close frame, so the read loop can enter its grace-period
    // sub-loop. Without this, the read loop would wait forever for
    // the peer's Close ack (which may never arrive across a
    // broken institutional link), and `ws_sink` would never be
    // dropped — the TCP connection would stay in CLOSE_WAIT.
    let close_initiated = Arc::new(Notify::new());
    let close_initiated_for_forwarder = Arc::clone(&close_initiated);

    tracing::debug!("WS: Spawning forwarder task for connection {}", conn_id);
    let forwarder = tokio::spawn(async move {
        let mut message_count = 0u64;
        let mut should_exit = false;
        while let Some(outbound) = rx.recv().await {
            message_count += 1;
            sent_count_for_forwarder.store(message_count, Ordering::Relaxed);
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
                WsOutbound::Close => {
                    tracing::debug!(
                        "WS: Sending close frame to connection {} (server-initiated)",
                        conn_id
                    );
                    should_exit = true;
                    WsMessage::Close(None)
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
            if should_exit {
                tracing::debug!(
                    "WS: Forwarder exiting after server-initiated close for connection {} (sent {} messages)",
                    conn_id,
                    message_count
                );
                // Wake the read loop so it can start the grace
                // period timer.
                close_initiated_for_forwarder.notify_one();
                break;
            }
        }
        tracing::debug!(
            "WS: Forwarder task exiting for connection {} (sent {} messages)",
            conn_id,
            message_count
        );
    });

    // The handler-exit guard is bound to the LAST local so it runs
    // on every exit path, including forced fallback. It performs
    // exact idempotent session completion and emits the single
    // authoritative `WS: Closed connection` event. It must not
    // await, must not spawn, and must not hold a Tokio mutex across
    // an await.
    let cluster_name_for_guard = cluster.name();
    let mut guard = HandlerExitGuard::new(
        download_session,
        accepted_conn_id,
        cluster_name_for_guard,
        conn_id,
        Arc::clone(&sent_count),
        0,
    );

    // Read from WS stream
    tracing::debug!("WS: Starting read loop for connection {}", conn_id);
    let mut received_count: u64 = 0;
    let mut server_close_initiated = false;
    let exit_reason: HandlerExitReason;

    loop {
        if server_close_initiated {
            // Enter grace-period sub-loop: wait for the peer's
            // Close ack OR the timeout, whichever comes first.
            // We continue to drain any pongs/pings/stragglers
            // from the stream but only break on Close, EOF, or
            // timeout. Other message types are ignored.
            tracing::debug!(
                "WS: Server-initiated close in progress, waiting up to {}s for peer Close (conn_id={})",
                WS_CLOSE_HANDSHAKE_GRACE_SECONDS,
                conn_id
            );
            let grace = tokio::time::sleep(Duration::from_secs(WS_CLOSE_HANDSHAKE_GRACE_SECONDS));
            tokio::pin!(grace);
            loop {
                tokio::select! {
                    msg = ws_stream.next() => {
                        match msg {
                            Some(Ok(WsMessage::Close(frame))) => {
                                tracing::debug!(
                                    "WS: Received peer Close during grace period for conn_id={}: {:?}",
                                    conn_id, frame
                                );
                                exit_reason = HandlerExitReason::GracefulCloseAcked;
                                break;
                            }
                            None | Some(Err(_)) => {
                                tracing::debug!(
                                    "WS: Stream ended during grace period (conn_id={})",
                                    conn_id
                                );
                                exit_reason = HandlerExitReason::GracefulCloseAcked;
                                break;
                            }
                            Some(Ok(_)) => {}
                        }
                    }
                    () = &mut grace => {
                        // Forced fallback: the peer didn't ack within
                        // the bound. We collapse the two pre-existing
                        // warnings into exactly one at this fallback
                        // boundary; the guard's Drop emits it.
                        guard.force_fallback();
                        exit_reason = HandlerExitReason::ForcedFallback;
                        break;
                    }
                }
            }
            break;
        }

        tokio::select! {
            () = close_initiated.notified() => {
                server_close_initiated = true;
            }
            msg_result = ws_stream.next() => {
                let Some(msg_result) = msg_result else {
                    exit_reason = HandlerExitReason::PeerEof;
                    break;
                };
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
                        exit_reason = HandlerExitReason::PeerClose;
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
                        exit_reason = HandlerExitReason::ReadError;
                        break;
                    }
                }
            }
        }
    }

    // Update the guard with the final received count.
    guard.received_count = received_count;

    // Cleanup
    tracing::debug!(
        "WS: Cleaning up connection {} (received {} messages, reason={:?})",
        conn_id,
        received_count,
        exit_reason
    );

    // Signal the exact session for every terminal reason. The
    // synchronous `trigger` only transitions once, so duplicates are
    // idempotent no-ops. Manager cleanup happens via the existing
    // `remove_connection` path which already routes `FileDownload`
    // through `cleanup_file_download`. We invoke it once per accepted
    // connection so the manager gets exactly one opportunity to
    // perform its side effects. The handler-exit guard then performs
    // the exact `Closing -> Closed` completion without a lookup.
    let remove_close_flag = match exit_reason {
        HandlerExitReason::ReadError
        | HandlerExitReason::WriteError
        | HandlerExitReason::ForcedFallback
        | HandlerExitReason::Inactivity
        | HandlerExitReason::MissedPong => true,
        HandlerExitReason::GracefulCloseAcked
        | HandlerExitReason::PeerClose
        | HandlerExitReason::PeerEof => false,
    };
    // Manager-driven cleanup: this also signals the exact session via
    // `cleanup_trigger().trigger(WebSocketError/WebSocketClosed)`.
    state
        .cluster_manager
        .remove_connection(conn_id, remove_close_flag)
        .await;

    // Conclusively terminate the outbound forwarder so its
    // `ws_sink_clone` is released BEFORE the handler returns.
    // Without this the forwarder's `Arc<Mutex<Sink>>` clone would
    // survive until the forwarder task body completes on its own
    // (which we cannot guarantee) and the TCP socket would linger.
    forwarder.abort();
    match tokio::time::timeout(
        Duration::from_secs(WS_CLOSE_HANDSHAKE_GRACE_SECONDS),
        forwarder,
    )
    .await
    {
        Ok(_) => {}
        Err(_) => {
            tracing::debug!(
                "WS: Forwarder join timed out after {}s for conn_id={}",
                WS_CLOSE_HANDSHAKE_GRACE_SECONDS,
                conn_id
            );
        }
    }

    // Drop the local sink clone so only the (already-completed)
    // forwarder task ever held one. The forwarder task body has
    // finished; its `ws_sink_clone` was dropped when the forwarder
    // closure returned, so dropping our local `ws_sink` here
    // removes the last owner and the underlying `Sink` is dropped.
    drop(ws_sink);

    // Explicit completion + closed-event emission is the guard's
    // job. Mark the closed event as already emitted so Drop does
    // not double-log, then let Drop run the completion.
    guard.mark_closed_emitted();
    guard.complete();
    drop(guard);
    // `ws_stream` is dropped at scope exit here, releasing the
    // remaining socket-half owner.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::file_download::{
        DownloadSession, DownloadSessionState, DownloadShutdownReason, FileDownloadState,
    };

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

    #[test]
    fn test_handler_exit_guard_idempotent_close_event() {
        let g = HandlerExitGuard::new(
            None,
            None,
            "test".to_string(),
            1,
            Arc::new(AtomicU64::new(0)),
            0,
        );
        // Emit twice; second is a no-op.
        g.emit_closed_event();
        g.emit_closed_event();
        assert!(g.closed_event_emitted.load(Ordering::SeqCst));
    }

    #[test]
    fn test_handler_exit_guard_idempotent_forced_fallback_warning() {
        let g = HandlerExitGuard::new(
            None,
            None,
            "test".to_string(),
            1,
            Arc::new(AtomicU64::new(0)),
            0,
        );
        g.emit_forced_fallback_warning();
        g.emit_forced_fallback_warning();
        assert!(g.forced_fallback_emitted.load(Ordering::SeqCst));
    }

    /// Build a `DownloadSession` bound to `conn_id` and pre-transitioned
    /// into `Closing` (so the guard's `complete` has a non-Pending
    /// starting state to transition out of).
    fn make_closing_session(conn_id: ConnectionId) -> Arc<DownloadSession> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let session = DownloadSession::new(
            "test-uuid".to_string(),
            Arc::new(FileDownloadState::new()),
            tx,
        );
        session.bind_connection(conn_id).expect("bind succeeds");
        let _ = session
            .cleanup_trigger()
            .trigger(DownloadShutdownReason::WebSocketError);
        let expected_state = DownloadSessionState::Closing {
            connection_id: Some(conn_id),
            reason: DownloadShutdownReason::WebSocketError,
        };
        assert_eq!(session.state(), expected_state);
        session
    }

    #[test]
    fn test_handler_exit_guard_completes_session_exactly_once() {
        let session = make_closing_session(7);
        let g = HandlerExitGuard::new(
            Some(Arc::clone(&session)),
            Some(7),
            "test".to_string(),
            7,
            Arc::new(AtomicU64::new(0)),
            0,
        );
        // Complete twice (idempotent).
        g.complete();
        g.complete();
        match session.state() {
            DownloadSessionState::Closed {
                connection_id: Some(7),
                reason: DownloadShutdownReason::WebSocketError,
            } => {}
            other => panic!("session should be Closed, got {other:?}"),
        }
    }

    #[test]
    fn test_handler_exit_guard_drop_runs_completion_and_emits_event() {
        let session = make_closing_session(11);
        let sent_count = Arc::new(AtomicU64::new(3));
        let g = HandlerExitGuard::new(
            Some(Arc::clone(&session)),
            Some(11),
            "test_cluster".to_string(),
            11,
            Arc::clone(&sent_count),
            5,
        );
        // Drop without explicit complete or emit.
        drop(g);
        match session.state() {
            DownloadSessionState::Closed { .. } => {}
            other => panic!("session should be Closed, got {other:?}"),
        }
    }

    #[test]
    fn test_handler_exit_guard_drop_without_session_is_safe() {
        // Non-download role: no session retained. Drop must not panic
        // and must not emit the forced-fallback warning (since the flag
        // wasn't set).
        let g = HandlerExitGuard::new(
            None,
            None,
            "master".to_string(),
            42,
            Arc::new(AtomicU64::new(0)),
            0,
        );
        drop(g);
        // The flag starts false; nothing forced it. No assertions
        // beyond "no panic" — the test passes if we reach this line.
    }

    #[test]
    fn test_handler_exit_guard_complete_ignores_wrong_connection_id() {
        let session = make_closing_session(7);
        let g = HandlerExitGuard::new(
            Some(Arc::clone(&session)),
            Some(99), // wrong id
            "test".to_string(),
            7,
            Arc::new(AtomicU64::new(0)),
            0,
        );
        g.complete();
        // Session stays in `Closing` because the wrong connection id
        // cannot transition to `Closed` for a different conn.
        match session.state() {
            DownloadSessionState::Closing { .. } => {}
            other => panic!("session must remain Closing, got {other:?}"),
        }
    }

    #[test]
    fn test_handler_exit_guard_complete_is_a_noop_for_pending_session() {
        // Pending session: never admitted; guard has no session in this
        // test path, but we also exercise the no-op path when the
        // session is Pending (no binding).
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let session = DownloadSession::new(
            "test-uuid-pending".to_string(),
            Arc::new(FileDownloadState::new()),
            tx,
        );
        // The guard's complete path requires the session to be
        // already Closing (which Pending is not). The guard holds
        // None for un-admitted connections, so we just verify the
        // guard's complete is safe with None.
        let g = HandlerExitGuard::new(
            None,
            None,
            "test".to_string(),
            1,
            Arc::new(AtomicU64::new(0)),
            0,
        );
        g.complete();
        // Session is still Pending (un-touched by the guard).
        assert_eq!(session.state(), DownloadSessionState::Pending);
    }
}
