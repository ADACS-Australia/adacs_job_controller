//! Comprehensive tests for job HTTP handlers.
//!
//! All tests use a real SQLite in-memory database so the full business logic
//! (DB inserts, state machine transitions, WS message dispatch) is exercised.
//! Cluster interactions are mocked via MockClusterTrait / MockClusterManagerTrait.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use adacs_job_controller::cluster::traits::{MockClusterManagerTrait, MockClusterTrait};
use adacs_job_controller::db::entities::{job, job_history};
use adacs_job_controller::http::server::create_router;
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::{ClusterRole, JobStatus};

use common::{
    encode_test_jwt, insert_job_history, insert_test_job, make_test_state, setup_test_db,
    test_cluster_config,
};

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

// ---------------------------------------------------------------------------
// Helper: build a mock cluster that captures sent messages
// ---------------------------------------------------------------------------

fn online_cluster_capturing_messages(
    name: &str,
    sent: Arc<Mutex<Vec<Message>>>,
) -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    let n = name.to_string();
    c.expect_name().returning(move || n.clone());
    c.expect_is_online().returning(|| true);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    c.expect_send_message().returning(move |msg| {
        sent.lock().unwrap().push(msg);
    });
    c
}

fn offline_cluster() -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    c.expect_name().returning(|| "ozstar".to_string());
    c.expect_is_online().returning(|| false);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    c
}

// ---------------------------------------------------------------------------
// create_job tests
// ---------------------------------------------------------------------------

/// Tests the full happy-path job creation flow when the cluster is online.
///
/// # Setup
/// Inserts no pre-existing jobs. Wires an online cluster that captures sent WS messages.
///
/// # Act
/// Sends POST /job/apiv1/job/ with valid auth, cluster "ozstar", and a bundle.
///
/// # Assert
/// Verifies 200 OK, job row in DB, PENDING + SUBMITTING history entries,
/// and exactly one SUBMIT_JOB WS message containing the new job ID.
#[tokio::test]
async fn test_create_job_cluster_online_inserts_and_submits() {
    let db = setup_test_db().await;
    let sent = Arc::new(Mutex::new(vec![]));
    let cluster = Arc::new(online_cluster_capturing_messages(
        "ozstar",
        Arc::clone(&sent),
    ));

    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 42}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"cluster":"ozstar","parameters":"{}","bundle":"mybundle"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let job_id = body["jobId"].as_i64().expect("jobId should be present");
    assert!(job_id > 0);

    // Verify DB: job exists
    let j = job::Entity::find_by_id(job_id)
        .one(&db)
        .await
        .unwrap()
        .expect("job should be in DB");
    assert_eq!(j.cluster, "ozstar");
    assert_eq!(j.bundle, "mybundle");
    assert_eq!(j.application, "testapp");

    // Verify DB: PENDING + SUBMITTING history inserted
    let histories = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job_id))
        .order_by_asc(job_history::Column::Timestamp)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(histories.len(), 2);
    assert_eq!(histories[0].state, JobStatus::Pending as i32);
    assert_eq!(histories[1].state, JobStatus::Submitting as i32);

    // Verify WS: SUBMIT_JOB message was sent
    let msgs = sent.lock().unwrap();
    assert_eq!(msgs.len(), 1);
    let mut sent_msg = Message::from_bytes(msgs[0].clone().into_data());
    assert_eq!(sent_msg.id(), SUBMIT_JOB);
    let sent_job_id = sent_msg.pop_uint() as i64;
    assert_eq!(sent_job_id, job_id);
}

/// Tests that creating a job when the cluster is offline stores only a PENDING history entry.
///
/// # Setup
/// Wires an offline cluster (is_online = false).
///
/// # Act
/// Sends POST /job/apiv1/job/ with valid auth.
///
/// # Assert
/// Verifies 200 OK, exactly one PENDING history entry, and no WS message sent.
#[tokio::test]
async fn test_create_job_cluster_offline_only_pending_no_ws_message() {
    let db = setup_test_db().await;
    let cluster = Arc::new(offline_cluster());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"cluster":"ozstar","parameters":"{}","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let job_id = body["jobId"].as_i64().unwrap();

    // Only PENDING history — no SUBMITTING because cluster is offline
    let histories = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(histories.len(), 1);
    assert_eq!(histories[0].state, JobStatus::Pending as i32);
}

/// Tests that requesting a cluster not in the token's allowed list returns 400.
///
/// # Setup
/// Wires a manager that returns no cluster. Token authorises "ozstar" and "nci" only.
///
/// # Act
/// Sends POST /job/apiv1/job/ with cluster "unknown_cluster".
///
/// # Assert
/// Verifies 400 Bad Request with body containing "does not have access".
#[tokio::test]
async fn test_create_job_cluster_not_in_secret_returns_400() {
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"cluster":"unknown_cluster","parameters":"{}","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("does not have access"));
}

// ---------------------------------------------------------------------------
// cancel_job tests — state machine exhaustively tested
// ---------------------------------------------------------------------------

async fn run_cancel(
    job_id: i64,
    db: &sea_orm::DatabaseConnection,
    manager: MockClusterManagerTrait,
) -> (StatusCode, String) {
    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));
    let body = serde_json::json!({ "jobId": job_id }).to_string();

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn manager_with_online_cluster() -> (MockClusterManagerTrait, Arc<Mutex<Vec<Message>>>) {
    let sent = Arc::new(Mutex::new(vec![]));
    let cluster = Arc::new(online_cluster_capturing_messages(
        "ozstar",
        Arc::clone(&sent),
    ));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    (manager, sent)
}

fn manager_no_clusters() -> MockClusterManagerTrait {
    let mut m = MockClusterManagerTrait::new();
    m.expect_get_cluster_by_name().returning(|_| None);
    m
}

/// Tests that a PENDING job is directly transitioned to Cancelled without a WS message.
///
/// # Setup
/// Inserts a job with a PENDING history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 200 OK, latest history state is Cancelled, and no WS message was sent.
#[tokio::test]
async fn test_cancel_job_pending_directly_cancelled_no_ws() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;

    let (manager, sent) = manager_with_online_cluster();
    let (status, body) = run_cancel(job_id, &db, manager).await;

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains(&job_id.to_string()));

    // Latest state should be Cancelled
    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job_id))
        .order_by_desc(job_history::Column::Timestamp)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.state, JobStatus::Cancelled as i32);

    // No WS message should be sent for PENDING→Cancelled
    assert!(sent.lock().unwrap().is_empty());
}

/// Tests that cancelling a SUBMITTING job transitions it to Cancelling and sends CANCEL_JOB.
///
/// # Setup
/// Inserts a job with PENDING → SUBMITTING history.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 200 OK, latest history state is Cancelling, and a CANCEL_JOB WS message
/// containing the job ID was sent.
#[tokio::test]
async fn test_cancel_job_submitting_sends_cancel_ws_message() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job_id, JobStatus::Submitting as i32, "system").await;

    let (manager, sent) = manager_with_online_cluster();
    let (status, body) = run_cancel(job_id, &db, manager).await;

    assert_eq!(status, StatusCode::OK, "body: {body}");

    // Latest state should be Cancelling
    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job_id))
        .order_by_desc(job_history::Column::Timestamp)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.state, JobStatus::Cancelling as i32);

    // CANCEL_JOB WS message should be sent
    let msgs = sent.lock().unwrap();
    assert_eq!(msgs.len(), 1);
    let mut m = Message::from_bytes(msgs[0].clone().into_data());
    assert_eq!(m.id(), CANCEL_JOB);
    assert_eq!(m.pop_uint() as i64, job_id);
}

/// Tests that cancelling a RUNNING job sends a CANCEL_JOB WS message.
///
/// # Setup
/// Inserts a job with a RUNNING history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 200 OK and exactly one CANCEL_JOB WS message sent.
#[tokio::test]
async fn test_cancel_job_running_sends_cancel_ws_message() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Running as i32, "system").await;

    let (manager, sent) = manager_with_online_cluster();
    let (status, _) = run_cancel(job_id, &db, manager).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent.lock().unwrap().len(), 1);
    assert_eq!(
        Message::from_bytes(sent.lock().unwrap()[0].clone().into_data()).id(),
        CANCEL_JOB
    );
}

// All states that are invalid for cancellation
/// Tests that cancelling a job already in Cancelling state returns 400 with "invalid state".
///
/// # Setup
/// Inserts a job with a Cancelling history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "invalid state".
#[tokio::test]
async fn test_cancel_job_already_cancelling_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Cancelling as i32, "system").await;
    let (status, body) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid state"), "body: {body}");
}

/// Tests that cancelling an already Cancelled job returns 400 with "invalid state".
///
/// # Setup
/// Inserts a job with a Cancelled history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "invalid state".
#[tokio::test]
async fn test_cancel_job_already_cancelled_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Cancelled as i32, "system").await;
    let (status, body) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid state"), "body: {body}");
}

/// Tests that cancelling a Completed job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Completed history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_cancel_job_completed_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Completed as i32, "system").await;
    let (status, _) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that cancelling a WallTimeExceeded job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a WallTimeExceeded history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_cancel_job_wall_time_exceeded_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::WallTimeExceeded as i32, "system").await;
    let (status, _) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that cancelling an OutOfMemory job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with an OutOfMemory history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_cancel_job_out_of_memory_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::OutOfMemory as i32, "system").await;
    let (status, _) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that cancelling an Error-state job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with an Error history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_cancel_job_error_state_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Error as i32, "system").await;
    let (status, _) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that cancelling a Deleting job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Deleting history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_cancel_job_deleting_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Deleting as i32, "system").await;
    let (status, _) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that cancelling an already Deleted job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Deleted history entry.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_cancel_job_deleted_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Deleted as i32, "system").await;
    let (status, _) = run_cancel(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that cancelling a non-existent job ID returns 400 with "did not exist".
///
/// # Setup
/// Empty database with no jobs.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with job ID 99999.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "did not exist".
#[tokio::test]
async fn test_cancel_job_not_found_returns_400() {
    let db = setup_test_db().await;
    let (manager, _) = manager_with_online_cluster();
    let (status, body) = run_cancel(99999, &db, manager).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("did not exist"), "body: {body}");
}

/// Tests that cancelling a job whose cluster is not found by the manager returns 400.
///
/// # Setup
/// Inserts a PENDING job on cluster "nci". Wires a manager that returns None for all clusters.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ (cancel) with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_cancel_job_wrong_cluster_access_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "nci", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;

    let mut manager = MockClusterManagerTrait::new();
    // Cluster "nci" not found by manager
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({ "jobId": job_id }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// delete_job tests — state machine exhaustively tested
// ---------------------------------------------------------------------------

async fn run_delete(
    job_id: i64,
    db: &sea_orm::DatabaseConnection,
    manager: MockClusterManagerTrait,
) -> (StatusCode, String) {
    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({ "jobId": job_id }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// Tests that deleting a PENDING job transitions it directly to Deleted without a WS message.
///
/// # Setup
/// Inserts a job with a PENDING history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 200 OK, latest history state is Deleted, and no WS message was sent.
#[tokio::test]
async fn test_delete_job_pending_directly_deleted_no_ws() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;

    let (manager, sent) = manager_with_online_cluster();
    let (status, body) = run_delete(job_id, &db, manager).await;

    assert_eq!(status, StatusCode::OK, "body: {body}");

    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job_id))
        .order_by_desc(job_history::Column::Timestamp)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.state, JobStatus::Deleted as i32);
    assert!(sent.lock().unwrap().is_empty());
}

/// Tests that deleting a Cancelled job transitions it to Deleting and sends DELETE_JOB.
///
/// # Setup
/// Inserts a job with a Cancelled history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 200 OK, latest history state is Deleting, and a DELETE_JOB WS message
/// containing the job ID was sent.
#[tokio::test]
async fn test_delete_job_cancelled_sends_delete_ws_message() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Cancelled as i32, "system").await;

    let (manager, sent) = manager_with_online_cluster();
    let (status, _) = run_delete(job_id, &db, manager).await;

    assert_eq!(status, StatusCode::OK);

    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job_id))
        .order_by_desc(job_history::Column::Timestamp)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.state, JobStatus::Deleting as i32);

    let msgs = sent.lock().unwrap();
    assert_eq!(msgs.len(), 1);
    let mut m = Message::from_bytes(msgs[0].clone().into_data());
    assert_eq!(m.id(), DELETE_JOB);
    assert_eq!(m.pop_uint() as i64, job_id);
}

/// Tests that deleting a Completed job sends a DELETE_JOB WS message.
///
/// # Setup
/// Inserts a job with a Completed history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 200 OK and exactly one DELETE_JOB WS message sent.
#[tokio::test]
async fn test_delete_job_completed_sends_delete_ws_message() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Completed as i32, "system").await;

    let (manager, sent) = manager_with_online_cluster();
    let (status, _) = run_delete(job_id, &db, manager).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent.lock().unwrap().len(), 1);
}

/// Tests that deleting an Error-state job sends a DELETE_JOB WS message.
///
/// # Setup
/// Inserts a job with an Error history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 200 OK and exactly one DELETE_JOB WS message sent.
#[tokio::test]
async fn test_delete_job_error_sends_delete_ws_message() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Error as i32, "system").await;

    let (manager, sent) = manager_with_online_cluster();
    let (status, _) = run_delete(job_id, &db, manager).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent.lock().unwrap().len(), 1);
}

// Invalid states for delete
/// Tests that deleting a Submitting job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Submitting history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_delete_job_submitting_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Submitting as i32, "system").await;
    let (status, _) = run_delete(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that deleting a Submitted job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Submitted history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_delete_job_submitted_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Submitted as i32, "system").await;
    let (status, _) = run_delete(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that deleting a Queued job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Queued history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_delete_job_queued_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Queued as i32, "system").await;
    let (status, _) = run_delete(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that deleting a Running job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Running history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_delete_job_running_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Running as i32, "system").await;
    let (status, _) = run_delete(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that deleting a Cancelling job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Cancelling history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_delete_job_cancelling_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Cancelling as i32, "system").await;
    let (status, _) = run_delete(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that deleting a job already in Deleting state returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Deleting history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_delete_job_deleting_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Deleting as i32, "system").await;
    let (status, _) = run_delete(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tests that deleting an already Deleted job returns 400 Bad Request.
///
/// # Setup
/// Inserts a job with a Deleted history entry.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with the job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_delete_job_already_deleted_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Deleted as i32, "system").await;
    let (status, _) = run_delete(job_id, &db, manager_no_clusters()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// get_jobs tests
// ---------------------------------------------------------------------------

/// Tests that GET /job/ returns all jobs belonging to the current application,
/// excluding jobs from other applications.
///
/// # Setup
/// Inserts two jobs for "testapp" and one for "other_app".
///
/// # Act
/// Sends GET /job/apiv1/job/ with a valid token for "testapp".
///
/// # Assert
/// Verifies 200 OK, exactly 2 jobs returned, and the "other_app" job is absent.
#[tokio::test]
async fn test_get_jobs_returns_all_application_jobs() {
    let db = setup_test_db().await;

    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history(&db, job1, JobStatus::Pending as i32, "system").await;

    let job2 = insert_test_job(&db, "nci", "b2", "testapp").await;
    insert_job_history(&db, job2, JobStatus::Completed as i32, "system").await;

    // A job from another application should NOT appear
    let other_job = insert_test_job(&db, "ozstar", "b3", "other_app").await;
    insert_job_history(&db, other_job, JobStatus::Running as i32, "system").await;

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/job/apiv1/job/")
                .header("authorization", &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 2, "should return exactly 2 jobs for testapp");

    let ids: Vec<i64> = jobs.iter().map(|j| j["id"].as_i64().unwrap()).collect();
    assert!(ids.contains(&job1));
    assert!(ids.contains(&job2));
    assert!(!ids.contains(&other_job));
}

/// Tests that the jobIds query parameter filters the result to only the specified job.
///
/// # Setup
/// Inserts two jobs (job1=Pending, job2=Running).
///
/// # Act
/// Sends GET /job/apiv1/job/?jobIds={job1}.
///
/// # Assert
/// Verifies only job1 is returned.
#[tokio::test]
async fn test_get_jobs_with_job_ids_filter() {
    let db = setup_test_db().await;
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history(&db, job1, JobStatus::Pending as i32, "system").await;
    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history(&db, job2, JobStatus::Running as i32, "system").await;

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/job/apiv1/job/?jobIds={job1}"))
                .header("authorization", &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["id"].as_i64().unwrap(), job1);
}

/// Tests that providing both startTimeGt and startTimeLt together returns 400.
///
/// # Setup
/// Empty database.
///
/// # Act
/// Sends GET /job/apiv1/job/?startTimeGt=100&startTimeLt=200.
///
/// # Assert
/// Verifies 400 Bad Request (conflicting filters are rejected).
#[tokio::test]
async fn test_get_jobs_conflicting_time_filters_returns_400() {
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/job/apiv1/job/?startTimeGt=100&startTimeLt=200")
                .header("authorization", &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that the job history array is included in the response, ordered descending by timestamp.
///
/// # Setup
/// Inserts a job with PENDING then SUBMITTING history entries.
///
/// # Act
/// Sends GET /job/apiv1/job/ with a valid token.
///
/// # Assert
/// Verifies the response includes 2 history entries with the most recent (SUBMITTING) first.
#[tokio::test]
async fn test_get_jobs_history_in_response() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job_id, JobStatus::Submitting as i32, "system").await;

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/job/apiv1/job/")
                .header("authorization", &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    let history = jobs[0]["history"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    // History is ordered DESC by timestamp — most recent first
    assert_eq!(
        history[0]["state"].as_i64().unwrap(),
        JobStatus::Submitting as i64
    );
    assert_eq!(
        history[1]["state"].as_i64().unwrap(),
        JobStatus::Pending as i64
    );
}

// ---------------------------------------------------------------------------
// get_jobs filter tests — all filtering happens at database level via subqueries
// ---------------------------------------------------------------------------

use common::insert_job_history_at;

/// Helper: make a UTC timestamp N seconds from epoch (for deterministic test data)
fn ts_secs(secs: i64) -> chrono::NaiveDateTime {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap()
        .naive_utc()
}

async fn get_jobs_with_query(db: sea_orm::DatabaseConnection, query: &str) -> serde_json::Value {
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);
    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/job/apiv1/job/{query}"))
                .header("authorization", &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "query: {query}");
    serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap()
}

/// Tests that startTimeGt includes only jobs whose first SYSTEM_SOURCE entry is after the cutoff.
///
/// # Setup
/// Inserts job1 (PENDING at t=100) and job2 (PENDING at t=1000). Cutoff is 500.
///
/// # Act
/// Sends GET /job/apiv1/job/?startTimeGt=500.
///
/// # Assert
/// Verifies only job2 (started after t=500) is returned.
#[tokio::test]
async fn test_get_jobs_start_time_gt_includes_jobs_started_after_cutoff() {
    let db = setup_test_db().await;
    // job1 started at t=100 (before cutoff=500)  — should NOT appear
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history_at(&db, job1, JobStatus::Pending as i32, "system", ts_secs(100)).await;

    // job2 started at t=2000 (after cutoff=500)  — should appear
    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history_at(
        &db,
        job2,
        JobStatus::Pending as i32,
        "system",
        ts_secs(1000),
    )
    .await;

    let body = get_jobs_with_query(db, "?startTimeGt=500").await;
    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1, "only job2 should pass startTimeGt=500");
    assert_eq!(jobs[0]["id"].as_i64().unwrap(), job2);
}

/// Tests that startTimeLt includes only jobs whose first SYSTEM_SOURCE entry is before the cutoff.
///
/// # Setup
/// Inserts job1 (PENDING at t=100) and job2 (PENDING at t=1000). Cutoff is 500.
///
/// # Act
/// Sends GET /job/apiv1/job/?startTimeLt=500.
///
/// # Assert
/// Verifies only job1 (started before t=500) is returned.
#[tokio::test]
async fn test_get_jobs_start_time_lt_includes_jobs_started_before_cutoff() {
    let db = setup_test_db().await;
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history_at(&db, job1, JobStatus::Pending as i32, "system", ts_secs(100)).await;

    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history_at(
        &db,
        job2,
        JobStatus::Pending as i32,
        "system",
        ts_secs(1000),
    )
    .await;

    let body = get_jobs_with_query(db, "?startTimeLt=500").await;
    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1, "only job1 should pass startTimeLt=500");
    assert_eq!(jobs[0]["id"].as_i64().unwrap(), job1);
}

/// Tests that endTimeGt includes only jobs with a completion entry after the cutoff.
///
/// # Setup
/// Inserts job1 (completed at t=200), job2 (completed at t=800), and job3 (no completion).
/// Cutoff is 500.
///
/// # Act
/// Sends GET /job/apiv1/job/?endTimeGt=500.
///
/// # Assert
/// Verifies only job2 (completed after t=500) is returned.
#[tokio::test]
async fn test_get_jobs_end_time_gt_includes_jobs_completed_after_cutoff() {
    let db = setup_test_db().await;
    // job1 completed at t=200 (before cutoff=500)  — should NOT appear
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history_at(&db, job1, JobStatus::Pending as i32, "system", ts_secs(100)).await;
    insert_job_history_at(
        &db,
        job1,
        JobStatus::Completed as i32,
        "_job_completion_",
        ts_secs(200),
    )
    .await;

    // job2 completed at t=800 (after cutoff=500)  — should appear
    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history_at(&db, job2, JobStatus::Pending as i32, "system", ts_secs(300)).await;
    insert_job_history_at(
        &db,
        job2,
        JobStatus::Completed as i32,
        "_job_completion_",
        ts_secs(800),
    )
    .await;

    // job3 has no completion entry  — should NOT appear
    let job3 = insert_test_job(&db, "ozstar", "b3", "testapp").await;
    insert_job_history_at(&db, job3, JobStatus::Running as i32, "system", ts_secs(400)).await;

    let body = get_jobs_with_query(db, "?endTimeGt=500").await;
    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1, "only job2 should pass endTimeGt=500");
    assert_eq!(jobs[0]["id"].as_i64().unwrap(), job2);
}

/// Tests that endTimeLt includes only jobs with a completion entry before the cutoff.
///
/// # Setup
/// Inserts job1 (completed at t=200) and job2 (completed at t=800). Cutoff is 500.
///
/// # Act
/// Sends GET /job/apiv1/job/?endTimeLt=500.
///
/// # Assert
/// Verifies only job1 (completed before t=500) is returned.
#[tokio::test]
async fn test_get_jobs_end_time_lt_includes_jobs_completed_before_cutoff() {
    let db = setup_test_db().await;
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history_at(&db, job1, JobStatus::Pending as i32, "system", ts_secs(100)).await;
    insert_job_history_at(
        &db,
        job1,
        JobStatus::Completed as i32,
        "_job_completion_",
        ts_secs(200),
    )
    .await;

    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history_at(&db, job2, JobStatus::Pending as i32, "system", ts_secs(300)).await;
    insert_job_history_at(
        &db,
        job2,
        JobStatus::Completed as i32,
        "_job_completion_",
        ts_secs(800),
    )
    .await;

    let body = get_jobs_with_query(db, "?endTimeLt=500").await;
    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1, "only job1 should pass endTimeLt=500");
    assert_eq!(jobs[0]["id"].as_i64().unwrap(), job1);
}

/// Tests that jobSteps filter matches jobs that have a history entry with the given (what, state).
///
/// # Setup
/// Inserts job1 (Pending → Running) and job2 (Pending only).
///
/// # Act
/// Sends GET /job/apiv1/job/?jobSteps=system,{Running}.
///
/// # Assert
/// Verifies only job1 (which has a Running entry) is returned.
#[tokio::test]
async fn test_get_jobs_job_steps_filter_matches_what_and_state() {
    let db = setup_test_db().await;
    // job1: Pending → Running — matches step (system, Running)
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history(&db, job1, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job1, JobStatus::Running as i32, "system").await;

    // job2: Pending only — does NOT match Running
    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history(&db, job2, JobStatus::Pending as i32, "system").await;

    let running_val = JobStatus::Running as u32;
    let body = get_jobs_with_query(db, &format!("?jobSteps=system,{running_val}")).await;
    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1, "only job1 has a Running history entry");
    assert_eq!(jobs[0]["id"].as_i64().unwrap(), job1);
}

/// Tests that multiple jobSteps pairs are OR'd, matching any job with at least one match.
///
/// # Setup
/// Inserts job1 (Completed), job2 (Error), and job3 (Running only).
///
/// # Act
/// Sends GET /job/apiv1/job/?jobSteps=system,{Completed},system,{Error}.
///
/// # Assert
/// Verifies job1 and job2 are returned; job3 (Running) is excluded.
#[tokio::test]
async fn test_get_jobs_job_steps_filter_multiple_steps_matches_any() {
    let db = setup_test_db().await;
    // job1: Completed
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history(&db, job1, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job1, JobStatus::Completed as i32, "system").await;

    // job2: Error
    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history(&db, job2, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job2, JobStatus::Error as i32, "system").await;

    // job3: Running only — not in the step filter
    let job3 = insert_test_job(&db, "ozstar", "b3", "testapp").await;
    insert_job_history(&db, job3, JobStatus::Running as i32, "system").await;

    let completed_val = JobStatus::Completed as u32;
    let error_val = JobStatus::Error as u32;
    let body = get_jobs_with_query(
        db,
        &format!("?jobSteps=system,{completed_val},system,{error_val}"),
    )
    .await;
    let jobs = body.as_array().unwrap();
    assert_eq!(
        jobs.len(),
        2,
        "job1 and job2 both match the multi-step filter"
    );
    let ids: Vec<i64> = jobs.iter().map(|j| j["id"].as_i64().unwrap()).collect();
    assert!(ids.contains(&job1));
    assert!(ids.contains(&job2));
    assert!(!ids.contains(&job3));
}

/// Tests that startTimeGt and jobSteps filters are AND'd together at the DB level.
///
/// # Setup
/// Inserts job1 (Pending at t=100, then Running), job2 (Pending at t=10, then Running),
/// and job3 (Pending at t=200 only).
///
/// # Act
/// Sends GET /job/apiv1/job/?startTimeGt=50&jobSteps=system,{Running}.
///
/// # Assert
/// Verifies only job1 satisfies both conditions (started after t=50 AND has Running history).
#[tokio::test]
async fn test_get_jobs_combined_filters_applied_at_db_level() {
    let db = setup_test_db().await;
    // job1: started at t=100, Running — passes startTimeGt=50 AND jobSteps filter
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history_at(&db, job1, JobStatus::Pending as i32, "system", ts_secs(100)).await;
    insert_job_history(&db, job1, JobStatus::Running as i32, "system").await;

    // job2: started at t=10 (before cutoff) — fails startTimeGt=50
    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history_at(&db, job2, JobStatus::Pending as i32, "system", ts_secs(10)).await;
    insert_job_history(&db, job2, JobStatus::Running as i32, "system").await;

    // job3: started at t=200 but Pending only — fails jobSteps filter
    let job3 = insert_test_job(&db, "ozstar", "b3", "testapp").await;
    insert_job_history_at(&db, job3, JobStatus::Pending as i32, "system", ts_secs(200)).await;

    let running_val = JobStatus::Running as u32;
    let body = get_jobs_with_query(
        db,
        &format!("?startTimeGt=50&jobSteps=system,{running_val}"),
    )
    .await;
    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 1, "only job1 passes both filters");
    assert_eq!(jobs[0]["id"].as_i64().unwrap(), job1);
}

/// Tests that the jobIds filter returns only the explicitly requested job IDs.
///
/// # Setup
/// Inserts three jobs (job1=Pending, job2=Running, job3=Completed).
///
/// # Act
/// Sends GET /job/apiv1/job/?jobIds={job1},{job3}.
///
/// # Assert
/// Verifies job1 and job3 are returned and job2 is excluded.
#[tokio::test]
async fn test_get_jobs_job_ids_filter_excludes_non_matching() {
    let db = setup_test_db().await;
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history(&db, job1, JobStatus::Pending as i32, "system").await;
    let job2 = insert_test_job(&db, "ozstar", "b2", "testapp").await;
    insert_job_history(&db, job2, JobStatus::Running as i32, "system").await;
    let job3 = insert_test_job(&db, "ozstar", "b3", "testapp").await;
    insert_job_history(&db, job3, JobStatus::Completed as i32, "system").await;

    let body = get_jobs_with_query(db, &format!("?jobIds={job1},{job3}")).await;
    let jobs = body.as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    let ids: Vec<i64> = jobs.iter().map(|j| j["id"].as_i64().unwrap()).collect();
    assert!(ids.contains(&job1));
    assert!(!ids.contains(&job2));
    assert!(ids.contains(&job3));
}

/// Tests that startTimeGt uses only SYSTEM_SOURCE history entries, ignoring cluster-source entries.
///
/// # Setup
/// Inserts job1 with a SYSTEM_SOURCE PENDING at t=100 and a cluster-source RUNNING at t=800.
///
/// # Act
/// Sends GET /job/apiv1/job/?startTimeGt=500 (job1's system entry is at t=100, before cutoff),
/// then GET /job/apiv1/job/?startTimeGt=50 (system entry at t=100 is after this cutoff).
///
/// # Assert
/// Verifies the first query returns 0 jobs and the second returns 1 job.
#[tokio::test]
async fn test_get_jobs_start_time_gt_only_uses_system_source_entries() {
    let db = setup_test_db().await;
    // The filter should use MIN of SYSTEM_SOURCE entries (t=100), not cluster entries
    let job1 = insert_test_job(&db, "ozstar", "b1", "testapp").await;
    insert_job_history_at(&db, job1, JobStatus::Pending as i32, "system", ts_secs(100)).await;
    insert_job_history_at(&db, job1, JobStatus::Running as i32, "ozstar", ts_secs(800)).await;

    // With cutoff=500: MIN(system entries) = t=100 < 500, so job1 should NOT appear
    let body = get_jobs_with_query(db.clone(), "?startTimeGt=500").await;
    assert_eq!(
        body.as_array().unwrap().len(),
        0,
        "job1 system source is before cutoff"
    );

    // With cutoff=50: MIN(system entries) = t=100 > 50, so job1 SHOULD appear
    let body2 = get_jobs_with_query(db, "?startTimeGt=50").await;
    assert_eq!(
        body2.as_array().unwrap().len(),
        1,
        "job1 system source is after cutoff=50"
    );
}
