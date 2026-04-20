use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "JobserverClusteruuid")]
#[allow(dead_code)]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub cluster: String,
    pub uuid: String,
    pub timestamp: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[allow(dead_code)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
