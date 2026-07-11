use sea_orm::entity::prelude::*;

/// Tracks status updates for jobs running on a cluster.
///
/// Each row records a single status event: what action occurred and
/// the resulting state, linked to the originating job.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_clusterjobstatus")]
pub struct Model {
    /// Primary key of the status record.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// ID of the job this status update belongs to.
    pub job_id: i64,
    /// Human-readable label describing the status event (e.g. "submitted", "completed").
    pub what: String,
    /// Numeric state code representing the job's status at this point.
    pub state: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
