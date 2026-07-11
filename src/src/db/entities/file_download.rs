use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverFiledownload` table.
///
/// Tracks file download events from job bundles on HPC clusters.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_filedownload")]
pub struct Model {
    /// Primary key — auto-incrementing row identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Foreign key referencing the user who initiated the download.
    pub user: i64,
    /// Foreign key referencing the job associated with the download.
    pub job: i64,
    /// Cluster identifier where the source job ran.
    pub cluster: String,
    /// Bundle name within the job.
    pub bundle: String,
    /// UUID identifying the specific file within the bundle.
    pub uuid: String,
    /// Filesystem path of the downloaded file on the cluster.
    #[sea_orm(column_type = "Text")]
    pub path: String,
    /// Timestamp when the download event was recorded.
    pub timestamp: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
