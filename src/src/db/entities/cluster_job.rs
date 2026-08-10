#![allow(clippy::pedantic)]
use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverClusterjob` table.
///
/// Persists per-cluster job lifecycle state synced from remote clusters via the
/// binary WebSocket protocol. Each row tracks scheduler submission, execution, and
/// deletion for one controller job on a named cluster.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_clusterjob")]
pub struct Model {
    /// Auto-increment primary key in the cluster job table.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Controller-assigned job identifier shared with HTTP clients.
    pub job_id: i64,
    /// Scheduler-specific job ID on the remote cluster.
    pub scheduler_id: i64,
    /// Whether the job is currently being submitted to the scheduler.
    pub submitting: bool,
    /// Number of in-flight submit attempts (used for retry tracking).
    pub submitting_count: i32,
    /// Content hash of the job bundle payload.
    #[sea_orm(column_type = "Text")]
    pub bundle_hash: String,
    /// Working directory on the cluster where the job runs.
    #[sea_orm(column_type = "Text")]
    pub working_directory: String,
    /// Whether the job is actively running on the cluster.
    pub running: bool,
    /// Whether a delete/cancel operation is in progress.
    pub deleting: bool,
    /// Whether the job record has been marked deleted on the cluster.
    pub deleted: bool,
    /// Cluster name this record belongs to (matches `ClusterConfig::name`).
    pub cluster: String,
}

#[allow(clippy::struct_excessive_bools)]
impl Model {}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
