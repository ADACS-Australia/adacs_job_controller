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
