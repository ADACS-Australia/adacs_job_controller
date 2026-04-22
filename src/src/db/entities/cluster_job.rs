use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "JobserverClusterjob")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(column_name = "jobId")]
    pub job_id: i64,
    #[sea_orm(column_name = "schedulerId")]
    pub scheduler_id: i64,
    pub submitting: bool,
    #[sea_orm(column_name = "submittingCount")]
    pub submitting_count: i32,
    #[sea_orm(column_type = "Text", column_name = "bundleHash")]
    pub bundle_hash: String,
    #[sea_orm(column_type = "Text", column_name = "workingDirectory")]
    pub working_directory: String,
    pub running: bool,
    pub deleting: bool,
    pub deleted: bool,
    pub cluster: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
