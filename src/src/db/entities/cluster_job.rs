use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_clusterjob")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub job_id: i64,
    pub scheduler_id: i64,
    pub submitting: bool,
    pub submitting_count: i32,
    #[sea_orm(column_type = "Text")]
    pub bundle_hash: String,
    #[sea_orm(column_type = "Text")]
    pub working_directory: String,
    pub running: bool,
    pub deleting: bool,
    pub deleted: bool,
    pub cluster: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
