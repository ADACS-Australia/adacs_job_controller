use sea_orm_migration::prelude::*;

/// Migration to add the `cluster` column to the `jobserver_clusterjobstatus` table.
///
/// Scopes each status row to the originating cluster so a cluster only reads and
/// writes its own status entries.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager
            .has_column("jobserver_clusterjobstatus", "cluster")
            .await?
        {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("jobserver_clusterjobstatus"))
                        .add_column(
                            ColumnDef::new(Alias::new("cluster"))
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("jobserver_clusterjobstatus"))
                    .drop_column(Alias::new("cluster"))
                    .to_owned(),
            )
            .await
    }
}
