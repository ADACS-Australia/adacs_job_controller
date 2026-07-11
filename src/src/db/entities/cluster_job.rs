#![allow(clippy::pedantic)]
use sea_orm::entity::prelude::*;

/// A row in the `jobserver_clusterjob` table representing a job's state on a cluster.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_clusterjob")]
pub struct Model {
    /// Primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Foreign key to the parent job.
    pub job_id: i64,
    /// Scheduler-assigned identifier for this job on the cluster.
    pub scheduler_id: i64,
    /// Whether the job is currently being submitted to the cluster.
    pub submitting: bool,
    /// Number of times the job has been submitted (retry counter).
    pub submitting_count: i32,
    /// Hash of the job bundle stored on the cluster.
    #[sea_orm(column_type = "Text")]
    pub bundle_hash: String,
    /// Working directory path on the cluster filesystem.
    #[sea_orm(column_type = "Text")]
    pub working_directory: String,
    /// Whether the job is currently running on the cluster.
    pub running: bool,
    /// Whether the job is currently being deleted from the cluster.
    pub deleting: bool,
    /// Whether the job has been fully deleted from the cluster.
    pub deleted: bool,
    /// Name of the cluster this job belongs to.
    pub cluster: String,
}

#[allow(clippy::struct_excessive_bools)]
impl Model {}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
