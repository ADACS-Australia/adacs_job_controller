pub mod access_secrets;
pub mod clusters;
pub mod settings;

/// Builds the "Loaded N cluster configurations" log message.
///
/// Shared by the config loader and `app.rs` so the format string stays in sync.
#[must_use]
pub fn loaded_cluster_configs(count: usize) -> String {
    format!("Loaded {count} cluster configurations")
}

/// Builds the "Loaded N access secrets" log message.
///
/// Shared by the config loader and `app.rs` so the format string stays in sync.
#[must_use]
pub fn loaded_access_secrets(count: usize) -> String {
    format!("Loaded {count} access secrets")
}

/// Read a JSON file and deserialize it into a `Vec<T>`, logging the read and item count.
///
/// `trace` must be a string literal (e.g. `"Access secrets file read ({} bytes)"`).
///
/// # Errors
///
/// Returns an error if the file cannot be read or the JSON is invalid.
macro_rules! load_json_file {
    ($path:expr, $label:expr, $trace:literal) => {{
        tracing::debug!("Loading {} from: {}", $label, $path.display());
        let content = std::fs::read_to_string($path)?;
        tracing::trace!($trace, content.len());
        let items: Vec<_> = serde_json::from_str(&content)?;
        tracing::info!("Loaded {} {}", items.len(), $label);
        items
    }};
}

pub(crate) use load_json_file;
