/// Creates all tables in an in-memory `SQLite` database for testing.
///
/// # Panics
///
/// Panics if the test schema table cannot be created.
#[cfg(feature = "test-support")]
#[allow(dead_code)]
pub async fn create_test_schema(db: &sea_orm::DatabaseConnection) {
    use sea_orm::{ConnectionTrait, DbBackend, Schema};

    tracing::trace!("DB: Creating test schema");
    let builder = DbBackend::Sqlite;
    let schema = Schema::new(builder);

    let stmts = [
        builder.build(&schema.create_table_from_entity(super::entities::job::Entity)),
        builder.build(&schema.create_table_from_entity(super::entities::job_history::Entity)),
        builder.build(&schema.create_table_from_entity(super::entities::file_download::Entity)),
        builder.build(&schema.create_table_from_entity(super::entities::file_list_cache::Entity)),
        builder.build(&schema.create_table_from_entity(super::entities::cluster_job::Entity)),
        builder
            .build(&schema.create_table_from_entity(super::entities::cluster_job_status::Entity)),
        builder.build(&schema.create_table_from_entity(super::entities::bundle_job::Entity)),
        builder.build(&schema.create_table_from_entity(super::entities::cluster_uuid::Entity)),
    ];

    for stmt in stmts {
        db.execute(stmt)
            .await
            .expect("failed to create test schema table");
    }
}
