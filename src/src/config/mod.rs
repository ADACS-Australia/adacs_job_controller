pub mod access_secrets;
pub mod clusters;
pub mod settings;

use std::path::Path;

/// Read a JSON file and deserialize it into a `Vec<T>`, logging the read and item count.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the JSON is invalid.
pub fn load_json_file<T>(path: &Path, label: &str) -> anyhow::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    tracing::debug!("Loading {} from: {}", label, path.display());
    let content = std::fs::read_to_string(path)?;
    tracing::trace!("{} file read ({} bytes)", label, content.len());
    let items: Vec<T> = serde_json::from_str(&content)?;
    tracing::info!("Loaded {} {}", items.len(), label);
    Ok(items)
}
