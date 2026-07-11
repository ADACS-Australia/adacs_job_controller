use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverJobhistory` table.
///
/// Stores audit records of job state transitions (who changed what, when, and to which state).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_jobhistory")]
pub struct Model {
    /// Surrogate primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Foreign key to the parent job (`jobserver_job.id`).
    pub job_id: i64,
    /// When this state transition was recorded.
    pub timestamp: DateTime,
    /// Source or step identifier (e.g. scheduler step name).
    pub what: String,
    /// Numeric job status at this transition (matches `JobStatus` wire values).
    pub state: i32,
    /// Optional human-readable details or error text for this transition.
    #[sea_orm(column_type = "Text")]
    pub details: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
