//! Database migration definitions for the ADACS Job Controller.
//!
//! Each module defines a single `SeaORM` migration that creates one table.

pub mod migrator;

pub mod m20250514_000001_create_job;
pub mod m20250514_000002_create_job_history;
pub mod m20250514_000003_create_file_download;
pub mod m20250514_000004_create_file_list_cache;
pub mod m20250514_000005_create_cluster_job;
pub mod m20250514_000006_create_cluster_job_status;
pub mod m20250514_000007_create_bundle_job;
pub mod m20250514_000008_create_cluster_uuid;
