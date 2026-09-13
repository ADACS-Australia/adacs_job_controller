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

use std::path::Path;

/// Read a JSON file and deserialize it into a `Vec<T>`, logging the read and item count.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the JSON is invalid.
pub fn load_json_file<T>(path: &Path, label: &str, trace_msg: &str) -> anyhow::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    tracing::debug!("Loading {} from: {}", label, path.display());
    let content = std::fs::read_to_string(path)?;
    tracing::trace!("{}", format_args!(trace_msg, content.len()));
    let items: Vec<T> = serde_json::from_str(&content)?;
    tracing::info!("Loaded {} {}", items.len(), label);
    Ok(items)
}
