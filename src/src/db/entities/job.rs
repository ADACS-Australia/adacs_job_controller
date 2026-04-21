use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "JobserverJob")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub user: i64,
    #[sea_orm(column_type = "Text")]
    pub parameters: String,
    pub cluster: String,
    pub bundle: String,
    pub application: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
