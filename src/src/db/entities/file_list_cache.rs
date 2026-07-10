use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverFilelistcache` table.
///
/// Persists cached directory listing entries for completed jobs. Rows are written
/// in the background after job completion and served on subsequent HTTP file-list
/// requests without contacting the remote cluster.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_filelistcache")]
pub struct Model {
    /// Auto-increment primary key in the file list cache table.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Controller job ID whose directory listing was cached.
    pub job_id: i64,
    /// Relative path of the file or directory within the job workspace.
    #[sea_orm(column_type = "Text")]
    pub path: String,
    /// Whether this entry represents a directory rather than a regular file.
    pub is_dir: bool,
    /// File size in bytes (0 for directories).
    pub file_size: i64,
    /// Unix-style permission bits from the remote cluster listing.
    pub permissions: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
