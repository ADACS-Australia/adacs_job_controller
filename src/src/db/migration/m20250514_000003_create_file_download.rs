use sea_orm::{ConnectionTrait, Schema};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let builder = manager.get_database_backend();
        let schema = Schema::new(builder);
        let stmt = builder.build(
            schema
                .create_table_from_entity(crate::db::entities::file_download::Entity)
                .if_not_exists(),
        );
        manager.get_connection().execute(stmt).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("jobserver_filedownload"))
                    .to_owned(),
            )
            .await
    }
}
