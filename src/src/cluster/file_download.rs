// File download session state - shared between HTTP handler and WS handler
use std::sync::atomic::{AtomicBool, AtomicU64};

/// State for an active file download session.
/// Shared between the HTTP GET handler (consumer) and the WebSocket handler (producer).
pub struct FileDownloadState {
    /// Channel for receiving file chunks from the WS handler
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
