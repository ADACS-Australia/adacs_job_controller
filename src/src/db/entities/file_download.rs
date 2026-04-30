use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverFiledownload` table.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "JobserverFiledownload")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub user: i32,
    pub job: i32,
    pub cluster: String,
    pub bundle: String,
    pub uuid: String,
    #[sea_orm(column_type = "Text")]
    pub path: String,
    pub timestamp: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
