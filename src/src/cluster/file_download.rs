// File download session state - shared between HTTP handler and WS handler
use std::sync::atomic::{AtomicBool, AtomicU64};

/// State for an active file download session.
/// Shared between the HTTP GET handler (consumer) and the WebSocket handler (producer).
pub struct FileDownloadState {
    /// Channel sender for forwarding file chunks from the WS handler to the HTTP consumer.
    pub chunk_sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    /// Mutex-protected receiver for file chunks consumed by the HTTP handler.
    pub chunk_receiver: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
    /// Total file size in bytes (set by `FILE_DETAILS`).
    pub file_size: AtomicU64,
    /// Whether `FILE_DETAILS` has been received.
    pub received_data: AtomicBool,
    /// Whether the download failed (cluster error or HTTP client disconnect).
    pub error: AtomicBool,
    /// Human-readable error message when `error` is set.
    pub error_details: tokio::sync::Mutex<String>,
    /// Total bytes received from the cluster (including buffered chunks).
    pub received_bytes: AtomicU64,
    /// Total bytes sent to the HTTP client.
    pub sent_bytes: AtomicU64,
    /// Whether the cluster stream is paused due to backpressure.
    pub client_paused: AtomicBool,
    /// Notifies the HTTP handler when new data, errors, or file details arrive.
    pub data_notify: tokio::sync::Notify,
    /// Whether file details, a chunk, or an error is ready for the HTTP handler.
    pub data_ready: AtomicBool,
}

impl FileDownloadState {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            chunk_sender: tx,
            chunk_receiver: tokio::sync::Mutex::new(rx),
            file_size: AtomicU64::new(0),
            received_data: AtomicBool::new(false),
            error: AtomicBool::new(false),
            error_details: tokio::sync::Mutex::new(String::new()),
            received_bytes: AtomicU64::new(0),
            sent_bytes: AtomicU64::new(0),
            client_paused: AtomicBool::new(false),
            data_notify: tokio::sync::Notify::new(),
            data_ready: AtomicBool::new(false),
        }
    }
}

impl Default for FileDownloadState {
    fn default() -> Self {
        Self::new()
    }
}

use std::sync::{Arc, Mutex};

use super::traits::ConnectionId;

/// Why shutdown of a dedicated download session was requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadShutdownReason {
    Complete,
    ChunkTimeout,
    FileError,
    ClusterOffline,
    #[allow(dead_code)]
    HttpCancelled,
    WebSocketClosed,
    WebSocketError,
    ResponseError,
    ApplicationShutdown,
}

/// Observable lifecycle state for a dedicated download session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadSessionState {
    Pending,
    Connected(ConnectionId),
    Closing {
        connection_id: Option<ConnectionId>,
        reason: DownloadShutdownReason,
    },
    Closed {
        connection_id: Option<ConnectionId>,
        reason: DownloadShutdownReason,
    },
}

/// Work emitted once when a session first enters `Closing`.
#[derive(Clone, Debug)]
pub struct DownloadCleanupRequest {
    pub download_id: String,
    pub connection_id: Option<ConnectionId>,
    pub reason: DownloadShutdownReason,
    /// Exact originating session without creating a sender/worker/session cycle.
    pub session: std::sync::Weak<DownloadSession>,
}

#[derive(Debug)]
struct DownloadLifecycle {
    state: DownloadSessionState,
}

/// Exact lifecycle ownership for one dedicated file download.
///
/// Transfer bytes remain in `FileDownloadState`; this type owns only immutable
/// session identity, admission state, and shutdown/completion transitions.
pub struct DownloadSession {
    download_id: String,
    transfer: Arc<FileDownloadState>,
    lifecycle: Mutex<DownloadLifecycle>,
    cleanup_sender: tokio::sync::mpsc::UnboundedSender<DownloadCleanupRequest>,
    weak_self: std::sync::Weak<DownloadSession>,
}

impl DownloadSession {
    #[must_use]
    pub fn new(
        download_id: String,
        transfer: Arc<FileDownloadState>,
        cleanup_sender: tokio::sync::mpsc::UnboundedSender<DownloadCleanupRequest>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            download_id,
            transfer,
            lifecycle: Mutex::new(DownloadLifecycle {
                state: DownloadSessionState::Pending,
            }),
            cleanup_sender,
            weak_self: weak_self.clone(),
        })
    }

    #[must_use]
    pub fn download_id(&self) -> &str {
        &self.download_id
    }

    #[must_use]
    pub fn transfer(&self) -> &Arc<FileDownloadState> {
        &self.transfer
    }

    #[must_use]
    pub fn state(&self) -> DownloadSessionState {
        self.lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
    }

    /// Bind the only accepted WebSocket connection while the session is pending.
    /// Bind this download session to the accepted WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadBindError`] when the session is no longer pending, when it
    /// has already been bound to a connection, or when shutdown has already won
    /// the lifecycle race.
    pub fn bind_connection(&self, connection_id: ConnectionId) -> Result<(), DownloadBindError> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match lifecycle.state {
            DownloadSessionState::Pending => {
                lifecycle.state = DownloadSessionState::Connected(connection_id);
                Ok(())
            }
            state => Err(DownloadBindError { state }),
        }
    }

    /// Mark exact handler completion without requiring a manager-map lookup.
    ///
    /// Connected sessions only close after their immutable accepted handler
    /// completes. Pending sessions have no handler and may complete locally.
    pub fn complete(&self, connection_id: Option<ConnectionId>) -> bool {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match lifecycle.state {
            DownloadSessionState::Closing {
                connection_id: expected,
                reason,
            } if expected == connection_id => {
                lifecycle.state = DownloadSessionState::Closed {
                    connection_id: expected,
                    reason,
                };
                true
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn cleanup_trigger(self: &Arc<Self>) -> DownloadCleanupTrigger {
        DownloadCleanupTrigger {
            session: Arc::clone(self),
        }
    }

    fn trigger(&self, reason: DownloadShutdownReason) -> bool {
        let request = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let connection_id = match lifecycle.state {
                DownloadSessionState::Pending => None,
                DownloadSessionState::Connected(connection_id) => Some(connection_id),
                DownloadSessionState::Closing { .. } | DownloadSessionState::Closed { .. } => {
                    return false;
                }
            };
            lifecycle.state = DownloadSessionState::Closing {
                connection_id,
                reason,
            };
            DownloadCleanupRequest {
                download_id: self.download_id.clone(),
                connection_id,
                reason,
                session: self.weak_self.clone(),
            }
        };

        // The endpoint is registered when the session is created. Sending is
        // synchronous and non-blocking; worker ownership stays outside session.
        let _ = self.cleanup_sender.send(request);
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadBindError {
    pub state: DownloadSessionState,
}

/// Cloneable synchronous one-shot trigger for guards and response-body `Drop`.
///
/// Clones share the session's transition lock. The first transition to
/// `Closing` records the reason and emits one worker notification; later calls
/// are idempotent no-ops.
#[derive(Clone)]
pub struct DownloadCleanupTrigger {
    session: Arc<DownloadSession>,
}

impl DownloadCleanupTrigger {
    #[must_use]
    pub fn trigger(&self, reason: DownloadShutdownReason) -> bool {
        self.session.trigger(reason)
    }
}
