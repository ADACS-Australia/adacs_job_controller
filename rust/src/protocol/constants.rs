pub const SYSTEM_SOURCE: &str = "system";
pub const JOB_COMPLETION_SOURCE: &str = "_job_completion_";

// Server status
pub const SERVER_READY: u32 = 1000;

// Job operations
pub const SUBMIT_JOB: u32 = 2000;
pub const UPDATE_JOB: u32 = 2001;
pub const CANCEL_JOB: u32 = 2002;
pub const DELETE_JOB: u32 = 2003;

// File download operations
pub const DOWNLOAD_FILE: u32 = 4000;
pub const FILE_DETAILS: u32 = 4001;
pub const FILE_ERROR: u32 = 4002;
pub const FILE_CHUNK: u32 = 4003;
pub const PAUSE_FILE_CHUNK_STREAM: u32 = 4004;
pub const RESUME_FILE_CHUNK_STREAM: u32 = 4005;
pub const FILE_LIST: u32 = 4006;
pub const FILE_LIST_ERROR: u32 = 4007;

// File upload operations
pub const UPLOAD_FILE: u32 = 4500;
pub const FILE_UPLOAD_CHUNK: u32 = 4501;
pub const FILE_UPLOAD_ERROR: u32 = 4502;
pub const FILE_UPLOAD_COMPLETE: u32 = 4503;

// Database job operations
pub const DB_JOB_GET_BY_JOB_ID: u32 = 5000;
pub const DB_JOB_GET_BY_ID: u32 = 5001;
pub const DB_JOB_GET_RUNNING_JOBS: u32 = 5002;
pub const DB_JOB_DELETE: u32 = 5003;
pub const DB_JOB_SAVE: u32 = 5004;

// Database job status operations
pub const DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT: u32 = 6000;
pub const DB_JOBSTATUS_GET_BY_JOB_ID: u32 = 6001;
pub const DB_JOBSTATUS_DELETE_BY_ID_LIST: u32 = 6002;
pub const DB_JOBSTATUS_SAVE: u32 = 6003;

// Database response
pub const DB_RESPONSE: u32 = 7000;

// Bundle operations
pub const DB_BUNDLE_CREATE_OR_UPDATE_JOB: u32 = 8000;
pub const DB_BUNDLE_GET_JOB_BY_ID: u32 = 8001;
pub const DB_BUNDLE_DELETE_JOB: u32 = 8002;

// Test message IDs
#[cfg(feature = "test-support")]
#[allow(dead_code)]
pub const TEST_MESSAGE_ID_MIN: u32 = 1000000;
#[cfg(feature = "test-support")]
#[allow(dead_code)]
pub const TEST_MESSAGE_ID_MAX: u32 = 1001000;
