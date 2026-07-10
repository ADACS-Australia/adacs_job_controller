/// Source label for system-generated messages.
pub const SYSTEM_SOURCE: &str = "system";

/// Source label for job completion messages.
pub const JOB_COMPLETION_SOURCE: &str = "_job_completion_";

/// Message ID indicating the server is ready.
pub const SERVER_READY: u32 = 1000;

/// Message ID for submitting a new job.
pub const SUBMIT_JOB: u32 = 2000;

/// Message ID for updating an existing job.
pub const UPDATE_JOB: u32 = 2001;

/// Message ID for cancelling a job.
pub const CANCEL_JOB: u32 = 2002;

/// Message ID for deleting a job.
pub const DELETE_JOB: u32 = 2003;

/// Message ID for downloading a file.
pub const DOWNLOAD_FILE: u32 = 4000;

/// Message ID for file details response.
pub const FILE_DETAILS: u32 = 4001;

/// Message ID for file error response.
pub const FILE_ERROR: u32 = 4002;

/// Message ID for a file data chunk.
pub const FILE_CHUNK: u32 = 4003;

/// Message ID to pause a file chunk stream.
pub const PAUSE_FILE_CHUNK_STREAM: u32 = 4004;

/// Message ID to resume a file chunk stream.
pub const RESUME_FILE_CHUNK_STREAM: u32 = 4005;

/// Message ID for requesting a file list.
pub const FILE_LIST: u32 = 4006;

/// Message ID for file list error response.
pub const FILE_LIST_ERROR: u32 = 4007;

/// Message ID for uploading a file.
pub const UPLOAD_FILE: u32 = 4500;

/// Message ID for a file upload chunk.
pub const FILE_UPLOAD_CHUNK: u32 = 4501;

/// Message ID for file upload error response.
pub const FILE_UPLOAD_ERROR: u32 = 4502;

/// Message ID for file upload completion.
pub const FILE_UPLOAD_COMPLETE: u32 = 4503;

/// Message ID for database job lookup by job ID.
pub const DB_JOB_GET_BY_JOB_ID: u32 = 5000;

/// Message ID for database job lookup by internal ID.
pub const DB_JOB_GET_BY_ID: u32 = 5001;

/// Message ID for retrieving running jobs from the database.
pub const DB_JOB_GET_RUNNING_JOBS: u32 = 5002;

/// Message ID for deleting a job from the database.
pub const DB_JOB_DELETE: u32 = 5003;

/// Message ID for saving a job to the database.
pub const DB_JOB_SAVE: u32 = 5004;

/// Message ID for database job status lookup by job ID and what.
pub const DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT: u32 = 6000;

/// Message ID for database job status lookup by job ID.
pub const DB_JOBSTATUS_GET_BY_JOB_ID: u32 = 6001;

/// Message ID for deleting job statuses by ID list.
pub const DB_JOBSTATUS_DELETE_BY_ID_LIST: u32 = 6002;

/// Message ID for saving a job status to the database.
pub const DB_JOBSTATUS_SAVE: u32 = 6003;

/// Message ID for a generic database response.
pub const DB_RESPONSE: u32 = 7000;

/// Message ID for creating or updating a job bundle.
pub const DB_BUNDLE_CREATE_OR_UPDATE_JOB: u32 = 8000;

/// Message ID for retrieving a job bundle by ID.
pub const DB_BUNDLE_GET_JOB_BY_ID: u32 = 8001;

/// Message ID for deleting a job bundle.
pub const DB_BUNDLE_DELETE_JOB: u32 = 8002;

/// Minimum message ID reserved for test messages.
#[cfg(feature = "test-support")]
#[allow(dead_code)]
pub const TEST_MESSAGE_ID_MIN: u32 = 1_000_000;

/// Maximum message ID reserved for test messages.
#[cfg(feature = "test-support")]
#[allow(dead_code)]
pub const TEST_MESSAGE_ID_MAX: u32 = 1_001_000;
