use std::sync::LazyLock;

#[allow(dead_code)]
fn env_or(key: &str, default: &str) -> String {
    let value = std::env::var(key).unwrap_or_else(|_| default.to_string());
    tracing::trace!(
        "Config: {} = {}",
        key,
        if key.contains("SECRET") || key.contains("PASSWORD") {
            "***REDACTED***"
        } else {
            &value
        }
    );
    value
}

fn env_or_u16(key: &str, default: u16) -> u16 {
    let value = std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);
    tracing::trace!("Config: {} = {}", key, value);
    value
}

fn env_or_u32(key: &str, default: u32) -> u32 {
    let value = std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);
    tracing::trace!("Config: {} = {}", key, value);
    value
}

fn env_or_u64(key: &str, default: u64) -> u64 {
    let value = std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);
    tracing::trace!("Config: {} = {}", key, value);
    value
}

fn env_or_bool(key: &str, default: bool) -> bool {
    let value = std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);
    tracing::trace!("Config: {} = {}", key, value);
    value
}

// Database settings
#[allow(dead_code)]
pub static DATABASE_USER: LazyLock<String> = LazyLock::new(|| env_or("MYSQL_USER", "jobserver"));
#[allow(dead_code)]
pub static DATABASE_PASSWORD: LazyLock<String> =
    LazyLock::new(|| env_or("MYSQL_PASSWORD", "jobserver"));
#[allow(dead_code)]
pub static DATABASE_SCHEMA: LazyLock<String> =
    LazyLock::new(|| env_or("MYSQL_DATABASE", "jobserver"));
#[allow(dead_code)]
pub static DATABASE_HOST: LazyLock<String> = LazyLock::new(|| env_or("DATABASE_HOST", "localhost"));
#[allow(dead_code)]
pub static DATABASE_PORT: LazyLock<u16> = LazyLock::new(|| env_or_u16("DATABASE_PORT", 3306));
#[allow(dead_code)]
pub static DATABASE_DEBUG: LazyLock<bool> = LazyLock::new(|| env_or_bool("DATABASE_DEBUG", false));

// File download expiry
/// Seconds before an unused file-download record expires (`FILE_DOWNLOAD_EXPIRY_TIME` env var).
pub static FILE_DOWNLOAD_EXPIRY_TIME: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("FILE_DOWNLOAD_EXPIRY_TIME", 86400));

// Config file environment variable names
/// Environment variable for the cluster configuration JSON file path.
pub const CLUSTER_CONFIG_FILE_ENV_VARIABLE: &str = "CLUSTER_CONFIG_FILE";
/// Environment variable for the JWT access-secrets JSON file path.
pub const ACCESS_SECRET_CONFIG_FILE_ENV_VARIABLE: &str = "ACCESS_SECRET_CONFIG_FILE";

// LTK security settings
/// Milliseconds to wait for an LTK WebSocket handshake before timing out (`LTK_CONNECTION_TIMEOUT_MS`).
pub static LTK_CONNECTION_TIMEOUT_MS: LazyLock<u32> = LazyLock::new(|| {
    #[cfg(test)]
    {
        std::env::var("LTK_CONNECTION_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }
    #[cfg(not(test))]
    {
        std::env::var("LTK_CONNECTION_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000)
    }
});

// File buffer sizes (bytes)
/// Maximum in-memory buffer for streaming file transfers (`MAX_FILE_BUFFER_SIZE`).
pub static MAX_FILE_BUFFER_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("MAX_FILE_BUFFER_SIZE", 50 * 1024 * 1024));
/// Minimum buffer size before a file chunk is flushed (`MIN_FILE_BUFFER_SIZE`).
pub static MIN_FILE_BUFFER_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("MIN_FILE_BUFFER_SIZE", 10 * 1024 * 1024));
/// Size of each file chunk sent over the WebSocket protocol (`FILE_CHUNK_SIZE`).
pub static FILE_CHUNK_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("FILE_CHUNK_SIZE", 64 * 1024));

// Queue and cluster timing
/// Interval between pruning stale message-queue sources (`QUEUE_SOURCE_PRUNE_MILLISECONDS`).
pub static QUEUE_SOURCE_PRUNE_MILLISECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("QUEUE_SOURCE_PRUNE_MILLISECONDS", 60000));
/// Interval for resending unacknowledged cluster messages (`CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS`).
pub static CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS", 60000));
/// HTTP client timeout for waiting on cluster responses (`CLIENT_TIMEOUT_SECONDS`).
pub static CLIENT_TIMEOUT_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLIENT_TIMEOUT_SECONDS", 30));

// Message settings
/// Initial capacity for binary `Message` buffers (`MESSAGE_INITIAL_VECTOR_SIZE`).
pub static MESSAGE_INITIAL_VECTOR_SIZE: LazyLock<u32> =
    LazyLock::new(|| env_or_u32("MESSAGE_INITIAL_VECTOR_SIZE", 65536));

// Cluster state
/// Grace period before re-applying recent job-state updates from a reconnecting cluster (`CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS`).
pub static CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS", 60));

// Cluster manager settings
/// Seconds between SSH reconnect attempts for offline clusters (`CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS`).
pub static CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS", 60));
/// Seconds between WebSocket keep-alive pings to connected clusters (`CLUSTER_MANAGER_PING_INTERVAL_SECONDS`).
pub static CLUSTER_MANAGER_PING_INTERVAL_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLUSTER_MANAGER_PING_INTERVAL_SECONDS", 10));
#[allow(dead_code)]
pub static CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS", 60));
#[allow(dead_code)]
pub static CLUSTER_MANAGER_MANUAL_TOKEN_EXPIRY_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLUSTER_MANAGER_MANUAL_TOKEN_EXPIRY_SECONDS", 600));
pub static CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS", 600));

// HTTP settings
/// TCP port for the HTTP REST API (`HTTP_PORT`).
pub static HTTP_PORT: LazyLock<u16> = LazyLock::new(|| env_or_u16("HTTP_PORT", 8000));
// Disabled by default (set env var to enable). PeerIpKeyExtractor in tower_governor
// fails on requests without a peer socket address (e.g., integration tests).
/// Sustained request rate limit; `0` disables rate limiting (`RATE_LIMIT_REQUESTS_PER_SECOND`).
pub static RATE_LIMIT_REQUESTS_PER_SECOND: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("RATE_LIMIT_REQUESTS_PER_SECOND", 0));
/// Burst allowance above the sustained rate (`RATE_LIMIT_BURST_SIZE`).
pub static RATE_LIMIT_BURST_SIZE: LazyLock<u32> =
    LazyLock::new(|| env_or_u32("RATE_LIMIT_BURST_SIZE", 50));
#[allow(dead_code)]
pub static HTTP_WORKER_POOL_SIZE: LazyLock<u32> =
    LazyLock::new(|| env_or_u32("HTTP_WORKER_POOL_SIZE", 1024));
#[allow(dead_code)]
pub static HTTP_CONTENT_TIMEOUT_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_or_u64("HTTP_CONTENT_TIMEOUT_SECONDS", 86400));

// WebSocket settings
/// TCP port for cluster WebSocket connections (`WEBSOCKET_PORT`).
pub static WEBSOCKET_PORT: LazyLock<u16> = LazyLock::new(|| env_or_u16("WEBSOCKET_PORT", 8001));
#[allow(dead_code)]
pub static WEBSOCKET_WORKER_POOL_SIZE: LazyLock<u32> =
    LazyLock::new(|| env_or_u32("WEBSOCKET_WORKER_POOL_SIZE", 1024));

// Bundle HTTP settings
#[allow(dead_code)]
pub static BUNDLE_HTTP_PORT: LazyLock<String> = LazyLock::new(|| env_or("BUNDLE_HTTP_PORT", ":80"));
#[allow(dead_code)]
pub static BUNDLE_HTTPS_PORT: LazyLock<String> =
    LazyLock::new(|| env_or("BUNDLE_HTTPS_PORT", ":443"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_port_and_debug() {
        // DATABASE_PORT and DATABASE_DEBUG are not typically overridden in .env
        assert_eq!(*DATABASE_PORT, 3306);
        assert!(!*DATABASE_DEBUG);
    }

    #[test]
    fn test_file_defaults() {
        assert_eq!(*FILE_DOWNLOAD_EXPIRY_TIME, 86400);
        assert_eq!(*MAX_FILE_BUFFER_SIZE, 50 * 1024 * 1024);
        assert_eq!(*MIN_FILE_BUFFER_SIZE, 10 * 1024 * 1024);
        assert_eq!(*FILE_CHUNK_SIZE, 64 * 1024);
    }

    #[test]
    fn test_config_file_env_variable_names() {
        assert_eq!(CLUSTER_CONFIG_FILE_ENV_VARIABLE, "CLUSTER_CONFIG_FILE");
        assert_eq!(
            ACCESS_SECRET_CONFIG_FILE_ENV_VARIABLE,
            "ACCESS_SECRET_CONFIG_FILE"
        );
    }

    #[test]
    fn test_timing_defaults() {
        assert_eq!(*QUEUE_SOURCE_PRUNE_MILLISECONDS, 60000);
        assert_eq!(*CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS, 60000);
        assert_eq!(*CLIENT_TIMEOUT_SECONDS, 30);
    }

    #[test]
    fn test_message_defaults() {
        assert_eq!(*MESSAGE_INITIAL_VECTOR_SIZE, 65536);
    }

    #[test]
    fn test_cluster_defaults() {
        assert_eq!(*CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS, 60);
        assert_eq!(*CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS, 60);
        assert_eq!(*CLUSTER_MANAGER_PING_INTERVAL_SECONDS, 10);
        assert_eq!(*CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS, 60);
        assert_eq!(*CLUSTER_MANAGER_MANUAL_TOKEN_EXPIRY_SECONDS, 600);
        assert_eq!(*CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS, 600);
    }

    #[test]
    fn test_http_defaults() {
        assert_eq!(*HTTP_PORT, 8000);
        assert_eq!(*HTTP_WORKER_POOL_SIZE, 1024);
        assert_eq!(*HTTP_CONTENT_TIMEOUT_SECONDS, 86400);
    }

    #[test]
    fn test_websocket_defaults() {
        assert_eq!(*WEBSOCKET_PORT, 8001);
        assert_eq!(*WEBSOCKET_WORKER_POOL_SIZE, 1024);
    }

    #[test]
    fn test_bundle_defaults() {
        assert_eq!(BUNDLE_HTTP_PORT.as_str(), ":80");
        assert_eq!(BUNDLE_HTTPS_PORT.as_str(), ":443");
    }

    #[test]
    fn test_env_or_helpers() {
        assert_eq!(env_or("__NONEXISTENT_TEST_VAR__", "fallback"), "fallback");
        assert_eq!(env_or_u16("__NONEXISTENT_TEST_VAR__", 42), 42);
        assert_eq!(env_or_u32("__NONEXISTENT_TEST_VAR__", 999), 999);
        assert_eq!(env_or_u64("__NONEXISTENT_TEST_VAR__", 123_456), 123_456);
        assert!(!env_or_bool("__NONEXISTENT_TEST_VAR__", false));
    }
}
