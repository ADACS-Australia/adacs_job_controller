/// Lifecycle state of a job on a cluster.
///
/// Wire-compatible `u32` values shared with the C++ orchestrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum JobStatus {
    /// Job accepted but not yet submitted to the scheduler (= 10).
    Pending = 10,
    /// Job is being submitted to the cluster scheduler (= 20).
    Submitting = 20,
    /// Job has been submitted to the scheduler (= 30).
    Submitted = 30,
    /// Job is queued on the cluster, awaiting execution (= 40).
    Queued = 40,
    /// Job is actively running on the cluster (= 50).
    Running = 50,
    /// Cancellation has been requested but not yet confirmed (= 60).
    Cancelling = 60,
    /// Job was cancelled before completion (= 70).
    Cancelled = 70,
    /// Job artifacts are being deleted from the cluster (= 80).
    Deleting = 80,
    /// Job and its artifacts have been removed (= 90).
    Deleted = 90,
    /// Job failed with a generic error (= 400).
    Error = 400,
    /// Job exceeded its allocated wall-clock time (= 401).
    WallTimeExceeded = 401,
    /// Job terminated due to out-of-memory (= 402).
    OutOfMemory = 402,
    /// Job finished successfully (= 500).
    Completed = 500,
}

impl TryFrom<u32> for JobStatus {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, <JobStatus as TryFrom<u32>>::Error> {
        match value {
            10 => Ok(JobStatus::Pending),
            20 => Ok(JobStatus::Submitting),
            30 => Ok(JobStatus::Submitted),
            40 => Ok(JobStatus::Queued),
            50 => Ok(JobStatus::Running),
            60 => Ok(JobStatus::Cancelling),
            70 => Ok(JobStatus::Cancelled),
            80 => Ok(JobStatus::Deleting),
            90 => Ok(JobStatus::Deleted),
            400 => Ok(JobStatus::Error),
            401 => Ok(JobStatus::WallTimeExceeded),
            402 => Ok(JobStatus::OutOfMemory),
            500 => Ok(JobStatus::Completed),
            _ => Err(format!("Unknown JobStatus value: {value}")),
        }
    }
}

impl JobStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            JobStatus::Pending => "Pending",
            JobStatus::Submitting => "Submitting",
            JobStatus::Submitted => "Submitted",
            JobStatus::Queued => "Queued",
            JobStatus::Running => "Running",
            JobStatus::Cancelling => "Cancelling",
            JobStatus::Cancelled => "Cancelled",
            JobStatus::Deleting => "Deleting",
            JobStatus::Deleted => "Deleted",
            JobStatus::Error => "Error",
            JobStatus::WallTimeExceeded => "Wall Time Exceeded",
            JobStatus::OutOfMemory => "Out of Memory",
            JobStatus::Completed => "Completed",
        }
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Role of a cluster WebSocket connection.
///
/// Determines which message handlers are active and how the connection
/// is labelled in logs (`as_str()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterRole {
    /// Primary scheduler connection for job submission and updates.
    Master,
    /// Dedicated connection for streaming file downloads to clients.
    FileDownload,
    /// Dedicated connection for receiving file uploads from clients.
    FileUpload,
}

impl ClusterRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ClusterRole::Master => "master",
            ClusterRole::FileDownload => "file download",
            ClusterRole::FileUpload => "file upload",
        }
    }
}

impl std::fmt::Display for ClusterRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Message queue priority for outbound WebSocket traffic.
///
/// Wire-compatible `u8` values shared with the C++ orchestrator.
/// Lower numeric values indicate higher scheduling priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Priority {
    /// Highest priority (= 0).
    Highest = 0,
    /// Normal priority (= 10).
    Medium = 10,
    /// Lowest priority (= 19).
    Lowest = 19,
}

impl Priority {
    /// Wire-protocol queue key for this priority level.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Metadata for a single file in a directory listing.
///
/// Serialized with specific field names (`path`, `fileSize`, `isDir`)
/// for compatibility with the HTTP API response format.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileInfo {
    /// File name (including relative path from the job directory).
    #[serde(rename = "path")]
    pub file_name: String,
    /// File size in bytes.
    #[serde(rename = "fileSize")]
    pub file_size: u64,
    /// Unix permission bits (e.g., `0o644`).
    pub permissions: u32,
    /// Whether this entry is a directory.
    #[serde(rename = "isDir")]
    pub is_directory: bool,
}

/// Shared state for a file list request.
/// Used to coordinate between HTTP handler (waiting) and WebSocket handler (providing data).
pub struct FileListState {
    /// Collected file entries from the remote cluster (empty until the list completes).
    pub files: Vec<FileInfo>,
    /// Whether the remote cluster reported an error for this list request.
    pub error: bool,
    /// Human-readable error message when `error` is true.
    pub error_details: String,
    /// Whether the WebSocket handler has finished populating `files` or set `error`.
    pub data_ready: bool,
    /// Wakes the HTTP handler when `data_ready` becomes true.
    pub notify: std::sync::Arc<tokio::sync::Notify>,
}

impl FileListState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            error: false,
            error_details: String::new(),
            data_ready: false,
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl Default for FileListState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_try_from_valid() {
        assert_eq!(JobStatus::try_from(10).unwrap(), JobStatus::Pending);
        assert_eq!(JobStatus::try_from(20).unwrap(), JobStatus::Submitting);
        assert_eq!(JobStatus::try_from(30).unwrap(), JobStatus::Submitted);
        assert_eq!(JobStatus::try_from(40).unwrap(), JobStatus::Queued);
        assert_eq!(JobStatus::try_from(50).unwrap(), JobStatus::Running);
        assert_eq!(JobStatus::try_from(60).unwrap(), JobStatus::Cancelling);
        assert_eq!(JobStatus::try_from(70).unwrap(), JobStatus::Cancelled);
        assert_eq!(JobStatus::try_from(80).unwrap(), JobStatus::Deleting);
        assert_eq!(JobStatus::try_from(90).unwrap(), JobStatus::Deleted);
        assert_eq!(JobStatus::try_from(400).unwrap(), JobStatus::Error);
        assert_eq!(
            JobStatus::try_from(401).unwrap(),
            JobStatus::WallTimeExceeded
        );
        assert_eq!(JobStatus::try_from(402).unwrap(), JobStatus::OutOfMemory);
        assert_eq!(JobStatus::try_from(500).unwrap(), JobStatus::Completed);
    }

    #[test]
    fn test_job_status_try_from_invalid() {
        assert!(JobStatus::try_from(0).is_err());
        assert!(JobStatus::try_from(1).is_err());
        assert!(JobStatus::try_from(999).is_err());
    }

    #[test]
    fn test_job_status_repr_roundtrip() {
        let status = JobStatus::Running;
        let value = status as u32;
        assert_eq!(value, 50);
        assert_eq!(JobStatus::try_from(value).unwrap(), status);
    }

    #[test]
    fn test_job_status_as_str() {
        assert_eq!(JobStatus::Pending.as_str(), "Pending");
        assert_eq!(JobStatus::WallTimeExceeded.as_str(), "Wall Time Exceeded");
        assert_eq!(JobStatus::Completed.as_str(), "Completed");
    }

    #[test]
    fn test_job_status_display() {
        assert_eq!(JobStatus::Pending.to_string(), "Pending");
        assert_eq!(
            JobStatus::WallTimeExceeded.to_string(),
            "Wall Time Exceeded"
        );
        assert_eq!(JobStatus::Completed.to_string(), "Completed");
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Highest < Priority::Medium);
        assert!(Priority::Medium < Priority::Lowest);
    }

    #[test]
    fn test_priority_as_u8() {
        assert_eq!(Priority::Highest.as_u8(), 0);
        assert_eq!(Priority::Medium.as_u8(), 10);
        assert_eq!(Priority::Lowest.as_u8(), 19);
    }

    #[test]
    fn test_cluster_role_equality() {
        assert_eq!(ClusterRole::Master, ClusterRole::Master);
        assert_ne!(ClusterRole::Master, ClusterRole::FileDownload);
    }

    #[test]
    fn test_cluster_role_display() {
        assert_eq!(ClusterRole::Master.to_string(), "master");
        assert_eq!(ClusterRole::FileDownload.to_string(), "file download");
        assert_eq!(ClusterRole::FileUpload.to_string(), "file upload");
    }

    #[test]
    fn test_file_info() {
        let info = FileInfo {
            file_name: "test.txt".to_string(),
            file_size: 1024,
            permissions: 0o644,
            is_directory: false,
        };
        assert_eq!(info.file_name, "test.txt");
        assert_eq!(info.file_size, 1024);
        assert!(!info.is_directory);
    }

    #[test]
    fn test_file_list_state_default() {
        let state = FileListState::new();
        assert!(state.files.is_empty());
        assert!(!state.error);
        assert!(!state.data_ready);
        assert!(state.error_details.is_empty());
    }
}
