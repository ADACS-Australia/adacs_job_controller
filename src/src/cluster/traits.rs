use std::sync::Arc;

use async_trait::async_trait;

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
    fn send_message(&self, message: Message);

    /// Low-level: queue serialized data for sending.
    fn queue_message(&self, source: String, data: Vec<u8>, priority: Priority);

    /// Wait for the message queue to drain.
    /// If `wait_for_empty` is false: waits only if queue exceeds MAX threshold, until it drops below MIN.
    /// If `wait_for_empty` is true: waits until queue is completely empty (for message ordering).
    /// Returns false on timeout.
    async fn wait_for_queue_drain(&self, wait_for_empty: bool) -> bool;

    /// Set or clear the WebSocket connection sender.
    /// Pass `None` to disconnect.
    fn set_connection(&self, conn: Option<WsConnectionSender>);

    /// Send a WebSocket-level Ping frame to the remote cluster (for keep-alive).
    fn send_ping(&self);

    /// Close the WebSocket connection.
    fn close(&self, force: bool);

    /// Stop all background tasks (scheduler, prune, resend).
    #[allow(dead_code)]
    fn stop(&self);
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
