use std::sync::Arc;

use async_trait::async_trait;

use crate::cluster::file_download::DownloadCleanupTrigger;
use crate::config::clusters::ClusterConfig;
use crate::protocol::message::Message;
use crate::protocol::types::{ClusterRole, Priority};

/// Outbound message type for the WebSocket connection channel.
/// The forwarder task in the WS handler matches on this to send
/// the appropriate WebSocket frame type.
#[derive(Debug)]
pub enum WsOutbound {
    /// A binary application message.
    Binary(Vec<u8>),
    /// A WebSocket-level Ping frame (for keep-alive / latency checks).
    Ping,
    /// A WebSocket-level Close frame. The forwarder sends a Close
    /// to the peer and then exits its send loop so the read loop
    /// can observe the peer's Close ack and clean up. This is the
    /// only reliable way to signal a server-initiated disconnect
    /// across institutional boundaries (firewall/NAT/partition)
    /// where TCP keep-alive and the application's own ping/pong
    /// are the only liveness signals.
    Close,
}

/// Channel sender for writing data to a WebSocket connection.
/// The actual WebSocket I/O is handled by the WS handler task;
/// clusters push `WsOutbound` variants through this channel.
pub type WsConnectionSender = tokio::sync::mpsc::UnboundedSender<WsOutbound>;

/// Trait for cluster operations. All cluster types (master, file download, file upload)
/// implement this trait. Use `Arc<dyn ClusterTrait>` for trait-object based dependency injection.
#[async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait ClusterTrait: Send + Sync {
    /// Cluster name (from config).
    fn name(&self) -> String;

    /// Whether the cluster has an active WebSocket connection.
    fn is_online(&self) -> bool;

    /// Role of this cluster connection.
    fn role(&self) -> ClusterRole;

    /// Human-readable role string for logging.
    fn role_string(&self) -> String;

    /// Cluster configuration details.
    fn cluster_details(&self) -> ClusterConfig;

    /// Handle an incoming binary message from the remote cluster.
    async fn handle_message(&self, message: Message);

    /// Send a message to the remote cluster (queues it for the scheduler).
    async fn send_message(&self, message: Message);

    /// Low-level: queue serialized data for sending.
    async fn queue_message(&self, source: String, data: Vec<u8>, priority: Priority);

    /// Wait for the message queue to drain.
    /// If `wait_for_empty` is false: waits only if queue exceeds MAX threshold, until it drops below MIN.
    /// If `wait_for_empty` is true: waits until queue is completely empty (for message ordering).
    /// Returns false on timeout.
    async fn wait_for_queue_drain(&self, wait_for_empty: bool) -> bool;

    /// Set or clear the WebSocket connection sender.
    /// Pass `None` to disconnect.
    async fn set_connection(&self, conn: Option<WsConnectionSender>);

    /// Send a WebSocket-level Ping frame to the remote cluster (for keep-alive).
    fn send_ping(&self);

    /// Close the WebSocket connection.
    async fn close(&self, force: bool);

    /// Stop all background tasks (scheduler, prune, resend).
    #[allow(dead_code)]
    fn stop(&self);

    /// Idempotent conclusive termination for retained background tasks.
    /// Dedicated file-download clusters override this to drain scheduler,
    /// prune, and resend handles within the existing five-second close
    /// bound reused by [`crate::websocket::server::WS_CLOSE_HANDSHAKE_GRACE_SECONDS`].
    /// The default implementation is a no-op so master, shared, and
    /// upload clusters keep their existing detached-task semantics.
    async fn terminate_download_tasks(&self) {}
}

/// Unique identifier for a WebSocket connection.
/// Used as a key in connection maps instead of raw pointer comparison.
pub type ConnectionId = u64;

/// Trait for cluster lifecycle management.
/// Manages cluster connections, reconnection, ping/pong, and file transfer sessions.
#[async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait ClusterManagerTrait: Send + Sync {
    /// Look up a cluster by name.
    fn get_cluster_by_name(&self, name: &str) -> Option<Arc<dyn ClusterTrait>>;

    /// Look up a cluster by its WebSocket connection ID.
    #[allow(dead_code)]
    fn get_cluster_by_connection(&self, conn_id: ConnectionId) -> Option<Arc<dyn ClusterTrait>>;

    /// Exact admission context for an accepted dedicated file-download handler.
    ///
    /// Returns `Some((session, connection_id, cluster))` only when the
    /// `connection_id` is currently bound to a `FileDownload` cluster AND its
    /// `DownloadSession` is in `Connected(connection_id)` state — i.e. the
    /// session has been admitted and is the canonical entry. The WebSocket
    /// handler retains the exact `Arc<DownloadSession>` and immutable
    /// accepted `ConnectionId` from this call so it can perform exact
    /// `Closing -> Closed` completion at handler exit without relying on a
    /// manager lookup.
    ///
    /// Returns `None` for non-download roles (master, LTK, upload) and when
    /// the connection is no longer admitted.
    fn get_file_download_admission(
        &self,
        conn_id: ConnectionId,
    ) -> Option<(
        Arc<crate::cluster::file_download::DownloadSession>,
        ConnectionId,
        Arc<dyn ClusterTrait>,
    )> {
        let _ = conn_id;
        None
    }

    /// Handle a new WebSocket connection with the given token.
    /// Returns the cluster if the token is valid, None otherwise.
    async fn handle_new_connection(
        &self,
        conn_id: ConnectionId,
        ws_sender: WsConnectionSender,
        token: &str,
    ) -> Option<Arc<dyn ClusterTrait>>;

    /// Remove a connection (on close/error). If `close` is true, also close the WS.
    async fn remove_connection(&self, conn_id: ConnectionId, close: bool);

    /// Handle a pong response from a cluster.
    fn handle_pong(&self, conn_id: ConnectionId);

    /// Create a file download session for the given cluster and UUID.
    async fn create_file_download(
        &self,
        cluster: &Arc<dyn ClusterTrait>,
        uuid: &str,
    ) -> Arc<dyn ClusterTrait>;

    /// Create a file upload session for the given cluster and UUID.
    async fn create_file_upload(
        &self,
        cluster: &Arc<dyn ClusterTrait>,
        uuid: &str,
    ) -> Arc<dyn ClusterTrait>;

    /// Check if a cluster is currently connected.
    #[allow(dead_code)]
    fn is_cluster_online(&self, cluster: &dyn ClusterTrait) -> bool;

    /// Log a WebSocket error for a cluster.
    fn report_websocket_error(&self, cluster_name: Option<String>, error: String);

    /// Get the `FileDownloadSession` for a given UUID (for HTTP handler to access).
    fn get_file_download(
        &self,
        uuid: &str,
    ) -> Option<Arc<crate::cluster::file_download::FileDownloadState>>;

    /// Get a clone of the session-scoped cleanup trigger for an active file
    /// download. Returns `None` when no session exists for the UUID. Used by
    /// the HTTP handler to wire pre-response and body-drop guards to the same
    /// one-shot trigger. The default implementation returns `None` so that
    /// callers without an active session (including mocks) continue to work
    /// even though they cannot observe cleanup.
    fn get_file_download_cleanup_trigger(&self, uuid: &str) -> Option<DownloadCleanupTrigger> {
        let _ = uuid;
        None
    }

    /// Returns `true` when the manager has begun bounded application
    /// shutdown. The HTTP download endpoint consults this flag and routes
    /// post-shutdown admission attempts into the existing
    /// `SERVICE_UNAVAILABLE` typed error path so no transport owner is
    /// published. Default returns `false` so mocks continue to work.
    fn is_application_shutting_down(&self) -> bool {
        false
    }

    /// Begin bounded application shutdown. Sets the dedicated-admission
    /// flag so new file-download admission is rejected for the remainder
    /// of the process lifetime, then synchronously triggers every
    /// currently registered dedicated download session with reason
    /// [`crate::cluster::file_download::DownloadShutdownReason::ApplicationShutdown`].
    /// The trigger is non-blocking, performs no `await`, and does not
    /// spawn. Returns the number of sessions that received the trigger;
    /// repeated calls return `0` because the flag is set on the first
    /// call and later triggers on already-`Closing` sessions are no-ops.
    /// Default returns `0` so mocks continue to work.
    fn begin_application_shutdown(&self) -> usize {
        0
    }

    /// Snapshot of every dedicated file-download cluster currently
    /// retained for shutdown-time termination. The returned `Vec`
    /// contains `Weak` references so dropped clusters do not extend the
    /// manager's lifetime. Callers pair this with
    /// [`ClusterTrait::terminate_download_tasks`] to drain scheduler,
    /// prune, and resend handles within the existing five-second close
    /// bound reused by the WebSocket handler. Default returns an empty
    /// `Vec` so mocks continue to work.
    fn dedicated_download_clusters(&self) -> Vec<std::sync::Weak<dyn ClusterTrait>> {
        Vec::new()
    }

    /// Get the `FileUploadSession` for a given UUID (for HTTP handler to access).
    fn get_file_upload(
        &self,
        uuid: &str,
    ) -> Option<Arc<crate::cluster::file_upload::FileUploadState>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id_is_u64() {
        let id: ConnectionId = 42;
        assert_eq!(id, 42u64);
    }
}
