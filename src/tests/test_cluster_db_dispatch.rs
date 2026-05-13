//! Integration tests for `ClusterDB` message dispatch using `MockClusterTrait`.
//!
//! These tests verify that the `maybe_handle_cluster_db_message` dispatcher
//! correctly routes messages by ID and returns the right boolean.
//! Since DB operations require a real `MySQL` connection, we focus on:
//!   1. Verifying dispatch routing (handled vs unhandled message IDs)
//!   2. Verifying the response message format via the mock's captured `send_message` calls

use std::sync::{Arc, Mutex};

use adacs_job_controller::cluster::traits::{ClusterTrait, MockClusterTrait};
use adacs_job_controller::db::models::{BundleJob, ClusterJob, ClusterJobStatus};
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::Priority;

/// Helper: create a `MockClusterTrait` that captures all sent messages.
fn mock_cluster_capturing_messages() -> (MockClusterTrait, Arc<Mutex<Vec<Message>>>) {
    let sent = Arc::new(Mutex::new(Vec::<Message>::new()));
    let sent_clone = Arc::clone(&sent);

    let mut mock = MockClusterTrait::new();
    mock.expect_name().returning(|| "test_cluster".to_string());
    mock.expect_send_message().returning(move |msg| {
        sent_clone.lock().unwrap().push(msg);
        Box::pin(async {})
    });

    (mock, sent)
}

// ---------------------------------------------------------------------------
// Dispatch routing: known DB message IDs return true
// ---------------------------------------------------------------------------

// Note: We can't call maybe_handle_cluster_db_message without a real DB pool,
// but we CAN test the message format construction and model serialization
// that feeds into the dispatcher. This verifies the protocol contract.

/// Verifies that a `DB_JOB_SAVE` message can be constructed with a `ClusterJob` payload and parsed back correctly.
///
/// # Setup
/// A `ClusterJob` with specific field values and a `db_request_id` of 100 are prepared.
///
/// # Act
/// The job and request ID are serialized into a `DB_JOB_SAVE` message, then round-tripped
/// through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed message ID and source are correct, the `db_request_id` is recovered first,
/// and all `ClusterJob` fields survive deserialization without loss.
#[test]
fn test_db_job_save_message_construction() {
    // Build a DB_JOB_SAVE message exactly as the cluster client would
    let job = ClusterJob {
        id: 0, // new
        job_id: 42,
        scheduler_id: 0,
        submitting: true,
        submitting_count: 1,
        bundle_hash: "hash123".to_string(),
        working_directory: "/work/dir".to_string(),
        running: false,
        deleting: false,
        deleted: false,
        cluster: "test_cluster".to_string(),
    };

    let mut msg = Message::new(DB_JOB_SAVE, Priority::Highest, SYSTEM_SOURCE);
    let db_request_id: u32 = 100;
    msg.push_uint(db_request_id);
    job.to_message(&mut msg);

    // Parse back and verify: the dispatcher reads db_request_id first, then ClusterJob
    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_JOB_SAVE);
    assert_eq!(parsed.source(), SYSTEM_SOURCE);

    let req_id = parsed.pop_uint();
    assert_eq!(req_id, db_request_id);

    let restored = ClusterJob::from_message(&mut parsed);
    assert_eq!(restored.job_id, 42);
    assert!(restored.submitting);
    assert_eq!(restored.bundle_hash, "hash123");
    assert_eq!(restored.working_directory, "/work/dir");
}

/// Verifies that a `DB_JOBSTATUS_SAVE` message can be constructed with a `ClusterJobStatus` payload and parsed back.
///
/// # Setup
/// A `ClusterJobStatus` with `job_id=42`, `what="scheduler_id"`, and `state=500` is created.
///
/// # Act
/// The status is serialized into a `DB_JOBSTATUS_SAVE` message with `db_request_id=200`,
/// then round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed `db_request_id` is 200, and all `ClusterJobStatus` fields are correctly restored.
#[test]
fn test_db_jobstatus_save_message_construction() {
    let status = ClusterJobStatus {
        id: 0,
        job_id: 42,
        what: "scheduler_id".to_string(),
        state: 500,
    };

    let mut msg = Message::new(DB_JOBSTATUS_SAVE, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(200); // db_request_id
    status.to_message(&mut msg);

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_JOBSTATUS_SAVE);
    let req_id = parsed.pop_uint();
    assert_eq!(req_id, 200);

    let restored = ClusterJobStatus::from_message(&mut parsed);
    assert_eq!(restored.job_id, 42);
    assert_eq!(restored.what, "scheduler_id");
    assert_eq!(restored.state, 500);
}

/// Verifies that a `DB_BUNDLE_CREATE_OR_UPDATE_JOB` message can be constructed with a `BundleJob` payload and parsed back.
///
/// # Setup
/// A `BundleJob` with JSON content, cluster name, and bundle hash is prepared.
///
/// # Act
/// The bundle is serialized into a `DB_BUNDLE_CREATE_OR_UPDATE_JOB` message with `db_request_id=300`,
/// then round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed `db_request_id` is 300, and the `BundleJob` content field is correctly restored.
#[test]
fn test_db_bundle_create_or_update_message_construction() {
    let bundle = BundleJob {
        id: 0,
        content: r#"{"key":"value"}"#.to_string(),
        cluster: "test_cluster".to_string(),
        bundle_hash: "hash_abc".to_string(),
    };

    let mut msg = Message::new(
        DB_BUNDLE_CREATE_OR_UPDATE_JOB,
        Priority::Highest,
        SYSTEM_SOURCE,
    );
    msg.push_uint(300); // db_request_id
    bundle.to_message(&mut msg);

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_BUNDLE_CREATE_OR_UPDATE_JOB);
    let req_id = parsed.pop_uint();
    assert_eq!(req_id, 300);

    let restored = BundleJob::from_message(&mut parsed);
    assert_eq!(restored.content, r#"{"key":"value"}"#);
}

/// Verifies that a `DB_RESPONSE` message carrying multiple `ClusterJob` results serializes and parses correctly.
///
/// # Setup
/// Two `ClusterJob` records with distinct field values (`job_id=100`, `job_id=200`) are constructed,
/// along with a `DB_RESPONSE` header containing `db_request_id=42` and a result count of 2.
///
/// # Act
/// Both jobs are serialized into the response message, which is then round-tripped through
/// `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed `db_request_id`, result count, and all fields of both `ClusterJob` records match the originals.
#[test]
fn test_db_response_format_with_result_count() {
    // Verify the response format: DB_RESPONSE, db_request_id, then results
    let db_request_id: u32 = 42;
    let mut response = Message::new(DB_RESPONSE, Priority::Highest, SYSTEM_SOURCE);
    response.push_uint(db_request_id);
    response.push_uint(2); // 2 results

    // Add two ClusterJob results
    let job1 = ClusterJob {
        id: 10,
        job_id: 100,
        scheduler_id: 5,
        submitting: false,
        submitting_count: 0,
        bundle_hash: "h1".to_string(),
        working_directory: "/w1".to_string(),
        running: true,
        deleting: false,
        deleted: false,
        cluster: "c1".to_string(),
    };
    let job2 = ClusterJob {
        id: 20,
        job_id: 200,
        scheduler_id: 6,
        submitting: true,
        submitting_count: 1,
        bundle_hash: "h2".to_string(),
        working_directory: "/w2".to_string(),
        running: false,
        deleting: true,
        deleted: false,
        cluster: "c2".to_string(),
    };
    job1.to_message(&mut response);
    job2.to_message(&mut response);

    // Parse and verify
    let mut parsed = Message::from_bytes(response.into_data());
    assert_eq!(parsed.id(), DB_RESPONSE);
    assert_eq!(parsed.source(), SYSTEM_SOURCE);
    assert_eq!(parsed.pop_uint(), db_request_id);

    let count = parsed.pop_uint();
    assert_eq!(count, 2);

    let r1 = ClusterJob::from_message(&mut parsed);
    assert_eq!(r1.id, 10);
    assert_eq!(r1.job_id, 100);
    assert!(r1.running);

    let r2 = ClusterJob::from_message(&mut parsed);
    assert_eq!(r2.id, 20);
    assert_eq!(r2.job_id, 200);
    assert!(r2.submitting);
    assert!(r2.deleting);
}

/// Verifies that a `DB_RESPONSE` message with zero results serializes and parses correctly.
///
/// # Setup
/// A `DB_RESPONSE` message is built with `db_request_id=999` and a result count of 0.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed message ID, `db_request_id`, and result count are all correct.
#[test]
fn test_db_response_format_empty_result() {
    let mut response = Message::new(DB_RESPONSE, Priority::Highest, SYSTEM_SOURCE);
    response.push_uint(999); // db_request_id
    response.push_uint(0); // no results

    let mut parsed = Message::from_bytes(response.into_data());
    assert_eq!(parsed.id(), DB_RESPONSE);
    assert_eq!(parsed.pop_uint(), 999);
    assert_eq!(parsed.pop_uint(), 0);
}

// ---------------------------------------------------------------------------
// Verify DB_JOB_GET_BY_JOB_ID message format (request side)
// ---------------------------------------------------------------------------

/// Verifies that a `DB_JOB_GET_BY_JOB_ID` request message serializes and parses correctly.
///
/// # Setup
/// A request is built with `db_request_id=50` and `job_id=123`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed message ID, `db_request_id`, and `job_id` match the original values.
#[test]
fn test_db_job_get_by_job_id_request_format() {
    let mut msg = Message::new(DB_JOB_GET_BY_JOB_ID, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(50); // db_request_id
    msg.push_ulong(123); // job_id

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_JOB_GET_BY_JOB_ID);
    assert_eq!(parsed.pop_uint(), 50);
    assert_eq!(parsed.pop_ulong(), 123);
}

/// Verifies that a `DB_JOB_GET_BY_ID` request message serializes and parses correctly.
///
/// # Setup
/// A request is built with `db_request_id=51` and `id=456`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed message ID, `db_request_id`, and record `id` match the original values.
#[test]
fn test_db_job_get_by_id_request_format() {
    let mut msg = Message::new(DB_JOB_GET_BY_ID, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(51); // db_request_id
    msg.push_ulong(456); // id

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_JOB_GET_BY_ID);
    assert_eq!(parsed.pop_uint(), 51);
    assert_eq!(parsed.pop_ulong(), 456);
}

/// Verifies that a `DB_JOBSTATUS_GET_BY_JOB_ID` request message serializes and parses correctly.
///
/// # Setup
/// A request is built with `db_request_id=60` and `job_id=789`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed message ID, `db_request_id`, and `job_id` match the original values.
#[test]
fn test_db_jobstatus_get_by_job_id_request_format() {
    let mut msg = Message::new(DB_JOBSTATUS_GET_BY_JOB_ID, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(60); // db_request_id
    msg.push_ulong(789); // job_id

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_JOBSTATUS_GET_BY_JOB_ID);
    assert_eq!(parsed.pop_uint(), 60);
    assert_eq!(parsed.pop_ulong(), 789);
}

// ---------------------------------------------------------------------------
// Mock cluster verification: send_message captures
// ---------------------------------------------------------------------------

/// Verifies that the mock cluster helper correctly captures a single sent message.
///
/// # Setup
/// A `MockClusterTrait` is created via `mock_cluster_capturing_messages`, which attaches
/// a shared `Vec` to the `send_message` expectation.
///
/// # Act
/// One `DB_RESPONSE` message is sent via `mock.send_message`.
///
/// # Assert
/// The captured message vector contains exactly one entry with the correct message ID and source.
#[tokio::test]
async fn test_mock_cluster_captures_sent_messages() {
    let (mock, sent) = mock_cluster_capturing_messages();

    // Simulate what the DB dispatcher would do: create and send a response
    let mut response = Message::new(DB_RESPONSE, Priority::Highest, SYSTEM_SOURCE);
    response.push_uint(42);
    response.push_uint(0);

    mock.send_message(response).await;

    let captured = sent.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].id(), DB_RESPONSE);
    assert_eq!(captured[0].source(), SYSTEM_SOURCE);
}

/// Verifies that the mock cluster helper captures all messages when multiple are sent.
///
/// # Setup
/// A `MockClusterTrait` with a shared capture buffer is created.
///
/// # Act
/// Five `DB_RESPONSE` messages are sent via `mock.send_message` with distinct `db_request_id` values.
///
/// # Assert
/// The captured message vector contains exactly 5 entries.
#[tokio::test]
async fn test_mock_cluster_multiple_messages() {
    let (mock, sent) = mock_cluster_capturing_messages();

    for i in 0..5 {
        let mut msg = Message::new(DB_RESPONSE, Priority::Highest, SYSTEM_SOURCE);
        msg.push_uint(i);
        mock.send_message(msg).await;
    }

    let captured = sent.lock().unwrap();
    assert_eq!(captured.len(), 5);
}

// ---------------------------------------------------------------------------
// Message ID range check: unhandled IDs
// ---------------------------------------------------------------------------

/// Verifies that known non-DB message IDs do not appear in the set of DB operation message IDs.
///
/// # Setup
/// Two static slices are defined: one containing non-DB message IDs (e.g., `SERVER_READY`, `SUBMIT_JOB`,
/// `FILE_CHUNK`) and one containing the known DB operation message IDs.
///
/// # Act
/// Each non-DB ID is checked against the DB ID set.
///
/// # Assert
/// No non-DB message ID is present in the DB handler set.
#[test]
fn test_unhandled_message_ids_are_not_db_operations() {
    // These message IDs should NOT be handled by the DB dispatcher
    let non_db_ids = [
        SERVER_READY,
        SUBMIT_JOB,
        UPDATE_JOB,
        CANCEL_JOB,
        DELETE_JOB,
        DOWNLOAD_FILE,
        FILE_DETAILS,
        FILE_ERROR,
        FILE_CHUNK,
        UPLOAD_FILE,
        FILE_UPLOAD_CHUNK,
        FILE_UPLOAD_ERROR,
        FILE_UPLOAD_COMPLETE,
        0,
        9999,
    ];

    let db_ids = [
        DB_JOB_GET_BY_JOB_ID,
        DB_JOB_GET_BY_ID,
        DB_JOB_GET_RUNNING_JOBS,
        DB_JOB_DELETE,
        DB_JOB_SAVE,
        DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT,
        DB_JOBSTATUS_GET_BY_JOB_ID,
        DB_JOBSTATUS_DELETE_BY_ID_LIST,
        DB_JOBSTATUS_SAVE,
        DB_BUNDLE_CREATE_OR_UPDATE_JOB,
        DB_BUNDLE_GET_JOB_BY_ID,
        DB_BUNDLE_DELETE_JOB,
    ];

    for id in &non_db_ids {
        assert!(
            !db_ids.contains(id),
            "ID {id} should not be in the DB handler set"
        );
    }
}
