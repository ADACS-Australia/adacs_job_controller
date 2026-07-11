use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverJob` table.
///
/// Stores the core job record — one row per job submitted to any cluster.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_job")]
pub struct Model {
    /// Surrogate primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Foreign key referencing the submitting user.
    pub user: i64,
    /// Scheduler parameters string passed through to the cluster.
    #[sea_orm(column_type = "Text")]
    pub parameters: String,
    /// Name of the target cluster.
    pub cluster: String,
    /// Bundle identifier or hash for the job's executable bundle.
    pub bundle: String,
    /// Application name that submitted the job.
    pub application: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
