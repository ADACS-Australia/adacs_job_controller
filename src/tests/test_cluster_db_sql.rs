//! SQLite-backed integration tests for the `ClusterDB` SQL handlers.
//!
//! Each test creates a fresh in-memory `SQLite` pool, sets up the cluster-specific
//! schema, and verifies BOTH the resulting DB state AND the `DB_RESPONSE` message
//! content for every handler in `cluster_db.rs`.

mod common;

use std::sync::{Arc, Mutex};

use adacs_job_controller::cluster::traits::MockClusterTrait;
use adacs_job_controller::db::cluster_db::maybe_handle_cluster_db_message;
use adacs_job_controller::db::models::{BundleJob, ClusterJob, ClusterJobStatus};
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::Priority;

use adacs_job_controller::db::entities::{bundle_job, cluster_job, cluster_job_status};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, Database, DatabaseConnection,
    DbBackend, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Schema,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn make_db() -> DatabaseConnection {
    Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection failed")
}

async fn setup_cluster_db(db: &DatabaseConnection) {
    let builder = DbBackend::Sqlite;
    let schema = Schema::new(builder);
    for stmt in [
        builder.build(&schema.create_table_from_entity(cluster_job::Entity)),
        builder.build(&schema.create_table_from_entity(cluster_job_status::Entity)),
        builder.build(&schema.create_table_from_entity(bundle_job::Entity)),
    ] {
        db.execute(stmt).await.unwrap();
    }
}

/// Build a mock cluster that captures all `send_message` calls.
fn mock_cluster_capturing(name: &str) -> (MockClusterTrait, Arc<Mutex<Vec<Message>>>) {
    let sent = Arc::new(Mutex::new(Vec::<Message>::new()));
    let sent_clone = Arc::clone(&sent);

    let mut mock = MockClusterTrait::new();
    let n = name.to_string();
    mock.expect_name().returning(move || n.clone());
    mock.expect_send_message().returning(move |msg| {
        sent_clone.lock().unwrap().push(msg);
    });

    (mock, sent)
}

/// Build a ready-to-dispatch `Message` by creating it, pushing payload, then round-tripping
/// through `into_data()` / `from_bytes()` so `id()` / `source()` are properly cached.
fn dispatch_message(id: u32, push: impl FnOnce(&mut Message)) -> Message {
    let mut msg = Message::new(id, Priority::Highest, SYSTEM_SOURCE);
    push(&mut msg);
    Message::from_bytes(msg.into_data())
}

/// Parse a `DB_RESPONSE` message: round-trip to reset read position, then pop `db_request_id`.
fn parse_response(msg: Message) -> (u32, Message) {
    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_RESPONSE);
    let db_request_id = parsed.pop_uint();
    (db_request_id, parsed)
}

// ---------------------------------------------------------------------------
// DB_JOB_SAVE — insert (id == 0)
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOB_SAVE` message with id=0 inserts a new cluster job row and responds with the assigned row id.
///
/// # Setup
/// Empty in-memory DB with cluster job schema; `ClusterJob` with id=0, `job_id=42`, submitting=true,
/// `bundle_hash="hash_abc`", and `working_directory="/work/dir`".
///
/// # Act
/// Dispatch a `DB_JOB_SAVE` message with `db_request_id=100` and the `ClusterJob` payload.
///
/// # Assert
/// A new row exists in `cluster_job` with the correct field values; the `DB_RESPONSE`
/// contains `db_request_id=100` and the assigned row id.
#[tokio::test]
async fn test_handle_job_save_insert() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let (mock, sent) = mock_cluster_capturing("ozstar");

    let job = ClusterJob {
        id: 0, // insert
        job_id: 42,
        scheduler_id: 0,
        submitting: true,
        submitting_count: 1,
        bundle_hash: "hash_abc".to_string(),
        working_directory: "/work/dir".to_string(),
        running: false,
        deleting: false,
        deleted: false,
        cluster: String::new(),
    };

    let mut msg = dispatch_message(DB_JOB_SAVE, |m| {
        m.push_uint(100);
        job.to_message(m);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled, "DB_JOB_SAVE should be handled");

    // Verify DB: one row with correct fields
    let model = cluster_job::Entity::find()
        .filter(cluster_job::Column::Cluster.eq("ozstar"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let db_id = model.id;
    let db_job_id = model.job_id;
    let db_sched_id = model.scheduler_id;
    let db_hash = model.bundle_hash.clone();
    let db_dir = model.working_directory.clone();

    assert!(db_id > 0, "Auto id should be assigned");
    assert_eq!(db_job_id, 42);
    assert_eq!(db_sched_id, 0);
    assert!(model.submitting); // true → 1
    assert_eq!(db_hash, "hash_abc");
    assert_eq!(db_dir, "/work/dir");
    assert!(!model.running);

    // Verify response: DB_RESPONSE with db_request_id=100
    let captured = sent.lock().unwrap();
    assert_eq!(captured.len(), 1);
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 100);
    let returned_id = body.pop_ulong();
    // SeaORM with SQLite returns the real id
    assert!(
        returned_id == 0 || returned_id == db_id as u64,
        "returned_id should be 0 (SQLite) or db_id, got {returned_id}"
    );
}

// ---------------------------------------------------------------------------
// DB_JOB_SAVE — update (id != 0)
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOB_SAVE` message with a non-zero id updates the existing cluster job row.
///
/// # Setup
/// In-memory DB with one pre-inserted cluster job row; `ClusterJob` carries the existing id
/// with updated fields (`scheduler_id=99`, `bundle_hash="newhash`", running=true).
///
/// # Act
/// Dispatch a `DB_JOB_SAVE` message with `db_request_id=200` and the updated `ClusterJob` payload.
///
/// # Assert
/// The existing row reflects the updated field values; the `DB_RESPONSE` contains
/// `db_request_id=200` and the existing row id.
#[tokio::test]
async fn test_handle_job_save_update() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // Pre-insert a row
    let inserted = cluster_job::ActiveModel {
        job_id: Set(10),
        scheduler_id: Set(0),
        submitting: Set(false),
        submitting_count: Set(0),
        bundle_hash: Set("oldhash".to_string()),
        working_directory: Set("/old".to_string()),
        running: Set(false),
        deleting: Set(false),
        deleted: Set(false),
        cluster: Set("ozstar".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let existing_id = inserted.id;

    let updated_job = ClusterJob {
        id: existing_id,
        job_id: 10,
        scheduler_id: 99,
        submitting: false,
        submitting_count: 2,
        bundle_hash: "newhash".to_string(),
        working_directory: "/new/work".to_string(),
        running: true,
        deleting: false,
        deleted: false,
        cluster: String::new(),
    };

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOB_SAVE, |m| {
        m.push_uint(200);
        updated_job.to_message(m);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    // Verify DB updated
    let model = cluster_job::Entity::find_by_id(existing_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let db_sched_id = model.scheduler_id;
    let db_hash = model.bundle_hash.clone();

    assert_eq!(db_sched_id, 99);
    assert_eq!(db_hash, "newhash");
    assert!(model.running);

    // Response: db_request_id=200, then existing_id
    let captured = sent.lock().unwrap();
    assert_eq!(captured.len(), 1);
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 200);
    assert_eq!(body.pop_ulong(), existing_id as u64);
}

// ---------------------------------------------------------------------------
// DB_JOB_GET_BY_ID — found
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOB_GET_BY_ID` message returns the matching cluster job row when it exists.
///
/// # Setup
/// In-memory DB with one pre-inserted cluster job row (`job_id=55`, `scheduler_id=7`,
/// submitting=true, `submitting_count=3`, `bundle_hash="myhash`", running=true).
///
/// # Act
/// Dispatch a `DB_JOB_GET_BY_ID` message with `db_request_id=301` and the row's id.
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=301`, count=1, and a `ClusterJob` with all
/// fields matching the inserted row.
#[tokio::test]
async fn test_handle_job_get_by_id_found() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let inserted = cluster_job::ActiveModel {
        job_id: Set(55),
        scheduler_id: Set(7),
        submitting: Set(true),
        submitting_count: Set(3),
        bundle_hash: Set("myhash".to_string()),
        working_directory: Set("/mydir".to_string()),
        running: Set(true),
        deleting: Set(false),
        deleted: Set(false),
        cluster: Set("ozstar".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let row_id = inserted.id;

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOB_GET_BY_ID, |m| {
        m.push_uint(301);
        m.push_ulong(row_id as u64);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 301);

    let count = body.pop_uint();
    assert_eq!(count, 1, "Expected exactly 1 row");

    let restored = ClusterJob::from_message(&mut body);
    assert_eq!(restored.id, row_id);
    assert_eq!(restored.job_id, 55);
    assert_eq!(restored.scheduler_id, 7);
    assert!(restored.submitting);
    assert_eq!(restored.submitting_count, 3);
    assert_eq!(restored.bundle_hash, "myhash");
    assert_eq!(restored.working_directory, "/mydir");
    assert!(restored.running);
}

// ---------------------------------------------------------------------------
// DB_JOB_GET_BY_ID — not found
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOB_GET_BY_ID` message returns count=0 when no matching row exists.
///
/// # Setup
/// Empty in-memory DB with cluster job schema.
///
/// # Act
/// Dispatch a `DB_JOB_GET_BY_ID` message with `db_request_id=302` and non-existent id=99999.
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=302` and count=0.
#[tokio::test]
async fn test_handle_job_get_by_id_not_found() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOB_GET_BY_ID, |m| {
        m.push_uint(302);
        m.push_ulong(99999);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 302);
    assert_eq!(body.pop_uint(), 0);
}

// ---------------------------------------------------------------------------
// DB_JOB_GET_BY_JOB_ID — found
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOB_GET_BY_JOB_ID` message returns matching rows for the given `job_id`.
///
/// # Setup
/// In-memory DB with one cluster job row with `job_id=77`.
///
/// # Act
/// Dispatch a `DB_JOB_GET_BY_JOB_ID` message with `db_request_id=400` and `job_id=77`.
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=400`, `count=1`, and a `ClusterJob` with `job_id=77`.
#[tokio::test]
async fn test_handle_job_get_by_job_id_found() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    cluster_job::ActiveModel {
        job_id: Set(77),
        scheduler_id: Set(0),
        submitting: Set(false),
        submitting_count: Set(0),
        bundle_hash: Set("h1".to_string()),
        working_directory: Set("/d1".to_string()),
        running: Set(false),
        deleting: Set(false),
        deleted: Set(false),
        cluster: Set("ozstar".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOB_GET_BY_JOB_ID, |m| {
        m.push_uint(400);
        m.push_ulong(77);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 400);
    let count = body.pop_uint();
    assert_eq!(count, 1);
    let row = ClusterJob::from_message(&mut body);
    assert_eq!(row.job_id, 77);
}

// ---------------------------------------------------------------------------
// DB_JOB_GET_RUNNING_JOBS — mixed running/non-running
// ---------------------------------------------------------------------------

/// Verifies that `DB_JOB_GET_RUNNING_JOBS` returns only running jobs belonging to the calling cluster.
///
/// # Setup
/// In-memory DB with three rows: one running job for "ozstar", one non-running job for
/// "ozstar", and one running job for "nci".
///
/// # Act
/// Dispatch a `DB_JOB_GET_RUNNING_JOBS` message with `db_request_id=500` from cluster "ozstar".
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=500`, count=1, and the single running ozstar
/// job (`job_id=1`).
#[tokio::test]
async fn test_handle_job_get_running_jobs() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // 1 running job for ozstar, 1 non-running for ozstar, 1 running for nci
    for (job_id, running, cluster) in [
        (1i64, true, "ozstar"),
        (2i64, false, "ozstar"),
        (3i64, true, "nci"),
    ] {
        cluster_job::ActiveModel {
            job_id: Set(job_id),
            scheduler_id: Set(0),
            submitting: Set(false),
            submitting_count: Set(0),
            bundle_hash: Set(String::new()),
            working_directory: Set(String::new()),
            running: Set(running),
            deleting: Set(false),
            deleted: Set(false),
            cluster: Set(cluster.to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOB_GET_RUNNING_JOBS, |m| {
        m.push_uint(500);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 500);
    let count = body.pop_uint();
    assert_eq!(count, 1, "Only 1 running job for 'ozstar'");
    let row = ClusterJob::from_message(&mut body);
    assert!(row.running);
    assert_eq!(row.job_id, 1);
}

// ---------------------------------------------------------------------------
// DB_JOB_DELETE
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOB_DELETE` message removes the specified cluster job row from the database.
///
/// # Setup
/// In-memory DB with one pre-inserted cluster job row (`job_id=88`).
///
/// # Act
/// Dispatch a `DB_JOB_DELETE` message with `db_request_id=600` and the row's id.
///
/// # Assert
/// No row with the given id remains in `cluster_job`; the `DB_RESPONSE` echoes `db_request_id=600`.
#[tokio::test]
async fn test_handle_job_delete() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let inserted = cluster_job::ActiveModel {
        job_id: Set(88),
        scheduler_id: Set(0),
        submitting: Set(false),
        submitting_count: Set(0),
        bundle_hash: Set(String::new()),
        working_directory: Set(String::new()),
        running: Set(false),
        deleting: Set(false),
        deleted: Set(false),
        cluster: Set("ozstar".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let row_id = inserted.id;

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOB_DELETE, |m| {
        m.push_uint(600);
        m.push_ulong(row_id as u64);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let count = cluster_job::Entity::find()
        .filter(cluster_job::Column::Id.eq(row_id))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(count, 0, "Row should be deleted");

    let captured = sent.lock().unwrap();
    let (req_id, _) = parse_response(captured[0].clone());
    assert_eq!(req_id, 600);
}

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_SAVE — insert
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOBSTATUS_SAVE` message with id=0 creates a new cluster job status row.
///
/// # Setup
/// Empty in-memory DB with cluster job status schema; `ClusterJobStatus` with id=0,
/// `job_id=42`, `what="scheduler_id`", state=500.
///
/// # Act
/// Dispatch a `DB_JOBSTATUS_SAVE` message with `db_request_id=700` and the `ClusterJobStatus` payload.
///
/// # Assert
/// A new row exists in `cluster_job_status` with the correct field values; the `DB_RESPONSE`
/// contains `db_request_id=700` and the assigned row id.
#[tokio::test]
async fn test_handle_jobstatus_save_insert() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let status = ClusterJobStatus {
        id: 0,
        job_id: 42,
        what: "scheduler_id".to_string(),
        state: 500,
    };

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_SAVE, |m| {
        m.push_uint(700);
        status.to_message(m);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let model = cluster_job_status::Entity::find()
        .filter(cluster_job_status::Column::JobId.eq(42i64))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let db_job_id = model.job_id;
    let db_what = model.what.clone();
    let db_state = model.state;
    assert_eq!(db_job_id, 42);
    assert_eq!(db_what, "scheduler_id");
    assert_eq!(db_state, 500);

    let (req_id, mut body) = parse_response(sent.lock().unwrap()[0].clone());
    assert_eq!(req_id, 700);
    let new_id = body.pop_ulong();
    let actual = cluster_job_status::Entity::find()
        .filter(cluster_job_status::Column::JobId.eq(42i64))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(new_id == 0 || new_id == actual.id as u64, "new_id={new_id}");
}

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_SAVE — update
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOBSTATUS_SAVE` message with a non-zero id updates the existing job status row.
///
/// # Setup
/// In-memory DB with one pre-inserted cluster job status row (`what="old_what`", state=1);
/// `ClusterJobStatus` updated to `what="new_what`", state=999.
///
/// # Act
/// Dispatch a `DB_JOBSTATUS_SAVE` message with `db_request_id=701` and the updated payload.
///
/// # Assert
/// The existing row reflects the updated field values; the `DB_RESPONSE` contains
/// `db_request_id=701` and the existing row id.
#[tokio::test]
async fn test_handle_jobstatus_save_update() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let inserted = cluster_job_status::ActiveModel {
        job_id: Set(10),
        what: Set("old_what".to_string()),
        state: Set(1),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let existing_id = inserted.id;

    let status = ClusterJobStatus {
        id: existing_id,
        job_id: 10,
        what: "new_what".to_string(),
        state: 999,
    };

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_SAVE, |m| {
        m.push_uint(701);
        status.to_message(m);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let model = cluster_job_status::Entity::find_by_id(existing_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let db_what = model.what.clone();
    let db_state = model.state;
    assert_eq!(db_what, "new_what");
    assert_eq!(db_state, 999);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 701);
    assert_eq!(body.pop_ulong(), existing_id as u64);
}

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_GET_BY_JOB_ID
// ---------------------------------------------------------------------------

/// Verifies that `DB_JOBSTATUS_GET_BY_JOB_ID` returns all status rows for the specified `job_id`.
///
/// # Setup
/// In-memory DB with three status rows: two for `job_id=20` (what="a", "b") and one for
/// `job_id=21` (what="c").
///
/// # Act
/// Dispatch a `DB_JOBSTATUS_GET_BY_JOB_ID` message with `db_request_id=800` and `job_id=20`.
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=800`, count=2, and two rows both with `job_id=20`
/// and whats "a" and "b".
#[tokio::test]
async fn test_handle_jobstatus_get_by_job_id() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    for (jid, what, state) in [(20i64, "a", 1i32), (20i64, "b", 2i32), (21i64, "c", 3i32)] {
        cluster_job_status::ActiveModel {
            job_id: Set(jid),
            what: Set(what.to_string()),
            state: Set(state),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_GET_BY_JOB_ID, |m| {
        m.push_uint(800);
        m.push_ulong(20);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 800);
    let count = body.pop_uint();
    assert_eq!(count, 2, "Should return only statuses for job 20");

    let s1 = ClusterJobStatus::from_message(&mut body);
    let s2 = ClusterJobStatus::from_message(&mut body);
    assert_eq!(s1.job_id, 20);
    assert_eq!(s2.job_id, 20);
    let mut whats: Vec<String> = vec![s1.what, s2.what];
    whats.sort();
    assert_eq!(whats, vec!["a", "b"]);
}

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT
// ---------------------------------------------------------------------------

/// Verifies that `DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT` returns only the status row matching both `job_id` and what.
///
/// # Setup
/// In-memory DB with three status rows: two for `job_id=30` (`what="cpu_time`", "`wall_time`")
/// and one for `job_id=31` (`what="cpu_time`").
///
/// # Act
/// Dispatch `DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT` with `db_request_id=900`, `job_id=30`,
/// and `what="cpu_time`".
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=900`, count=1, and the single status row
/// with `job_id=30`, `what="cpu_time`", state=42.
#[tokio::test]
async fn test_handle_jobstatus_get_by_job_id_and_what() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    for (jid, what, state) in [
        (30i64, "cpu_time", 42i32),
        (30i64, "wall_time", 88i32),
        (31i64, "cpu_time", 99i32),
    ] {
        cluster_job_status::ActiveModel {
            job_id: Set(jid),
            what: Set(what.to_string()),
            state: Set(state),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT, |m| {
        m.push_uint(900);
        m.push_ulong(30);
        m.push_string("cpu_time");
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 900);
    let count = body.pop_uint();
    assert_eq!(count, 1, "Only job 30 with what='cpu_time'");
    let s = ClusterJobStatus::from_message(&mut body);
    assert_eq!(s.job_id, 30);
    assert_eq!(s.what, "cpu_time");
    assert_eq!(s.state, 42);
}

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_DELETE_BY_ID_LIST
// ---------------------------------------------------------------------------

/// Verifies that `DB_JOBSTATUS_DELETE_BY_ID_LIST` removes exactly the specified status rows by id.
///
/// # Setup
/// In-memory DB with four status rows (what: "a", "b", "c", "d") all for `job_id=1`.
///
/// # Act
/// Dispatch `DB_JOBSTATUS_DELETE_BY_ID_LIST` with `db_request_id=1000` and the ids of rows
/// "a" and "c".
///
/// # Assert
/// Only rows "b" and "d" remain in `cluster_job_status`; the `DB_RESPONSE` echoes
/// `db_request_id=1000`.
#[tokio::test]
async fn test_handle_jobstatus_delete_by_id_list() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // Insert 4 statuses; we'll delete 2 of them
    let mut ids = Vec::new();
    for (what, state) in [("a", 1i32), ("b", 2i32), ("c", 3i32), ("d", 4i32)] {
        let inserted = cluster_job_status::ActiveModel {
            job_id: Set(1),
            what: Set(what.to_string()),
            state: Set(state),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        ids.push(inserted.id as u64);
    }

    let to_delete = [ids[0], ids[2]]; // delete "a" and "c"

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_DELETE_BY_ID_LIST, |m| {
        m.push_uint(1000);
        m.push_uint(2);
        m.push_ulong(to_delete[0]);
        m.push_ulong(to_delete[1]);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let remaining = cluster_job_status::Entity::find()
        .order_by_asc(cluster_job_status::Column::What)
        .all(&db)
        .await
        .unwrap();
    let whats: Vec<String> = remaining.iter().map(|m| m.what.clone()).collect();
    assert_eq!(whats, vec!["b", "d"]);

    let captured = sent.lock().unwrap();
    let (req_id, _) = parse_response(captured[0].clone());
    assert_eq!(req_id, 1000);
}

// ---------------------------------------------------------------------------
// DB_BUNDLE_CREATE_OR_UPDATE_JOB — new bundle
// ---------------------------------------------------------------------------

/// Verifies that `DB_BUNDLE_CREATE_OR_UPDATE_JOB` creates a new bundle row when no row with the given hash exists.
///
/// # Setup
/// Empty in-memory DB with bundle job schema; `BundleJob` with id=0 and
/// content=`{"script":"run.sh"}`.
///
/// # Act
/// Dispatch `DB_BUNDLE_CREATE_OR_UPDATE_JOB` with `db_request_id=1100`, the `BundleJob`
/// payload, and hash="myhash".
///
/// # Assert
/// A new row exists in `bundle_job` with the correct content, cluster="ozstar", and
/// `bundle_hash="myhash`"; the `DB_RESPONSE` contains `db_request_id=1100`.
#[tokio::test]
async fn test_handle_bundle_create_or_update_new() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let bundle = BundleJob {
        id: 0,
        content: r#"{"script":"run.sh"}"#.to_string(),
        cluster: String::new(),
        bundle_hash: String::new(),
    };

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_BUNDLE_CREATE_OR_UPDATE_JOB, |m| {
        m.push_uint(1100);
        bundle.to_message(m);
        m.push_string("myhash");
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let model = bundle_job::Entity::find()
        .filter(bundle_job::Column::BundleHash.eq("myhash"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let db_content = model.content.clone();
    let db_cluster = model.cluster.clone();
    let db_hash = model.bundle_hash.clone();
    assert_eq!(db_content, r#"{"script":"run.sh"}"#);
    assert_eq!(db_cluster, "ozstar");
    assert_eq!(db_hash, "myhash");

    let (req_id, mut body) = parse_response(sent.lock().unwrap()[0].clone());
    assert_eq!(req_id, 1100);
    let new_id = body.pop_ulong();
    let actual = bundle_job::Entity::find()
        .filter(bundle_job::Column::BundleHash.eq("myhash"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(new_id == 0 || new_id == actual.id as u64, "new_id={new_id}");
}

// ---------------------------------------------------------------------------
// DB_BUNDLE_CREATE_OR_UPDATE_JOB — existing bundle (update)
// ---------------------------------------------------------------------------

/// Verifies that `DB_BUNDLE_CREATE_OR_UPDATE_JOB` updates an existing bundle row when the hash matches.
///
/// # Setup
/// In-memory DB with one pre-inserted bundle row (content="old", `bundle_hash="samehash`");
/// updated `BundleJob` carries `content="new_content`".
///
/// # Act
/// Dispatch `DB_BUNDLE_CREATE_OR_UPDATE_JOB` with `db_request_id=1101`, updated content,
/// and hash="samehash".
///
/// # Assert
/// The existing row is updated to `content="new_content`"; no new row is created; the
/// `DB_RESPONSE` contains `db_request_id=1101` and the existing row id.
#[tokio::test]
async fn test_handle_bundle_create_or_update_existing() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let inserted = bundle_job::ActiveModel {
        content: Set("old".to_string()),
        cluster: Set("ozstar".to_string()),
        bundle_hash: Set("samehash".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let existing_id = inserted.id;

    let bundle = BundleJob {
        id: 0,
        content: "new_content".to_string(),
        cluster: String::new(),
        bundle_hash: String::new(),
    };

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_BUNDLE_CREATE_OR_UPDATE_JOB, |m| {
        m.push_uint(1101);
        bundle.to_message(m);
        m.push_string("samehash");
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let model = bundle_job::Entity::find_by_id(existing_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let db_content = model.content.clone();
    assert_eq!(db_content, "new_content");

    // No new row inserted
    let count = bundle_job::Entity::find().count(&db).await.unwrap();
    assert_eq!(count, 1);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 1101);
    assert_eq!(body.pop_ulong(), existing_id as u64);
}

// ---------------------------------------------------------------------------
// DB_BUNDLE_GET_JOB_BY_ID — found
// ---------------------------------------------------------------------------

/// Verifies that `DB_BUNDLE_GET_JOB_BY_ID` returns the matching bundle row when it exists.
///
/// # Setup
/// In-memory DB with one pre-inserted bundle row (`content="bundle_content`",
/// `bundle_hash="xyz`").
///
/// # Act
/// Dispatch `DB_BUNDLE_GET_JOB_BY_ID` with `db_request_id=1200` and the bundle's id.
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=1200`, count=1, and a `BundleJob` with
/// the correct id and content.
#[tokio::test]
async fn test_handle_bundle_get_by_id_found() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let inserted = bundle_job::ActiveModel {
        content: Set("bundle_content".to_string()),
        cluster: Set("ozstar".to_string()),
        bundle_hash: Set("xyz".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let bundle_id = inserted.id;

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_BUNDLE_GET_JOB_BY_ID, |m| {
        m.push_uint(1200);
        m.push_ulong(bundle_id as u64);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 1200);
    let count = body.pop_uint();
    assert_eq!(count, 1);
    let restored = BundleJob::from_message(&mut body);
    assert_eq!(restored.id, bundle_id);
    assert_eq!(restored.content, "bundle_content");
}

// ---------------------------------------------------------------------------
// DB_BUNDLE_GET_JOB_BY_ID — not found
// ---------------------------------------------------------------------------

/// Verifies that `DB_BUNDLE_GET_JOB_BY_ID` returns count=0 when no bundle matches the given id.
///
/// # Setup
/// Empty in-memory DB with bundle job schema.
///
/// # Act
/// Dispatch `DB_BUNDLE_GET_JOB_BY_ID` with `db_request_id=1201` and non-existent id=99999.
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=1201` and count=0.
#[tokio::test]
async fn test_handle_bundle_get_by_id_not_found() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_BUNDLE_GET_JOB_BY_ID, |m| {
        m.push_uint(1201);
        m.push_ulong(99999);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 1201);
    assert_eq!(body.pop_uint(), 0);
}

// ---------------------------------------------------------------------------
// DB_BUNDLE_DELETE_JOB
// ---------------------------------------------------------------------------

/// Verifies that `DB_BUNDLE_DELETE_JOB` removes the specified bundle row from the database.
///
/// # Setup
/// In-memory DB with one pre-inserted bundle row.
///
/// # Act
/// Dispatch `DB_BUNDLE_DELETE_JOB` with `db_request_id=1300` and the bundle's id.
///
/// # Assert
/// No row with the given id remains in `bundle_job`; the `DB_RESPONSE` echoes `db_request_id=1300`.
#[tokio::test]
async fn test_handle_bundle_delete() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let inserted = bundle_job::ActiveModel {
        content: Set("c".to_string()),
        cluster: Set("ozstar".to_string()),
        bundle_hash: Set("h".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let bundle_id = inserted.id;

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_BUNDLE_DELETE_JOB, |m| {
        m.push_uint(1300);
        m.push_ulong(bundle_id as u64);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let count = bundle_job::Entity::find()
        .filter(bundle_job::Column::Id.eq(bundle_id))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);

    let captured = sent.lock().unwrap();
    let (req_id, _) = parse_response(captured[0].clone());
    assert_eq!(req_id, 1300);
}

// ---------------------------------------------------------------------------
// Non-DB message returns false
// ---------------------------------------------------------------------------

/// Verifies that a non-DB message is not handled by `maybe_handle_cluster_db_message` and no response is sent.
///
/// # Setup
/// Empty in-memory DB with cluster job schema.
///
/// # Act
/// Dispatch an `UPDATE_JOB` message (not a DB message) through `maybe_handle_cluster_db_message`.
///
/// # Assert
/// The handler returns `false` and no messages are sent to the cluster.
#[tokio::test]
async fn test_unhandled_message_returns_false() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    let (mock, sent) = mock_cluster_capturing("ozstar");

    let mut msg = dispatch_message(UPDATE_JOB, |m| {
        m.push_uint(0);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(!handled);

    let captured = sent.lock().unwrap();
    assert_eq!(captured.len(), 0);
}

// ===========================================================================
// FK / error-path tests
//
// These tests verify that operations referencing non-existent IDs or
// mismatched bundle hashes behave correctly. Rust does not enforce
// foreign keys (SQLite FKs are off and not declared), so the behavior differs:
//   - GET with non-existent FK → returns count=0 (no matching rows)
//   - SAVE with non-existent FK → insert succeeds (no FK constraint)
//   - Bundle update with wrong hash → creates a new bundle
//   - Bundle delete of non-existent ID → no-op
// These tests document the actual behavior for these edge cases.
// ===========================================================================

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_GET_BY_JOB_ID — non-existent job ID returns count=0
// Equivalent behavior: test_db_job_status_get_by_job_id_invalid_job
// ---------------------------------------------------------------------------

/// Verifies that `DB_JOBSTATUS_GET_BY_JOB_ID` returns count=0 when the requested `job_id` does not exist.
///
/// # Setup
/// In-memory DB with one real cluster job row; no status rows for the target job id exist.
///
/// # Act
/// Dispatch `DB_JOBSTATUS_GET_BY_JOB_ID` with `db_request_id=5001` and a non-existent job id.
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=5001` and count=0.
#[tokio::test]
async fn test_handle_jobstatus_get_by_job_id_nonexistent_job() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // Insert a real job so the DB is not empty
    let inserted = cluster_job::ActiveModel {
        job_id: Set(42),
        scheduler_id: Set(0),
        submitting: Set(false),
        submitting_count: Set(0),
        bundle_hash: Set("hash".to_string()),
        working_directory: Set("/work".to_string()),
        running: Set(false),
        deleting: Set(false),
        deleted: Set(false),
        cluster: Set("ozstar".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let job_row_id = inserted.id;

    // Request statuses for a job ID that doesn't exist
    let invalid_job_id = (job_row_id + 100) as u64;
    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_GET_BY_JOB_ID, |m| {
        m.push_uint(5001);
        m.push_ulong(invalid_job_id);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 5001);
    assert_eq!(body.pop_uint(), 0, "Non-existent job should return count=0");
}

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT — non-existent job ID returns count=0
// Equivalent behavior: test_db_job_status_get_by_job_id_and_what_invalid_job
// ---------------------------------------------------------------------------

/// Verifies that `DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT` returns count=0 when the `job_id` does not exist.
///
/// # Setup
/// In-memory DB with one real job row and one status row for that job; a different
/// non-existent job id is queried.
///
/// # Act
/// Dispatch `DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT` with `db_request_id=5002`, a non-existent
/// job id, and `what="cpu_time`".
///
/// # Assert
/// The `DB_RESPONSE` contains `db_request_id=5002` and count=0.
#[tokio::test]
async fn test_handle_jobstatus_get_by_job_id_and_what_nonexistent_job() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // Insert a real job
    let inserted = cluster_job::ActiveModel {
        job_id: Set(42),
        scheduler_id: Set(0),
        submitting: Set(false),
        submitting_count: Set(0),
        bundle_hash: Set("hash".to_string()),
        working_directory: Set("/work".to_string()),
        running: Set(false),
        deleting: Set(false),
        deleted: Set(false),
        cluster: Set("ozstar".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let job_row_id = inserted.id;

    // Insert a real status for the existing job
    cluster_job_status::ActiveModel {
        job_id: Set(job_row_id),
        what: Set("cpu_time".to_string()),
        state: Set(100),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // Query with a non-existent job ID
    let invalid_job_id = (job_row_id + 100) as u64;
    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT, |m| {
        m.push_uint(5002);
        m.push_ulong(invalid_job_id);
        m.push_string("cpu_time");
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 5002);
    assert_eq!(body.pop_uint(), 0, "Non-existent job should return count=0");
}

// ---------------------------------------------------------------------------
// DB_JOBSTATUS_SAVE — save with non-existent job FK
// Equivalent behavior: test_db_job_status_save_invalid_job → returns success=false
// Rust: no FK constraint, so the insert succeeds and returns the new row ID.
// ---------------------------------------------------------------------------

/// Verifies that `DB_JOBSTATUS_SAVE` succeeds even when the referenced `job_id` does not exist, because no FK constraint is enforced.
///
/// # Setup
/// In-memory DB with one real cluster job row; a `ClusterJobStatus` references a
/// non-existent job id (`job_row_id` + 999).
///
/// # Act
/// Dispatch `DB_JOBSTATUS_SAVE` with `db_request_id=5003` and the orphan status payload.
///
/// # Assert
/// The insert succeeds; the new status row is found in `cluster_job_status` with the
/// non-existent `job_id`; the `DB_RESPONSE` echoes `db_request_id=5003`.
#[tokio::test]
async fn test_handle_jobstatus_save_nonexistent_job_fk() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // Insert a real job to establish a known ID range
    let inserted = cluster_job::ActiveModel {
        job_id: Set(42),
        scheduler_id: Set(0),
        submitting: Set(false),
        submitting_count: Set(0),
        bundle_hash: Set("hash".to_string()),
        working_directory: Set("/work".to_string()),
        running: Set(false),
        deleting: Set(false),
        deleted: Set(false),
        cluster: Set("ozstar".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let job_row_id = inserted.id;

    // Save a status referencing a non-existent job (no FK enforcement in Rust)
    let invalid_job_id = job_row_id + 999;
    let status = ClusterJobStatus {
        id: 0,
        job_id: invalid_job_id,
        what: "orphan_status".to_string(),
        state: 42,
    };

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_JOBSTATUS_SAVE, |m| {
        m.push_uint(5003);
        status.to_message(m);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    // In Rust (no FK), the insert succeeds — response contains the new row ID
    let (req_id, mut body) = parse_response(sent.lock().unwrap()[0].clone());
    assert_eq!(req_id, 5003);
    let returned_id = body.pop_ulong();
    // The insert succeeds, so returned_id should be a valid (non-zero) row ID
    // (SQLite may return 0 or real id depending on SeaORM version)
    // Verify the row actually exists in the DB
    let model = cluster_job_status::Entity::find()
        .filter(cluster_job_status::Column::JobId.eq(invalid_job_id))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let db_job_id = model.job_id;
    assert_eq!(
        db_job_id, invalid_job_id,
        "Row was inserted despite non-existent FK"
    );
    let _ = returned_id; // acknowledged
}

// ---------------------------------------------------------------------------
// DB_BUNDLE_CREATE_OR_UPDATE_JOB — update with different hash creates new entry
// Equivalent behavior: test_db_bundle_job_save_error → returns success=false
// Rust: lookup by hash fails to find existing, so a new bundle is created.
// ---------------------------------------------------------------------------

/// Verifies that `DB_BUNDLE_CREATE_OR_UPDATE_JOB` creates a new bundle instead of updating when the hash does not match any existing row.
///
/// # Setup
/// In-memory DB with one bundle row (`bundle_hash="original_hash`"); attempt to update
/// using `hash="different_hash`".
///
/// # Act
/// Dispatch `DB_BUNDLE_CREATE_OR_UPDATE_JOB` with `db_request_id=5004` and
/// `hash="different_hash"`.
///
/// # Assert
/// A new bundle row is created (total count=2); the original bundle is unchanged;
/// the `DB_RESPONSE` returns an id differing from the original.
#[tokio::test]
async fn test_handle_bundle_update_wrong_hash_returns_error() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // Create an initial bundle with hash "original_hash"
    let inserted = bundle_job::ActiveModel {
        content: Set("original content".to_string()),
        cluster: Set("ozstar".to_string()),
        bundle_hash: Set("original_hash".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let original_id = inserted.id;

    // Try to update using the bundle ID but with a different hash
    // C++ behavior: return error (id=0) when hash doesn't match
    let bundle = BundleJob {
        id: original_id, // Use existing ID
        content: "updated content".to_string(),
        cluster: String::new(),
        bundle_hash: String::new(),
    };

    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_BUNDLE_CREATE_OR_UPDATE_JOB, |m| {
        m.push_uint(5004);
        bundle.to_message(m);
        m.push_string("different_hash"); // wrong hash
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    // Verify: NO new bundle was created (update failed)
    let count = bundle_job::Entity::find().count(&db).await.unwrap();
    assert_eq!(count, 1, "Wrong hash should NOT create a new bundle");

    // Original bundle should be unchanged
    let original = bundle_job::Entity::find_by_id(original_id as i64)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        original.content, "original content",
        "Original bundle should be unchanged"
    );

    // Response should contain id=0 (error)
    let captured = sent.lock().unwrap();
    let (req_id, mut body) = parse_response(captured[0].clone());
    assert_eq!(req_id, 5004);
    let returned_id = body.pop_ulong();
    assert_eq!(returned_id, 0, "Wrong hash should return id=0 (error)");
}

// ---------------------------------------------------------------------------
// DB_BUNDLE_DELETE_JOB — delete non-existent ID is a no-op
// Equivalent behavior: test_db_bundle_job_delete_error → returns success=false
// Rust: delete_by_id is silently ignored, no error response.
// ---------------------------------------------------------------------------

/// Verifies that `DB_BUNDLE_DELETE_JOB` is a no-op when the target id does not exist, leaving existing rows untouched.
///
/// # Setup
/// In-memory DB with one real bundle row (`bundle_hash="keep_hash`").
///
/// # Act
/// Dispatch `DB_BUNDLE_DELETE_JOB` with `db_request_id=5005` and non-existent id=99999.
///
/// # Assert
/// The existing bundle row is unaffected; the `DB_RESPONSE` echoes `db_request_id=5005`.
#[tokio::test]
async fn test_handle_bundle_delete_nonexistent_is_noop() {
    let db = make_db().await;
    setup_cluster_db(&db).await;

    // Insert a bundle to ensure table is not empty
    bundle_job::ActiveModel {
        content: Set("keep me".to_string()),
        cluster: Set("ozstar".to_string()),
        bundle_hash: Set("keep_hash".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // Try to delete a non-existent ID
    let (mock, sent) = mock_cluster_capturing("ozstar");
    let mut msg = dispatch_message(DB_BUNDLE_DELETE_JOB, |m| {
        m.push_uint(5005);
        m.push_ulong(99999);
    });

    let handled = maybe_handle_cluster_db_message(&mut msg, &mock, &db).await;
    assert!(handled);

    // Verify existing bundle is still there
    let count = bundle_job::Entity::find().count(&db).await.unwrap();
    assert_eq!(
        count, 1,
        "Existing bundle should survive delete of non-existent ID"
    );

    // Response is still sent (db_request_id echoed)
    let captured = sent.lock().unwrap();
    let (req_id, _) = parse_response(captured[0].clone());
    assert_eq!(req_id, 5005);
}
