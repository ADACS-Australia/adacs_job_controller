// File upload session state - shared between HTTP handler and WS handler
use std::sync::atomic::AtomicBool;

/// State for an active file upload session.
/// Shared between the HTTP PUT handler (producer) and the WebSocket handler (consumer).
pub struct FileUploadState {
    /// Error state (set by WS handler if remote reports error)
    pub error: AtomicBool,
    pub error_details: tokio::sync::Mutex<String>,

    /// Completion state
    pub complete: AtomicBool,
    pub received_data: AtomicBool,

    /// Notification for readiness/error/completion
    pub data_notify: tokio::sync::Notify,
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
