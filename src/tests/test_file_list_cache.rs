//! Integration tests for `replace_file_list` cache replacement semantics.
//!
//! `replace_file_list` is the single write path for the file-list cache. It
//! must delete any existing rows for a job before inserting one row per file,
//! and an empty file list must delete all existing rows (leaving the cache
//! empty rather than stale).

mod common;

use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use adacs_job_controller::db::entities::file_list_cache;
use adacs_job_controller::db::file_list_cache::replace_file_list;
use adacs_job_controller::protocol::types::FileInfo;

use common::setup_test_db;

fn file(name: &str, is_dir: bool) -> FileInfo {
    FileInfo {
        file_name: name.to_string(),
        file_size: 1024,
        permissions: 0o644,
        is_directory: is_dir,
    }
}

async fn cached_paths(db: &sea_orm::DatabaseConnection, job_id: i64) -> Vec<String> {
    let mut rows = file_list_cache::Entity::find()
        .filter(file_list_cache::Column::JobId.eq(job_id))
        .all(db)
        .await
        .unwrap();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows.into_iter().map(|r| r.path).collect()
}

async fn insert_cache_row(db: &sea_orm::DatabaseConnection, job_id: i64, path: &str) {
    file_list_cache::ActiveModel {
        job_id: Set(job_id),
        path: Set(path.to_string()),
        is_dir: Set(false),
        file_size: Set(1024),
        permissions: Set(0o644),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

/// Replacing a file list must delete all existing rows for the job and insert
/// exactly the new entries (delete-then-insert replacement semantics).
#[tokio::test]
async fn test_replace_file_list_deletes_then_inserts() {
    let db = setup_test_db().await;
    let job_id = 42;

    insert_cache_row(&db, job_id, "/old/one.txt").await;
    insert_cache_row(&db, job_id, "/old/two.txt").await;

    replace_file_list(
        &db,
        job_id,
        &[file("/out/results.txt", false), file("/out/", true)],
    )
    .await;

    assert_eq!(
        cached_paths(&db, job_id).await,
        vec!["/out/".to_string(), "/out/results.txt".to_string()]
    );
}

/// An empty file list must delete all existing rows for the job.
#[tokio::test]
async fn test_replace_file_list_empty_deletes_all_rows() {
    let db = setup_test_db().await;
    let job_id = 7;

    insert_cache_row(&db, job_id, "/old/one.txt").await;
    insert_cache_row(&db, job_id, "/old/two.txt").await;

    replace_file_list(&db, job_id, &[]).await;

    assert!(cached_paths(&db, job_id).await.is_empty());
}

/// Replacing one job's cache must not touch another job's rows.
#[tokio::test]
async fn test_replace_file_list_only_affects_target_job() {
    let db = setup_test_db().await;
    let other_job = 1;
    let target_job = 2;

    insert_cache_row(&db, other_job, "/other/keep.txt").await;

    replace_file_list(&db, target_job, &[file("/target/new.txt", false)]).await;

    assert_eq!(
        cached_paths(&db, other_job).await,
        vec!["/other/keep.txt".to_string()]
    );
    assert_eq!(
        cached_paths(&db, target_job).await,
        vec!["/target/new.txt".to_string()]
    );
}
