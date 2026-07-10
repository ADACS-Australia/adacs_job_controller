// File upload session state - shared between HTTP handler and WS handler
use std::sync::atomic::AtomicBool;

/// State for an active file upload session.
/// Shared between the HTTP PUT handler (producer) and the WebSocket handler (consumer).
pub struct FileUploadState {
    /// Error state (set by WS handler if remote reports error)
    pub error: AtomicBool,
    /// Human-readable error message from the remote cluster
    pub error_details: tokio::sync::Mutex<String>,

    /// Set when the remote cluster reports upload completion
    pub complete: AtomicBool,
    /// Set when at least one chunk of upload data has been received
    pub received_data: AtomicBool,

    /// Notifies HTTP handler when data is ready, an error occurs, or upload completes
    pub data_notify: tokio::sync::Notify,
    /// Set when file metadata or an error is available for the HTTP handler to read
    pub data_ready: AtomicBool,
}

impl FileUploadState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            error: AtomicBool::new(false),
            error_details: tokio::sync::Mutex::new(String::new()),
            complete: AtomicBool::new(false),
            received_data: AtomicBool::new(false),
            data_notify: tokio::sync::Notify::new(),
            data_ready: AtomicBool::new(false),
        }
    }
}

impl Default for FileUploadState {
    fn default() -> Self {
        Self::new()
    }
}
