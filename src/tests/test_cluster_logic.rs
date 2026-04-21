//! Integration tests for cluster business logic:
//! `handle_update_job`, `check_unsubmitted_jobs`, `check_cancelling_jobs`,
//! `check_deleting_jobs`.
//!
//! Each test spins up a real `Cluster` with a SQLite-backed `AppContext`
//! and verifies both DB side-effects and outgoing WS messages.

mod common;

use std::sync::Arc;
use std::time::Duration;

use adacs_job_controller::cluster::cluster::{AppContext, Cluster};
use adacs_job_controller::cluster::traits::WsOutbound;
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::db::entities::{job, job_history};
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::Priority;
use dashmap::DashMap;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ColumnTrait, Database, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn make_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection failed");
    adacs_job_controller::db::schema::create_test_schema(&db).await;
    db
}

/// Old-style timestamp (far in the past) for triggering re-submission logic.
fn old_timestamp() -> sea_orm::prelude::DateTime {
    chrono::NaiveDateTime::parse_from_str("2000-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
}

/// Recent timestamp for suppressing re-submission logic.
fn now_timestamp() -> sea_orm::prelude::DateTime {
    chrono::Utc::now().naive_utc()
}

async fn insert_job(
    db: &DatabaseConnection,
    id: i64,
    cluster: &str,
    bundle: &str,
    application: &str,
    parameters: &str,
) {
    job::ActiveModel {
        id: Set(id),
        user: Set(1),
        parameters: Set(parameters.to_string()),
        cluster: Set(cluster.to_string()),
        bundle: Set(bundle.to_string()),
        application: Set(application.to_string()),
    }
    .insert(db)
    .await
    .unwrap();
}

async fn insert_history(
    db: &DatabaseConnection,
    job_id: i64,
    timestamp: sea_orm::prelude::DateTime,
    what: &str,
    state: i32,
) {
    job_history::ActiveModel {
        job_id: Set(job_id),
        timestamp: Set(timestamp),
        what: Set(what.to_string()),
        state: Set(state),
        details: Set(String::new()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

fn test_cluster_config(name: &str) -> ClusterConfig {
    ClusterConfig {
        name: name.to_string(),
        host: "localhost".to_string(),
        username: "testuser".to_string(),
        path: "/home/testuser/jobcontroller".to_string(),
        key: String::new(),
        connection_type: "manual".to_string(),
        keytab: String::new(),
        kerberos_principal: String::new(),
        ltk: None,
    }
}

fn make_app_context(db: DatabaseConnection) -> Arc<AppContext> {
    Arc::new(AppContext {
        db,
        file_list_map: Arc::new(DashMap::new()),
    })
}

/// Make an UPDATE_JOB message with the given fields.
fn make_update_job_message(job_id: u32, what: &str, status: u32, details: &str) -> Message {
    let mut msg = Message::new(UPDATE_JOB, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(job_id);
    msg.push_string(what);
    msg.push_uint(status);
    msg.push_string(details);
    // Round-trip so id() and source() are properly set
    Message::from_bytes(msg.into_data())
}

// ---------------------------------------------------------------------------
// handle_update_job: verify JobserverJobhistory row is inserted
// ---------------------------------------------------------------------------

/// Verifies that `handle_message` with an `UPDATE_JOB` message inserts a row into `JobserverJobhistory`.
///
/// # Setup
/// An in-memory SQLite DB with the `JobserverJob` and `JobserverJobhistory` tables is created.
/// A `Cluster` is initialized with an `AppContext` wrapping that DB.
/// An `UPDATE_JOB` message for `job_id=42` with `what="job_submission"` and `state=10` is constructed.
///
/// # Act
/// `cluster.handle_message(msg).await` is called.
///
/// # Assert
/// A row exists in `JobserverJobhistory` with `jobId=42`, `what="job_submission"`, `state=10`,
/// and `details="submitted to scheduler"`.
#[tokio::test]
async fn test_handle_update_job_inserts_history() {
    let db = make_db().await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let msg = make_update_job_message(42, "job_submission", 10, "submitted to scheduler");

    // handle_message dispatches to handle_update_job for UPDATE_JOB
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.handle_message(msg).await;

    // Verify row was inserted
    let row = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(42i64))
        .one(&db)
        .await
        .unwrap()
        .expect("expected a row");

    assert_eq!(row.job_id, 42);
    assert_eq!(row.what, "job_submission");
    assert_eq!(row.state, 10);
    assert_eq!(row.details, "submitted to scheduler");
}

/// Verifies that multiple `UPDATE_JOB` messages for the same job each insert a separate history row.
///
/// # Setup
/// An in-memory SQLite DB with the required tables is created. Three `UPDATE_JOB` messages
/// are built for `job_id=7` with states 10 (queued), 40 (running), and 500 (complete).
///
/// # Act
/// All three messages are dispatched via `cluster.handle_message`.
///
/// # Assert
/// `JobserverJobhistory` contains exactly 3 rows for `jobId=7`.
#[tokio::test]
async fn test_handle_update_job_multiple_updates() {
    let db = make_db().await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    use adacs_job_controller::cluster::traits::ClusterTrait;

    for (state, what) in [(10u32, "queued"), (40, "running"), (500, "complete")] {
        let msg = make_update_job_message(7, what, state, "details");
        cluster.handle_message(msg).await;
    }

    let count = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(7i64))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(count, 3, "Should have 3 history rows");
}

/// Verifies that `handle_message` with an `UPDATE_JOB` message returns early without panicking
/// when no `AppContext` is provided.
///
/// # Setup
/// A `Cluster` is created without an `AppContext` (i.e., `None`).
/// An `UPDATE_JOB` message is constructed for `job_id=1`.
///
/// # Act
/// `cluster.handle_message(msg).await` is called.
///
/// # Assert
/// The call completes without panicking.
#[tokio::test]
async fn test_handle_update_job_no_app_context_does_not_panic() {
    // Without app_context, the handler just returns early — no crash.
    let cluster = Cluster::new(test_cluster_config("ozstar"), None);
    let msg = make_update_job_message(1, "test", 10, "no ctx");

    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.handle_message(msg).await; // must not panic
}

// ---------------------------------------------------------------------------
// check_unsubmitted_jobs: old PENDING state → SUBMIT_JOB resent
// ---------------------------------------------------------------------------

/// Verifies that `check_unsubmitted_jobs` re-sends a `SUBMIT_JOB` message for a job
/// stuck in PENDING state (state=10) with an old timestamp.
///
/// # Setup
/// A job in `JobserverJob` and a history row with `state=10` and timestamp `2000-01-01` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// At least one `SUBMIT_JOB` message is present in the drained output.
#[tokio::test]
async fn test_check_unsubmitted_jobs_resends_old_pending() {
    let db = make_db().await;

    // Insert a job on "ozstar"
    insert_job(&db, 1, "ozstar", "mybundle", "myapp", "{}").await;

    // Insert history with state=10 (PENDING) and timestamp far in the past
    insert_history(&db, 1, old_timestamp(), "submit", 10).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    // Set cluster online by giving it a real WS sender
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));

    // Start the scheduler so queued messages are forwarded to the channel
    cluster.start_tasks();

    // Call check_unsubmitted_jobs
    cluster.check_unsubmitted_jobs().await;

    // Wait briefly for the scheduler to forward the message
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Drain channel
    let mut messages = Vec::new();
    while let Ok(outbound) = rx.try_recv() {
        let WsOutbound::Binary(data) = outbound else {
            continue;
        };
        messages.push(data);
    }

    // Expect at least one SUBMIT_JOB message + the SERVER_READY that gets sent on connection
    let submit_msgs: Vec<_> = messages
        .iter()
        .filter_map(|data| {
            let msg = Message::from_bytes(data.clone());
            if msg.id() == SUBMIT_JOB {
                Some(msg)
            } else {
                None
            }
        })
        .collect();

    assert!(
        !submit_msgs.is_empty(),
        "Expected at least one SUBMIT_JOB message"
    );
    cluster.stop();
}

/// Verifies that `check_unsubmitted_jobs` does NOT re-send `SUBMIT_JOB` for a job
/// in PENDING state (state=10) with a recent timestamp.
///
/// # Setup
/// A job and a history row with `state=10` and `timestamp=NOW` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// No `SUBMIT_JOB` messages appear in the output.
#[tokio::test]
async fn test_check_unsubmitted_jobs_ignores_recent_state() {
    let db = make_db().await;

    insert_job(&db, 2, "ozstar", "mybundle", "myapp", "{}").await;

    // Insert history with state=10 and timestamp = NOW (within the ignore window)
    insert_history(&db, 2, now_timestamp(), "submit", 10).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    cluster.check_unsubmitted_jobs().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut messages = Vec::new();
    while let Ok(outbound) = rx.try_recv() {
        let WsOutbound::Binary(data) = outbound else {
            continue;
        };
        messages.push(data);
    }

    let submit_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes(data.to_vec()).id() == SUBMIT_JOB)
        .collect();

    assert!(
        submit_msgs.is_empty(),
        "Recent job should NOT trigger SUBMIT_JOB"
    );
    cluster.stop();
}

/// Verifies that `check_unsubmitted_jobs` returns early without panicking
/// when the cluster is offline (no WS connection set).
///
/// # Setup
/// A job and an old PENDING history row are inserted. The cluster is created without calling
/// `set_connection`.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called.
///
/// # Assert
/// The call completes without panicking.
#[tokio::test]
async fn test_check_unsubmitted_jobs_skips_offline_cluster() {
    let db = make_db().await;

    insert_job(&db, 3, "ozstar", "b", "app", "{}").await;
    insert_history(&db, 3, old_timestamp(), "sub", 10).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));
    // Do NOT call set_connection — cluster is offline

    // Should return early without panicking
    cluster.check_unsubmitted_jobs().await;
}

// ---------------------------------------------------------------------------
// check_cancelling_jobs: old state=75 → CANCEL_JOB resent
// ---------------------------------------------------------------------------

/// Verifies that `check_cancelling_jobs` re-sends a `CANCEL_JOB` message for a job
/// stuck in CANCELLING state (state=75) with an old timestamp.
///
/// # Setup
/// A job and a history row with `state=75` and timestamp `2000-01-01` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_cancelling_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// At least one `CANCEL_JOB` message is present in the output.
#[tokio::test]
async fn test_check_cancelling_jobs_resends_old_cancelling() {
    let db = make_db().await;

    insert_job(&db, 10, "ozstar", "b", "app", "{}").await;
    insert_history(&db, 10, old_timestamp(), "cancel", 75).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    cluster.check_cancelling_jobs().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut messages = Vec::new();
    while let Ok(outbound) = rx.try_recv() {
        let WsOutbound::Binary(data) = outbound else {
            continue;
        };
        messages.push(data);
    }

    let cancel_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes(data.to_vec()).id() == CANCEL_JOB)
        .collect();

    assert!(!cancel_msgs.is_empty(), "Expected at least one CANCEL_JOB");
    cluster.stop();
}

/// Verifies that `check_cancelling_jobs` does NOT re-send `CANCEL_JOB` for a job
/// in CANCELLING state (state=75) with a recent timestamp.
///
/// # Setup
/// A job and a history row with `state=75` and `timestamp=NOW` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_cancelling_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// No `CANCEL_JOB` messages appear in the output.
#[tokio::test]
async fn test_check_cancelling_jobs_ignores_recent() {
    let db = make_db().await;

    insert_job(&db, 11, "ozstar", "b", "app", "{}").await;
    insert_history(&db, 11, now_timestamp(), "cancel", 75).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    cluster.check_cancelling_jobs().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let cancel_msgs: Vec<_> = {
        let mut msgs = Vec::new();
        while let Ok(outbound) = rx.try_recv() {
            let WsOutbound::Binary(data) = outbound else {
                continue;
            };
            msgs.push(data);
        }
        msgs.into_iter()
            .filter(|d| Message::from_bytes(d.clone()).id() == CANCEL_JOB)
            .collect()
    };

    assert!(cancel_msgs.is_empty(), "Recent cancel should NOT be resent");
    cluster.stop();
}

// ---------------------------------------------------------------------------
// check_deleting_jobs: old state=85 → DELETE_JOB resent
// ---------------------------------------------------------------------------

/// Verifies that `check_deleting_jobs` re-sends a `DELETE_JOB` message for a job
/// stuck in DELETING state (state=85) with an old timestamp.
///
/// # Setup
/// A job and a history row with `state=85` and timestamp `2000-01-01` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_deleting_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// At least one `DELETE_JOB` message is present in the output.
#[tokio::test]
async fn test_check_deleting_jobs_resends_old_deleting() {
    let db = make_db().await;

    insert_job(&db, 20, "ozstar", "b", "app", "{}").await;
    insert_history(&db, 20, old_timestamp(), "delete", 85).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    cluster.check_deleting_jobs().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut messages = Vec::new();
    while let Ok(outbound) = rx.try_recv() {
        let WsOutbound::Binary(data) = outbound else {
            continue;
        };
        messages.push(data);
    }

    let delete_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes(data.to_vec()).id() == DELETE_JOB)
        .collect();

    assert!(!delete_msgs.is_empty(), "Expected at least one DELETE_JOB");
    cluster.stop();
}

/// Verifies that `check_deleting_jobs` does NOT re-send `DELETE_JOB` for a job
/// in DELETING state (state=85) with a recent timestamp.
///
/// # Setup
/// A job and a history row with `state=85` and `timestamp=NOW` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_deleting_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// No `DELETE_JOB` messages appear in the output.
#[tokio::test]
async fn test_check_deleting_jobs_ignores_recent() {
    let db = make_db().await;

    insert_job(&db, 21, "ozstar", "b", "app", "{}").await;
    insert_history(&db, 21, now_timestamp(), "delete", 85).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    cluster.check_deleting_jobs().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let delete_msgs: Vec<_> = {
        let mut msgs = Vec::new();
        while let Ok(outbound) = rx.try_recv() {
            let WsOutbound::Binary(data) = outbound else {
                continue;
            };
            msgs.push(data);
        }
        msgs.into_iter()
            .filter(|d| Message::from_bytes(d.clone()).id() == DELETE_JOB)
            .collect()
    };

    assert!(delete_msgs.is_empty(), "Recent delete should NOT be resent");
    cluster.stop();
}

// ---------------------------------------------------------------------------
// check_* with wrong cluster: jobs from another cluster not resent
// ---------------------------------------------------------------------------

/// Verifies that `check_unsubmitted_jobs` does not resubmit jobs belonging to a different cluster.
///
/// # Setup
/// A job assigned to cluster `"nci"` with an old PENDING history row is inserted.
/// The cluster under test is named `"ozstar"`.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// No `SUBMIT_JOB` messages appear in the output.
#[tokio::test]
async fn test_check_unsubmitted_jobs_only_for_own_cluster() {
    let db = make_db().await;

    // Job belongs to "nci", cluster is "ozstar" — should NOT resend
    insert_job(&db, 30, "nci", "b", "app", "{}").await;
    insert_history(&db, 30, old_timestamp(), "sub", 10).await;

    let ctx = make_app_context(db);
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    cluster.check_unsubmitted_jobs().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let submit_msgs: Vec<_> = {
        let mut msgs = Vec::new();
        while let Ok(outbound) = rx.try_recv() {
            let WsOutbound::Binary(data) = outbound else {
                continue;
            };
            msgs.push(data);
        }
        msgs.into_iter()
            .filter(|d| Message::from_bytes(d.clone()).id() == SUBMIT_JOB)
            .collect()
    };

    assert!(
        submit_msgs.is_empty(),
        "Jobs from different cluster should NOT be resubmitted"
    );
    cluster.stop();
}

// ---------------------------------------------------------------------------
// Noop status tests: check_unsubmitted/cancelling/deleting_jobs ignores
// all non-matching statuses
// ---------------------------------------------------------------------------

/// All JobStatus values that should NOT trigger check_unsubmitted_jobs.
/// (Only PENDING=10 and SUBMITTING=20 should trigger.)
const UNSUBMITTED_NOOP_STATES: &[i32] = &[
    30,  // Submitted
    40,  // Queued
    50,  // Running
    75,  // Cancelling
    80,  // Cancelled
    85,  // Deleting
    90,  // Deleted
    100, // Error
    200, // WallTimeExceeded
    300, // OutOfMemory
    500, // Completed
];

/// Verifies that `check_unsubmitted_jobs` does not trigger `SUBMIT_JOB` for any status
/// other than PENDING (10) or SUBMITTING (20).
///
/// # Setup
/// For each status in `UNSUBMITTED_NOOP_STATES`, an old history row with that status is inserted
/// for a job on `"ozstar"`. The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called for each status, and the channel is drained.
///
/// # Assert
/// No `SUBMIT_JOB` messages appear for any of the non-matching status values.
#[tokio::test]
async fn test_check_unsubmitted_jobs_noop_for_non_matching_statuses() {
    for &state_val in UNSUBMITTED_NOOP_STATES {
        let db = make_db().await;

        insert_job(&db, 100, "ozstar", "b", "app", "{}").await;
        insert_history(&db, 100, old_timestamp(), "test", state_val).await;

        let ctx = make_app_context(db.clone());
        let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
        use adacs_job_controller::cluster::traits::ClusterTrait;
        cluster.set_connection(Some(tx));
        cluster.start_tasks();

        cluster.check_unsubmitted_jobs().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let submit_msgs: Vec<_> = {
            let mut msgs = Vec::new();
            while let Ok(outbound) = rx.try_recv() {
                let WsOutbound::Binary(data) = outbound else {
                    continue;
                };
                msgs.push(data);
            }
            msgs.into_iter()
                .filter(|d| Message::from_bytes(d.clone()).id() == SUBMIT_JOB)
                .collect()
        };

        assert!(
            submit_msgs.is_empty(),
            "State {} should NOT trigger SUBMIT_JOB",
            state_val
        );
        cluster.stop();
    }
}

/// All JobStatus values that should NOT trigger check_cancelling_jobs.
/// (Only CANCELLING=75 should trigger.)
const CANCELLING_NOOP_STATES: &[i32] = &[
    10,  // Pending
    20,  // Submitting
    30,  // Submitted
    40,  // Queued
    50,  // Running
    80,  // Cancelled
    85,  // Deleting
    90,  // Deleted
    100, // Error
    200, // WallTimeExceeded
    300, // OutOfMemory
    500, // Completed
];

/// Verifies that `check_cancelling_jobs` does not trigger `CANCEL_JOB` for any status
/// other than CANCELLING (75).
///
/// # Setup
/// For each status in `CANCELLING_NOOP_STATES`, an old history row with that status is inserted
/// for a job on `"ozstar"`. The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_cancelling_jobs().await` is called for each status, and the channel is drained.
///
/// # Assert
/// No `CANCEL_JOB` messages appear for any of the non-matching status values.
#[tokio::test]
async fn test_check_cancelling_jobs_noop_for_non_matching_statuses() {
    for &state_val in CANCELLING_NOOP_STATES {
        let db = make_db().await;

        insert_job(&db, 200, "ozstar", "b", "app", "{}").await;
        insert_history(&db, 200, old_timestamp(), "test", state_val).await;

        let ctx = make_app_context(db.clone());
        let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
        use adacs_job_controller::cluster::traits::ClusterTrait;
        cluster.set_connection(Some(tx));
        cluster.start_tasks();

        cluster.check_cancelling_jobs().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let cancel_msgs: Vec<_> = {
            let mut msgs = Vec::new();
            while let Ok(outbound) = rx.try_recv() {
                let WsOutbound::Binary(data) = outbound else {
                    continue;
                };
                msgs.push(data);
            }
            msgs.into_iter()
                .filter(|d| Message::from_bytes(d.clone()).id() == CANCEL_JOB)
                .collect()
        };

        assert!(
            cancel_msgs.is_empty(),
            "State {} should NOT trigger CANCEL_JOB",
            state_val
        );
        cluster.stop();
    }
}

/// All JobStatus values that should NOT trigger check_deleting_jobs.
/// (Only DELETING=85 should trigger.)
const DELETING_NOOP_STATES: &[i32] = &[
    10,  // Pending
    20,  // Submitting
    30,  // Submitted
    40,  // Queued
    50,  // Running
    75,  // Cancelling
    80,  // Cancelled
    90,  // Deleted
    100, // Error
    200, // WallTimeExceeded
    300, // OutOfMemory
    500, // Completed
];

/// Verifies that `check_deleting_jobs` does not trigger `DELETE_JOB` for any status
/// other than DELETING (85).
///
/// # Setup
/// For each status in `DELETING_NOOP_STATES`, an old history row with that status is inserted
/// for a job on `"ozstar"`. The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_deleting_jobs().await` is called for each status, and the channel is drained.
///
/// # Assert
/// No `DELETE_JOB` messages appear for any of the non-matching status values.
#[tokio::test]
async fn test_check_deleting_jobs_noop_for_non_matching_statuses() {
    for &state_val in DELETING_NOOP_STATES {
        let db = make_db().await;

        insert_job(&db, 300, "ozstar", "b", "app", "{}").await;
        insert_history(&db, 300, old_timestamp(), "test", state_val).await;

        let ctx = make_app_context(db.clone());
        let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
        use adacs_job_controller::cluster::traits::ClusterTrait;
        cluster.set_connection(Some(tx));
        cluster.start_tasks();

        cluster.check_deleting_jobs().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let delete_msgs: Vec<_> = {
            let mut msgs = Vec::new();
            while let Ok(outbound) = rx.try_recv() {
                let WsOutbound::Binary(data) = outbound else {
                    continue;
                };
                msgs.push(data);
            }
            msgs.into_iter()
                .filter(|d| Message::from_bytes(d.clone()).id() == DELETE_JOB)
                .collect()
        };

        assert!(
            delete_msgs.is_empty(),
            "State {} should NOT trigger DELETE_JOB",
            state_val
        );
        cluster.stop();
    }
}

// ---------------------------------------------------------------------------
// SUBMITTING status also triggers resubmit
// (state=20 in check_unsubmitted_jobs also triggers resubmission)
// ---------------------------------------------------------------------------

/// Verifies that `check_unsubmitted_jobs` re-sends a `SUBMIT_JOB` message for a job
/// stuck in SUBMITTING state (state=20) with an old timestamp.
///
/// # Setup
/// A job and a history row with `state=20` and timestamp `2000-01-01` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// At least one `SUBMIT_JOB` message is present in the output.
#[tokio::test]
async fn test_check_unsubmitted_jobs_resends_old_submitting() {
    let db = make_db().await;

    insert_job(&db, 400, "ozstar", "mybundle", "myapp", r#"{"key":"val"}"#).await;

    // Insert history with state=20 (SUBMITTING) and timestamp far in the past
    insert_history(&db, 400, old_timestamp(), "submit", 20).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    cluster.check_unsubmitted_jobs().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut messages = Vec::new();
    while let Ok(outbound) = rx.try_recv() {
        let WsOutbound::Binary(data) = outbound else {
            continue;
        };
        messages.push(data);
    }

    let submit_msgs: Vec<_> = messages
        .iter()
        .filter_map(|data| {
            let msg = Message::from_bytes(data.clone());
            if msg.id() == SUBMIT_JOB {
                Some(msg)
            } else {
                None
            }
        })
        .collect();

    assert!(
        !submit_msgs.is_empty(),
        "SUBMITTING state should trigger SUBMIT_JOB resubmission"
    );
    cluster.stop();
}
