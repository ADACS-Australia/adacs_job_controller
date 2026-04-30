// File download session state - shared between HTTP handler and WS handler
use std::sync::atomic::{AtomicBool, AtomicU64};

/// State for an active file download session.
/// Shared between the HTTP GET handler (consumer) and the WebSocket handler (producer).
pub struct FileDownloadState {
    /// Channel for receiving file chunks from the WS handler
    pub chunk_sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub chunk_receiver: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,

    /// File metadata
    pub file_size: AtomicU64,
    pub received_data: AtomicBool,

    /// Error state
    pub error: AtomicBool,
    pub error_details: tokio::sync::Mutex<String>,

    /// Byte counters for backpressure
    pub received_bytes: AtomicU64,
    pub sent_bytes: AtomicU64,
    pub client_paused: AtomicBool,

    /// Notification for data readiness (file details or error received)
    pub data_notify: tokio::sync::Notify,
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
