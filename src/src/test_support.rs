pub const SQLITE_IN_MEMORY_CONNECTION_FAILED: &str = "sqlite in-memory connection failed";

//! Shared constants for tests (library unit tests and integration tests).
#![allow(dead_code)]

/// In-memory `SQLite` connection string used across test modules.
pub const SQLITE_MEMORY: &str = "sqlite::memory:";
