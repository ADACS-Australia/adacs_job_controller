mod common;

use adacs_job_controller::db::migration::migrator::Migrator;
use common::make_db;
use sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

#[tokio::test]
async fn test_all_migrations_up() {
    let db = make_db().await;

    Migrator::up(&db, None)
        .await
        .expect("Migrations should succeed");

    let expected_tables = [
        "jobserver_job",
        "jobserver_jobhistory",
        "jobserver_filedownload",
        "jobserver_filelistcache",
        "jobserver_clusterjob",
        "jobserver_clusterjobstatus",
        "jobserver_bundlejob",
        "jobserver_clusteruuid",
    ];

    for table_name in &expected_tables {
        let stmt = sea_orm::Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{table_name}'"
            ),
        );
        let result = db.query_one(stmt).await.expect("Query should succeed");
        let count: i64 = result
            .and_then(|r| r.try_get::<i64>("", "COUNT(*)").ok())
            .unwrap_or(0);
        assert_eq!(
            count, 1,
            "Table '{table_name}' should exist exactly once after migration",
        );
    }
}

#[tokio::test]
async fn test_migrations_idempotent() {
    let db = make_db().await;

    Migrator::up(&db, None)
        .await
        .expect("First migration run should succeed");
    Migrator::up(&db, None)
        .await
        .expect("Second migration run should succeed (idempotent)");
}

#[tokio::test]
async fn test_migrations_down() {
    let db = make_db().await;

    Migrator::up(&db, None)
        .await
        .expect("Migrations should succeed");
    Migrator::down(&db, None)
        .await
        .expect("Rollback should succeed");

    let stmt = sea_orm::Statement::from_string(
        DbBackend::Sqlite,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE 'jobserver_%'"
            .to_string(),
    );
    let result = db.query_one(stmt).await.expect("Query should succeed");
    let count: i64 = result
        .and_then(|r| r.try_get::<i64>("", "COUNT(*)").ok())
        .unwrap_or(0);
    assert_eq!(
        count, 0,
        "No jobserver tables should remain after full rollback"
    );
}
