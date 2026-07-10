use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverJob` table.
///
/// Stores the controller-side job record created when a client submits a job via HTTP.
/// The row is inserted in the same transaction as the initial `Pending` history entry.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_job")]
pub struct Model {
    /// Auto-increment primary key; also used as the client-visible job ID.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Submitting user ID from the JWT `userId` claim (0 if absent).
    pub user: i64,
    /// Opaque job parameters forwarded to the remote cluster scheduler.
    #[sea_orm(column_type = "Text")]
    pub parameters: String,
    /// Target cluster name; must match a configured cluster and JWT access list.
    pub cluster: String,
    /// Bundle identifier or payload reference passed to the cluster on submit.
    pub bundle: String,
    /// JWT application name that created the job (from `AccessSecret::name`).
    pub application: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
