//! Integration tests for cluster business logic:
//! `handle_update_job`, `check_unsubmitted_jobs`, `check_cancelling_jobs`,
//! `check_deleting_jobs`.
//!
//! Each test spins up a real `Cluster` with a SQLite-backed `AppContext`
//! and verifies both DB side-effects and outgoing WS messages.

mod common;

use std::sync::Arc;

use common::{insert_job_history_at, make_app_context, setup_test_db, test_cluster_config};

use adacs_job_controller::cluster::cluster::Cluster;
use adacs_job_controller::cluster::traits::ClusterTrait;
use adacs_job_controller::cluster::traits::WsOutbound;
use adacs_job_controller::db::entities::{job, job_history};
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::Priority;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use tokio::sync::mpsc::UnboundedReceiver;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Create an online `Cluster` for `"ozstar"` with a live WS sender and a
/// started scheduler, returning the cluster and the outbound receiver.
async fn make_online_cluster(
    db: &DatabaseConnection,
) -> (
    Arc<Cluster>,
    tokio::sync::mpsc::UnboundedReceiver<WsOutbound>,
) {
    let ctx = make_app_context(db.clone());
    common::make_online_cluster("ozstar", Some(ctx)).await
}

/// Make an `UPDATE_JOB` message with the given fields.
fn make_update_job_message(job_id: u32, what: &str, status: u32, details: &str) -> Message {
    let mut msg = Message::new(UPDATE_JOB, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(job_id);
    msg.push_string(what);
    msg.push_uint(status);
    msg.push_string(details);
    // Round-trip so id() and source() are properly set
    Message::from_bytes(msg.into_data())
}

/// Drain all pending `WsOutbound` messages from the channel, keeping only the
/// binary payloads (non-binary messages are skipped).
fn drain_binary_messages(rx: &mut UnboundedReceiver<WsOutbound>) -> Vec<Vec<u8>> {
    let mut messages = Vec::new();
    while let Ok(outbound) = rx.try_recv() {
        if let WsOutbound::Binary(data) = outbound {
            messages.push(data);
        }
    }
    messages
}

// ---------------------------------------------------------------------------
// handle_update_job: verify JobserverJobhistory row is inserted
// ---------------------------------------------------------------------------

/// Verifies that `handle_message` with an `UPDATE_JOB` message inserts a row into `JobserverJobhistory`.
///
/// # Setup
/// An in-memory `SQLite` DB with the `JobserverJob` and `JobserverJobhistory` tables is created.
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
    let db = setup_test_db().await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

    let msg = make_update_job_message(42, "job_submission", 10, "submitted to scheduler");

    // handle_message dispatches to handle_update_job for UPDATE_JOB
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
/// An in-memory `SQLite` DB with the required tables is created. Three `UPDATE_JOB` messages
/// are built for `job_id=7` with states 10 (Pending), 40 (Queued), and 500 (Completed).
///
/// # Act
/// All three messages are dispatched via `cluster.handle_message`.
///
/// # Assert
/// `JobserverJobhistory` contains exactly 3 rows for `jobId=7`.
#[tokio::test]
async fn test_handle_update_job_multiple_updates() {
    let db = setup_test_db().await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));

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
    let db = setup_test_db().await;

    // Insert a job on "ozstar"
    insert_job(&db, 1, "ozstar", "mybundle", "myapp", "{}").await;

    // Insert history with state=10 (PENDING) and timestamp far in the past
    insert_job_history_at(&db, 1, 10, "submit", old_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    // Call check_unsubmitted_jobs
    cluster.check_unsubmitted_jobs().await;

    // Wait for scheduler to forward all queued messages
    cluster.wait_for_queue_drain(true).await;

    // Drain channel
    let messages = drain_binary_messages(&mut rx);

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

    assert_eq!(
        submit_msgs.len(),
        1,
        "Expected exactly one SUBMIT_JOB message"
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
    let db = setup_test_db().await;

    insert_job(&db, 2, "ozstar", "mybundle", "myapp", "{}").await;

    // Insert history with state=10 and timestamp = NOW (within the ignore window)
    insert_job_history_at(&db, 2, 10, "submit", now_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_unsubmitted_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let messages = drain_binary_messages(&mut rx);

    let submit_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes((*data).clone()).id() == SUBMIT_JOB)
        .collect();

    assert!(
        submit_msgs.is_empty(),
        "Recent job should NOT trigger SUBMIT_JOB"
    );
    cluster.stop();
}

/// Verifies that `check_unsubmitted_jobs` does NOT re-send `SUBMIT_JOB` for a job
/// whose most recent history row is a terminal state (Completed) written within
/// the ignore window, even when an older stale PENDING row exists.
///
/// # Setup
/// A job with an old PENDING history row and a recent COMPLETED history row is inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// No `SUBMIT_JOB` messages appear in the output.
#[tokio::test]
async fn test_check_unsubmitted_jobs_ignores_recent_terminal_state() {
    let db = setup_test_db().await;

    insert_job(&db, 5, "ozstar", "mybundle", "myapp", "{}").await;

    // Old PENDING row (would trigger resubmit on its own) ...
    insert_job_history_at(&db, 5, 10, "submit", old_timestamp()).await;
    // ... followed by a recent COMPLETED row within the ignore window.
    insert_job_history_at(&db, 5, 500, "complete", now_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_unsubmitted_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let messages = drain_binary_messages(&mut rx);

    let submit_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes((*data).clone()).id() == SUBMIT_JOB)
        .collect();

    assert!(
        submit_msgs.is_empty(),
        "Job with recent terminal state should NOT trigger SUBMIT_JOB"
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
    let db = setup_test_db().await;

    insert_job(&db, 3, "ozstar", "b", "app", "{}").await;
    insert_job_history_at(&db, 3, 10, "sub", old_timestamp()).await;

    let ctx = make_app_context(db.clone());
    let cluster = Cluster::new(test_cluster_config("ozstar"), Some(ctx));
    // Do NOT call set_connection — cluster is offline

    // Should return early without panicking
    cluster.check_unsubmitted_jobs().await;
}

// ---------------------------------------------------------------------------
// check_cancelling_jobs: old state=60 → CANCEL_JOB resent
// ---------------------------------------------------------------------------

/// Verifies that `check_cancelling_jobs` re-sends a `CANCEL_JOB` message for a job
/// stuck in CANCELLING state (state=60) with an old timestamp.
///
/// # Setup
/// A job and a history row with `state=60` and timestamp `2000-01-01` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_cancelling_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// At least one `CANCEL_JOB` message is present in the output.
#[tokio::test]
async fn test_check_cancelling_jobs_resends_old_cancelling() {
    let db = setup_test_db().await;

    insert_job(&db, 10, "ozstar", "b", "app", "{}").await;
    insert_job_history_at(&db, 10, 60, "cancel", old_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_cancelling_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let messages = drain_binary_messages(&mut rx);

    let cancel_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes((*data).clone()).id() == CANCEL_JOB)
        .collect();

    assert_eq!(cancel_msgs.len(), 1, "Expected exactly one CANCEL_JOB");
    cluster.stop();
}

/// Verifies that `check_cancelling_jobs` does NOT re-send `CANCEL_JOB` for a job
/// in CANCELLING state (state=60) with a recent timestamp.
///
/// # Setup
/// A job and a history row with `state=60` and `timestamp=NOW` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_cancelling_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// No `CANCEL_JOB` messages appear in the output.
#[tokio::test]
async fn test_check_cancelling_jobs_ignores_recent() {
    let db = setup_test_db().await;

    insert_job(&db, 11, "ozstar", "b", "app", "{}").await;
    insert_job_history_at(&db, 11, 60, "cancel", now_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_cancelling_jobs().await;
    cluster.wait_for_queue_drain(true).await;

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

/// Verifies that `check_cancelling_jobs` re-sends a `CANCEL_JOB` message for a job
/// stuck in CANCELLING state even when it also has a Pending(10) creation row.
///
/// # Setup
/// A job and two history rows with old timestamps are inserted: state=10 (Pending,
/// written at job creation) and state=60 (Cancelling).
///
/// # Act
/// `cluster.check_cancelling_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// At least one `CANCEL_JOB` message is present in the output.
#[tokio::test]
async fn test_check_cancelling_jobs_resends_with_pending_history() {
    let db = setup_test_db().await;

    insert_job(&db, 12, "ozstar", "b", "app", "{}").await;
    insert_job_history_at(&db, 12, 10, "created", old_timestamp()).await;
    insert_job_history_at(&db, 12, 60, "cancel", old_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_cancelling_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let messages = drain_binary_messages(&mut rx);

    let cancel_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes((*data).clone()).id() == CANCEL_JOB)
        .collect();

    assert_eq!(
        cancel_msgs.len(),
        1,
        "Expected exactly one CANCEL_JOB despite Pending history row"
    );
    cluster.stop();
}

// ---------------------------------------------------------------------------
// check_deleting_jobs: old state=80 → DELETE_JOB resent
// ---------------------------------------------------------------------------

/// Verifies that `check_deleting_jobs` re-sends a `DELETE_JOB` message for a job
/// stuck in DELETING state (state=80) with an old timestamp.
///
/// # Setup
/// A job and a history row with `state=80` and timestamp `2000-01-01` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_deleting_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// At least one `DELETE_JOB` message is present in the output.
#[tokio::test]
async fn test_check_deleting_jobs_resends_old_deleting() {
    let db = setup_test_db().await;

    insert_job(&db, 20, "ozstar", "b", "app", "{}").await;
    insert_job_history_at(&db, 20, 80, "delete", old_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_deleting_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let messages = drain_binary_messages(&mut rx);

    let delete_msgs: Vec<_> = messages
        .iter()
        .filter(|data| Message::from_bytes((*data).clone()).id() == DELETE_JOB)
        .collect();

    assert_eq!(delete_msgs.len(), 1, "Expected exactly one DELETE_JOB");
    cluster.stop();
}

/// Verifies that `check_deleting_jobs` does NOT re-send `DELETE_JOB` for a job
/// in DELETING state (state=80) with a recent timestamp.
///
/// # Setup
/// A job and a history row with `state=80` and `timestamp=NOW` are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_deleting_jobs().await` is called, then the channel is drained after a short wait.
///
/// # Assert
/// No `DELETE_JOB` messages appear in the output.
#[tokio::test]
async fn test_check_deleting_jobs_ignores_recent() {
    let db = setup_test_db().await;

    insert_job(&db, 21, "ozstar", "b", "app", "{}").await;
    insert_job_history_at(&db, 21, 80, "delete", now_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_deleting_jobs().await;
    cluster.wait_for_queue_drain(true).await;

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
    let db = setup_test_db().await;

    // Job belongs to "nci", cluster is "ozstar" — should NOT resend
    insert_job(&db, 30, "nci", "b", "app", "{}").await;
    insert_job_history_at(&db, 30, 10, "sub", old_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_unsubmitted_jobs().await;
    cluster.wait_for_queue_drain(true).await;

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

/// All `JobStatus` values that should NOT trigger `check_unsubmitted_jobs`.
/// (Only PENDING=10 and SUBMITTING=20 should trigger.)
const UNSUBMITTED_NOOP_STATES: &[i32] = &[
    30,  // Submitted
    40,  // Queued
    50,  // Running
    60,  // Cancelling
    70,  // Cancelled
    80,  // Deleting
    90,  // Deleted
    400, // Error
    401, // WallTimeExceeded
    402, // OutOfMemory
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
        let db = setup_test_db().await;

        insert_job(&db, 100, "ozstar", "b", "app", "{}").await;
        insert_job_history_at(&db, 100, state_val, "test", old_timestamp()).await;

        let (cluster, mut rx) = make_online_cluster(&db).await;

        cluster.check_unsubmitted_jobs().await;
        cluster.wait_for_queue_drain(true).await;

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
            "State {state_val} should NOT trigger SUBMIT_JOB"
        );
        cluster.stop();
    }
}

/// All `JobStatus` values that should NOT trigger `check_cancelling_jobs`.
/// (Only CANCELLING=60 should trigger.)
const CANCELLING_NOOP_STATES: &[i32] = &[
    10,  // Pending
    20,  // Submitting
    30,  // Submitted
    40,  // Queued
    50,  // Running
    70,  // Cancelled
    80,  // Deleting
    90,  // Deleted
    400, // Error
    401, // WallTimeExceeded
    402, // OutOfMemory
    500, // Completed
];

/// Verifies that `check_cancelling_jobs` does not trigger `CANCEL_JOB` for any status
/// other than CANCELLING (60).
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
        let db = setup_test_db().await;

        insert_job(&db, 200, "ozstar", "b", "app", "{}").await;
        insert_job_history_at(&db, 200, state_val, "test", old_timestamp()).await;

        let (cluster, mut rx) = make_online_cluster(&db).await;

        cluster.check_cancelling_jobs().await;
        cluster.wait_for_queue_drain(true).await;

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
            "State {state_val} should NOT trigger CANCEL_JOB"
        );
        cluster.stop();
    }
}

/// All `JobStatus` values that should NOT trigger `check_deleting_jobs`.
/// (Only DELETING=80 should trigger.)
const DELETING_NOOP_STATES: &[i32] = &[
    10,  // Pending
    20,  // Submitting
    30,  // Submitted
    40,  // Queued
    50,  // Running
    60,  // Cancelling
    70,  // Cancelled
    90,  // Deleted
    400, // Error
    401, // WallTimeExceeded
    402, // OutOfMemory
    500, // Completed
];

/// Verifies that `check_deleting_jobs` does not trigger `DELETE_JOB` for any status
/// other than DELETING (80).
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
        let db = setup_test_db().await;

        insert_job(&db, 300, "ozstar", "b", "app", "{}").await;
        insert_job_history_at(&db, 300, state_val, "test", old_timestamp()).await;

        let (cluster, mut rx) = make_online_cluster(&db).await;

        cluster.check_deleting_jobs().await;
        cluster.wait_for_queue_drain(true).await;

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
            "State {state_val} should NOT trigger DELETE_JOB"
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
    let db = setup_test_db().await;

    insert_job(&db, 400, "ozstar", "mybundle", "myapp", r#"{"key":"val"}"#).await;

    // Insert history with state=20 (SUBMITTING) and timestamp far in the past
    insert_job_history_at(&db, 400, 20, "submit", old_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_unsubmitted_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let messages = drain_binary_messages(&mut rx);

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

    assert_eq!(
        submit_msgs.len(),
        1,
        "SUBMITTING state should trigger exactly one SUBMIT_JOB resubmission"
    );
    cluster.stop();
}

/// Verifies that resend logic handles multiple stale jobs in one pass and emits one resubmission
/// per eligible job.
///
/// # Setup
/// Three old pending jobs and one recent pending job are inserted for the same cluster.
/// A live WS sender is attached so resent messages are forwarded through the scheduler.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called and the channel is drained.
///
/// # Assert
/// Exactly the three stale jobs are resubmitted, ensuring the batched resend path processes all
/// candidates instead of stopping at a single latest-history lookup.
#[tokio::test]
async fn test_check_unsubmitted_jobs_resends_all_stale_jobs_in_batch() {
    let db = setup_test_db().await;

    for job_id in 1..=3 {
        insert_job(
            &db,
            job_id,
            "ozstar",
            &format!("bundle-{job_id}"),
            "myapp",
            "{}",
        )
        .await;
        insert_job_history_at(&db, job_id, 10, "submit", old_timestamp()).await;
    }
    insert_job(&db, 4, "ozstar", "bundle-4", "myapp", "{}").await;
    insert_job_history_at(&db, 4, 10, "submit", now_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_unsubmitted_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let mut resent_ids = Vec::new();
    for _ in 0..3 {
        if let Ok(Some(WsOutbound::Binary(data))) =
            tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await
        {
            resent_ids.push(Message::from_bytes(data).pop_uint());
        }
    }

    cluster.stop();

    resent_ids.sort_unstable();
    assert_eq!(resent_ids, vec![1, 2, 3]);
}

/// Verifies that `check_unsubmitted_jobs` skips a stale PENDING job whose id exceeds
/// the `u32` range instead of panicking on the `u32::try_from` conversion.
///
/// # Setup
/// A job with `id = u32::MAX + 1` and an old PENDING history row are inserted.
/// The cluster is given a live WS sender and the scheduler is started.
///
/// # Act
/// `cluster.check_unsubmitted_jobs().await` is called, then the channel is drained.
///
/// # Assert
/// No `SUBMIT_JOB` message is emitted for the out-of-range job.
#[tokio::test]
async fn test_check_unsubmitted_jobs_skips_job_id_exceeding_u32_range() {
    let db = setup_test_db().await;

    let oversized_id = i64::from(u32::MAX) + 1;
    insert_job(&db, oversized_id, "ozstar", "mybundle", "myapp", "{}").await;
    insert_job_history_at(&db, oversized_id, 10, "submit", old_timestamp()).await;

    let (cluster, mut rx) = make_online_cluster(&db).await;

    cluster.check_unsubmitted_jobs().await;
    cluster.wait_for_queue_drain(true).await;

    let messages = drain_binary_messages(&mut rx);

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
        submit_msgs.is_empty(),
        "Job id exceeding u32 range should be skipped, not resubmitted"
    );
    cluster.stop();
}
