use sea_orm::entity::prelude::*;

/// SeaORM entity for the `JobserverFilelistcache` table.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "JobserverFilelistcache")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(column_name = "jobId")]
    pub job_id: i64,
    #[sea_orm(column_type = "Text")]
    pub path: String,
    #[sea_orm(column_name = "isDir")]
    pub is_dir: bool,
    #[sea_orm(column_name = "fileSize")]
    pub file_size: i64,
    pub permissions: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
