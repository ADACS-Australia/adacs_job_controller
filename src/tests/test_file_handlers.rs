//! Comprehensive tests for file HTTP handlers.
//!
//! Tests cover the full business logic for:
//! - POST /job/apiv1/file/  (create download record)
//! - GET  /job/apiv1/file/  (stream file download — WS→HTTP flow)
//! - PUT  /job/apiv1/file/upload/ (file upload — HTTP→WS→HTTP flow)
//! - PATCH /job/apiv1/file/ (list files — WS→HTTP with cache)

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rand::{Rng, SeedableRng};
use tower::ServiceExt;

use adacs_job_controller::cluster::file_download::{DownloadSession, FileDownloadState};
use adacs_job_controller::cluster::file_upload::FileUploadState;
use adacs_job_controller::cluster::traits::{MockClusterManagerTrait, MockClusterTrait};
use adacs_job_controller::db::entities::{file_download, file_list_cache};
use adacs_job_controller::http::server::create_router;
use adacs_job_controller::protocol::types::{ClusterRole, FileInfo, FileListState};

use common::{
    encode_jwt_for_secret, encode_test_jwt, insert_job_history, insert_test_job,
    insert_test_job_with_id, make_test_state, make_test_state_with_secrets,
    manager_with_online_cluster_no_messages, offline_cluster, online_cluster,
    online_cluster_no_messages, setup_test_db, test_cluster_config, test_jwt_secrets,
    test_jwt_secrets_multi, upload_cluster,
};

use adacs_job_controller::protocol::types::JobStatus;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use std::sync::atomic::Ordering;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// POST /job/apiv1/file/ — create file download records
// ---------------------------------------------------------------------------

/// Tests that POST /file/ with a single path creates a download record and returns a fileId.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"jobId": ..., "path": "/result/output.txt"}`.
///
/// # Assert
/// Verifies 200 OK, non-empty `fileId`, and the DB record has the correct path, cluster,
/// bundle, and job ID.
#[tokio::test]
async fn test_create_file_download_single_path_returns_file_id() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let manager = manager_with_online_cluster_no_messages();

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "path": "/result/output.txt"
                    })
                    .to_string(),
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

    let file_id = body["fileId"].as_str().expect("fileId should be present");
    assert!(uuid::Uuid::parse_str(file_id).is_ok());

    // Verify the record is in the DB
    let record = file_download::Entity::find()
        .filter(file_download::Column::Uuid.eq(file_id))
        .one(&db)
        .await
        .unwrap()
        .expect("file download record should be in DB");

    assert_eq!(record.path, "/result/output.txt");
    assert_eq!(record.cluster, "ozstar");
    assert_eq!(record.bundle, "b");
    assert_eq!(record.job, job_id);
}

/// Tests that POST /file/ with a paths array creates multiple download records.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"jobId": ..., "paths": ["/a.txt", "/b.txt", "/c.txt"]}`.
///
/// # Assert
/// Verifies 200 OK and a `fileIds` array containing 3 non-empty UUID strings.
#[tokio::test]
async fn test_create_file_download_multiple_paths_returns_file_ids() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    let manager = manager_with_online_cluster_no_messages();
    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "paths": ["/a.txt", "/b.txt", "/c.txt"]
                    })
                    .to_string(),
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

    let file_ids = body["fileIds"].as_array().expect("fileIds should be array");
    assert_eq!(file_ids.len(), 3);
    for id in file_ids {
        assert!(uuid::Uuid::parse_str(id.as_str().unwrap_or("")).is_ok());
    }
}

/// Tests that POST /file/ without a path or paths field returns 400.
///
/// # Setup
/// Inserts a test job.
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"jobId": ...}` (no path key).
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_file_download_no_path_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_get_cluster_by_name()
        .returning(|_| Some(Arc::new(online_cluster_no_messages())));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
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

/// Tests that POST /file/ rejects empty or whitespace-only paths instead of
/// creating useless file-download records with an empty path.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"jobId": ..., "path": ""}` and with
/// `{"jobId": ..., "paths": ["", "   ", "/valid.txt"]}`.
///
/// # Assert
/// Verifies 200 OK with an empty `fileIds` array for the all-empty request, and
/// that only the non-empty path yields a download record (no empty-path rows).
#[tokio::test]
async fn test_create_file_download_rejects_empty_paths() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    // Single empty path — must not create a record.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({ "jobId": job_id, "path": "" }).to_string(),
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
    assert!(
        body["fileIds"]
            .as_array()
            .expect("fileIds should be array")
            .is_empty(),
        "empty path must not create a download record"
    );

    // Mixed list — empty/whitespace entries are filtered out, valid path kept.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "paths": ["", "   ", "/valid.txt"]
                    })
                    .to_string(),
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
    let file_ids = body["fileIds"].as_array().expect("fileIds should be array");
    assert_eq!(file_ids.len(), 1, "only the non-empty path should be kept");

    let records = file_download::Entity::find().all(&db).await.unwrap();
    assert_eq!(records.len(), 1, "no empty-path records should exist");
    assert_eq!(records[0].path, "/valid.txt");
}

// ---------------------------------------------------------------------------
// GET /job/apiv1/file/ — stream file download (WS→HTTP data flow)
// ---------------------------------------------------------------------------

/// Tests that GET /file/ without a fileId query parameter returns 400.
///
/// # Setup
/// Wires a manager that returns nothing.
///
/// # Act
/// Sends GET /job/apiv1/file/ with no `fileId` query parameter.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_download_file_no_file_id_returns_400() {
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);
    manager.expect_get_file_download().returning(|_| None);

    let app = create_router(make_test_state(db, manager));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/job/apiv1/file/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that GET /file/ with an unknown UUID returns 400.
///
/// # Setup
/// Empty DB (no download records). Wires a manager that returns nothing.
///
/// # Act
/// Sends GET /job/apiv1/file/?fileId=not-a-real-uuid.
///
/// # Assert
/// Verifies 400 Bad Request because the UUID is not found in the DB.
#[tokio::test]
async fn test_download_file_unknown_uuid_returns_400() {
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);
    manager.expect_get_file_download().returning(|_| None);
    let app = create_router(make_test_state(db, manager));
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/job/apiv1/file/?fileId=not-a-real-uuid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should be 400 — UUID not found in DB
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that GET /file/ when the cluster is offline returns 503.
///
/// # Setup
/// Inserts a download record pointing at "ozstar". Wires an offline cluster.
///
/// # Act
/// Sends GET /job/apiv1/file/?fileId={uuid}.
///
/// # Assert
/// Verifies 503 Service Unavailable.
#[tokio::test]
async fn test_download_file_cluster_offline_returns_503() {
    let db = setup_test_db().await;
    // Insert a file download record pointing at "ozstar"
    let uuid = "test-uuid-1234".to_string();
    file_download::ActiveModel {
        user: Set(1),
        job: Set(0),
        cluster: Set("ozstar".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(uuid.clone()),
        path: Set(String::new()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let cluster = Arc::new(offline_cluster());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    manager.expect_get_file_download().returning(|_| None);

    let app = create_router(make_test_state(db, manager));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/job/apiv1/file/?fileId={uuid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// Tests the full file download flow: WS pushes data, HTTP streams it back.
///
/// # Setup
/// Inserts 5 download records (one for each repeated download). For each download,
/// a background task simulates `FILE_DETAILS` + chunks arriving via a fresh `FileDownloadState`.
///
/// # Act
/// Sends GET /job/apiv1/file/?fileId={uuid} **5 times** with different UUIDs (repeated downloads).
///
/// # Assert
/// Verifies 200 OK with correct `Content-Length`, `Content-Type: application/octet-stream`,
/// and the exact chunk bytes in the response body for **each of the 5 downloads**,
/// matching the C++ `test_file_transfer` behavior with `BOOST_CHECK_EQUAL_COLLECTIONS`.
#[tokio::test]
async fn test_download_file_streams_chunks() {
    let db = setup_test_db().await;
    // Generate random file data like the C++ test (0 to 1MB range)
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let file_size = rng.random_range(0..=1024 * 1024);
    let expected_data: Vec<u8> = (0..file_size).map(|_| rng.random()).collect();

    // Insert 5 download records for 5 repeated downloads
    let mut uuids = Vec::new();
    for i in 0..5 {
        let uuid = format!("download-uuid-{i}");
        file_download::ActiveModel {
            user: Set(1),
            job: Set(0),
            cluster: Set("ozstar".to_string()),
            bundle: Set("b".to_string()),
            uuid: Set(uuid.clone()),
            path: Set(String::new()),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        uuids.push(uuid);
    }

    // Set up mock manager that creates a fresh FileDownloadState for each download
    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_is_application_shutting_down()
        .returning(|| false);
    manager.expect_begin_application_shutdown().returning(|| 0);
    manager
        .expect_dedicated_download_clusters()
        .returning(Vec::new);
    manager
        .expect_get_file_download_cleanup_trigger()
        .returning(|_| None);
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .times(5)
        .returning(move |_| Some(c.clone()));

    let c2 = Arc::new(online_cluster_no_messages());
    manager
        .expect_create_file_download()
        .times(5)
        .returning(move |_, _| {
            let c = Arc::clone(&c2);
            Box::pin(
                async move { c as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait> },
            )
        });

    // For each get_file_download call, create a fresh FileDownloadState and spawn a task to push data
    let expected_data_for_mock = expected_data.clone();
    let call_count = Arc::new(std::sync::Mutex::new(0));
    manager
        .expect_get_file_download()
        .times(5)
        .returning(move |_| {
            // Create a fresh FileDownloadState for this download
            let fd_state = Arc::new(FileDownloadState::new());
            let fd_state_sim = Arc::clone(&fd_state);
            let data_copy = expected_data_for_mock.clone();

            // Track call count to ensure unique state per call
            {
                let mut count = call_count.lock().unwrap();
                *count += 1;
            }

            tokio::spawn(async move {
                // Brief delay so the HTTP handler starts waiting first
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                fd_state_sim
                    .file_size
                    .store(data_copy.len() as u64, Ordering::Release);
                fd_state_sim.received_data.store(true, Ordering::Release);
                fd_state_sim.data_ready.store(true, Ordering::Release);
                fd_state_sim.data_notify.notify_waiters();

                // Push the data as a single chunk
                let _ = fd_state_sim.chunk_sender.send(data_copy);
            });

            Some(fd_state)
        });

    let app = create_router(make_test_state(db, manager));

    // Perform 5 repeated downloads like the C++ test
    for (i, uuid) in uuids.iter().enumerate().take(5) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/job/apiv1/file/?fileId={uuid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Download iteration {} failed",
            i + 1
        );
        assert_eq!(
            resp.headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok()),
            Some(file_size.to_string().as_str()),
            "Content-Length mismatch on download {}",
            i + 1
        );
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/octet-stream"),
            "Content-Type mismatch on download {}",
            i + 1
        );

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();

        // Verify data integrity - equivalent to BOOST_CHECK_EQUAL_COLLECTIONS
        assert_eq!(
            body_bytes.as_ref(),
            expected_data.as_slice(),
            "Data integrity check failed on download {} (expected {} bytes, got {})",
            i + 1,
            expected_data.len(),
            body_bytes.len()
        );
    }
}

/// Tests that a cluster file error propagates to a 400 response with the error message.
///
/// # Setup
/// Inserts a download record. A background task sets the error flag and error details
/// in `FileDownloadState` after a short delay.
///
/// # Act
/// Sends GET /job/apiv1/file/?fileId={uuid}.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "File not found".
#[tokio::test]
async fn test_download_file_error_from_cluster_returns_400() {
    let db = setup_test_db().await;
    let uuid = "error-uuid".to_string();
    file_download::ActiveModel {
        user: Set(1),
        job: Set(0),
        cluster: Set("ozstar".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(uuid.clone()),
        path: Set("/file.txt".to_string()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let fd_state = Arc::new(FileDownloadState::new());
    let fd_sim = Arc::clone(&fd_state);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        *fd_sim.error_details.lock().await = "File not found on cluster".to_string();
        fd_sim.error.store(true, Ordering::Release);
        fd_sim.data_ready.store(true, Ordering::Release);
        fd_sim.data_notify.notify_waiters();
    });

    let fd_for_manager = Arc::clone(&fd_state);
    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_is_application_shutting_down()
        .returning(|| false);
    manager.expect_begin_application_shutdown().returning(|| 0);
    manager
        .expect_dedicated_download_clusters()
        .returning(Vec::new);
    manager
        .expect_get_file_download_cleanup_trigger()
        .returning(|_| None);
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    let c2 = Arc::new(online_cluster_no_messages());
    manager
        .expect_create_file_download()
        .returning(move |_, _| {
            let c = Arc::clone(&c2);
            Box::pin(
                async move { c as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait> },
            )
        });
    manager
        .expect_get_file_download()
        .returning(move |_| Some(Arc::clone(&fd_for_manager)));

    let app = create_router(make_test_state(db, manager));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/job/apiv1/file/?fileId={uuid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("File not found"),
        "body: {}",
        String::from_utf8_lossy(&body)
    );
}

/// Verifies 400 Bad Request when the download record's job ID exceeds `u32::MAX`.
///
/// # Setup
/// Inserts a download record whose `job` column exceeds `u32::MAX`. Wires an
/// online cluster and a real cleanup trigger so the pre-response guard's
/// `Some` path (`fire_guard`) is exercised.
///
/// # Act
/// Sends GET /job/apiv1/file/?fileId={uuid}.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "exceeds maximum supported value".
#[tokio::test]
async fn test_download_file_job_id_exceeding_u32_returns_400() {
    let db = setup_test_db().await;
    let uuid = "huge-job-uuid".to_string();
    let huge: i64 = i64::from(u32::MAX) + 1;
    file_download::ActiveModel {
        user: Set(1),
        job: Set(huge),
        cluster: Set("ozstar".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(uuid.clone()),
        path: Set(String::new()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_is_application_shutting_down()
        .returning(|| false);
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    let c2 = Arc::new(online_cluster_no_messages());
    manager
        .expect_create_file_download()
        .returning(move |_, _| {
            let c = Arc::clone(&c2);
            Box::pin(
                async move { c as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait> },
            )
        });
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let session = DownloadSession::new(uuid.clone(), Arc::new(FileDownloadState::new()), tx);
    let trigger = session.cleanup_trigger();
    let trigger_for_mock = trigger.clone();
    manager
        .expect_get_file_download_cleanup_trigger()
        .returning(move |_| Some(trigger_for_mock.clone()));

    let app = create_router(make_test_state(db, manager));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/job/apiv1/file/?fileId={uuid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("exceeds maximum supported value"),
        "body: {}",
        String::from_utf8_lossy(&body)
    );
}

/// Verifies 400 Bad Request when the download session is missing after the
/// download record resolves (cluster online, session not found).
///
/// # Setup
/// Inserts a download record. Wires an online cluster, a real cleanup trigger,
/// and a manager whose `get_file_download` returns `None`.
///
/// # Act
/// Sends GET /job/apiv1/file/?fileId={uuid}.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "File download session not found".
#[tokio::test]
async fn test_download_file_session_not_found_returns_400() {
    let db = setup_test_db().await;
    let uuid = "missing-session-uuid".to_string();
    file_download::ActiveModel {
        user: Set(1),
        job: Set(0),
        cluster: Set("ozstar".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(uuid.clone()),
        path: Set(String::new()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_is_application_shutting_down()
        .returning(|| false);
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    let c2 = Arc::new(online_cluster_no_messages());
    manager
        .expect_create_file_download()
        .returning(move |_, _| {
            let c = Arc::clone(&c2);
            Box::pin(
                async move { c as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait> },
            )
        });
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let session = DownloadSession::new(uuid.clone(), Arc::new(FileDownloadState::new()), tx);
    let trigger = session.cleanup_trigger();
    let trigger_for_mock = trigger.clone();
    manager
        .expect_get_file_download_cleanup_trigger()
        .returning(move |_| Some(trigger_for_mock.clone()));
    manager.expect_get_file_download().returning(|_| None);

    let app = create_router(make_test_state(db, manager));

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/job/apiv1/file/?fileId={uuid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("File download session not found"),
        "body: {}",
        String::from_utf8_lossy(&body)
    );
}

// ---------------------------------------------------------------------------
// PATCH /job/apiv1/file/ — list files (cache hit and miss)
// ---------------------------------------------------------------------------

/// Tests that PATCH /file/ returns cached files from DB when the job is complete.
///
/// # Setup
/// Inserts a completed job and pre-populates the `file_list_cache` table with 2 entries.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with the job ID.
///
/// # Assert
/// Verifies 200 OK with a `files` array containing the 2 cached entries,
/// and no WS `FILE_LIST` message is sent.
#[tokio::test]
async fn test_list_files_cache_hit_returns_cached_files() {
    let db = setup_test_db().await;

    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    // Mark as complete
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job_id, JobStatus::Completed as i32, "_job_completion_").await;

    // Pre-populate cache
    for (name, is_dir) in [("/out/results.txt", false), ("/out/", true)] {
        file_list_cache::ActiveModel {
            job_id: Set(job_id),
            path: Set(name.to_string()),
            is_dir: Set(is_dir),
            file_size: Set(1024),
            permissions: Set(0o644),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // Cluster manager should NOT be asked to send a FILE_LIST message
    let manager = manager_with_online_cluster_no_messages();
    // send_message should NOT be called (already mocked with .returning in cluster)

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "path": "",
                        "recursive": true
                    })
                    .to_string(),
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

    let files = body["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "should return 2 cached files");
}

/// Tests the WS-driven file list flow: cluster receives `FILE_LIST`, populates state, HTTP returns result.
///
/// # Setup
/// Inserts a Running job (no cache). Wires a cluster whose `send_message` mock intercepts
/// the `FILE_LIST` message, parses the UUID, and populates the `file_list_map` entry.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with the job ID.
///
/// # Assert
/// Verifies 200 OK with a `files` array containing "/output/result.txt".
#[tokio::test]
async fn test_list_files_ws_response_populates_result() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    // Job is NOT complete — no caching path
    insert_job_history(&db, job_id, JobStatus::Running as i32, "system").await;

    // The fl_state will be populated by a background task simulating the WS handler
    let file_list_map: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<FileListState>>>> =
        Arc::new(dashmap::DashMap::new());
    let file_list_map_clone = Arc::clone(&file_list_map);

    let cluster = {
        let flm = Arc::clone(&file_list_map_clone);
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(move |msg| {
            // Parse the UUID from the FILE_LIST message, then signal it
            let mut m =
                adacs_job_controller::protocol::message::Message::from_bytes(msg.into_data());
            let _job_id = m.pop_uint();
            let uuid = m.pop_string();

            let flm2 = Arc::clone(&flm);
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                if let Some(state_arc) = flm2.get(&uuid) {
                    let mut locked = state_arc.lock().await;
                    locked.files = vec![FileInfo {
                        file_name: "/output/result.txt".to_string(),
                        file_size: 512,
                        permissions: 0o644,
                        is_directory: false,
                    }];
                    locked.data_ready = true;
                    locked.notify.notify_waiters();
                }
            });
            Box::pin(async {})
        });
        Arc::new(c)
    };

    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    // Build AppState with the shared file_list_map
    let state = adacs_job_controller::app::AppState {
        db: db.clone(),
        cluster_manager: Arc::new(manager),
        file_list_map,
        jwt_secrets: std::sync::Arc::new(test_jwt_secrets()),
        client_timeout_seconds: None,
    };

    let app = create_router(state);
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "path": "",
                        "recursive": true
                    })
                    .to_string(),
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

    let files = body["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"].as_str().unwrap(), "/output/result.txt");
}

/// Tests that a root recursive listing of a completed job populates the `file_list_cache`
/// with the files returned by the cluster.
///
/// # Setup
/// Inserts a completed job (no pre-existing cache). Wires a cluster whose `send_message`
/// mock inserts a stale cache row while the request is in flight, then populates the
/// `FileListState` with a single fresh file.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with `{"jobId": ..., "path": "", "recursive": true}`.
///
/// # Assert
/// Verifies 200 OK with the file in the response, and that the cache table now contains
/// exactly that file — the stale row is deleted before the fresh rows are inserted, so
/// no duplicate cache rows remain.
#[tokio::test]
async fn test_list_files_completed_job_populates_cache() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    // Mark as complete
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job_id, JobStatus::Completed as i32, "_job_completion_").await;

    let file_list_map: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<FileListState>>>> =
        Arc::new(dashmap::DashMap::new());
    let file_list_map_clone = Arc::clone(&file_list_map);

    let db_for_mock = db.clone();
    let job_id_for_mock = job_id;
    let cluster = {
        let flm = Arc::clone(&file_list_map_clone);
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(move |msg| {
            let mut m =
                adacs_job_controller::protocol::message::Message::from_bytes(msg.into_data());
            let _job_id = m.pop_uint();
            let uuid = m.pop_string();

            let flm2 = Arc::clone(&flm);
            let db2 = db_for_mock.clone();
            let jid = job_id_for_mock;
            tokio::spawn(async move {
                // Simulate a stale cache row appearing while the request is in flight
                let _ = file_list_cache::ActiveModel {
                    job_id: Set(jid),
                    path: Set("/stale/old.txt".to_string()),
                    is_dir: Set(false),
                    file_size: Set(1024),
                    permissions: Set(0o644),
                    ..Default::default()
                }
                .insert(&db2)
                .await;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                if let Some(state_arc) = flm2.get(&uuid) {
                    let mut locked = state_arc.lock().await;
                    locked.files = vec![FileInfo {
                        file_name: "/output/result.txt".to_string(),
                        file_size: 512,
                        permissions: 0o644,
                        is_directory: false,
                    }];
                    locked.data_ready = true;
                    locked.notify.notify_waiters();
                }
            });
            Box::pin(async {})
        });
        Arc::new(c)
    };

    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let state = adacs_job_controller::app::AppState {
        db: db.clone(),
        cluster_manager: Arc::new(manager),
        file_list_map,
        jwt_secrets: std::sync::Arc::new(test_jwt_secrets()),
        client_timeout_seconds: None,
    };

    let app = create_router(state);
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "path": "",
                        "recursive": true
                    })
                    .to_string(),
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

    let files = body["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"].as_str().unwrap(), "/output/result.txt");

    // Cache should contain exactly the fresh file — the stale row was deleted first
    let cached = file_list_cache::Entity::find()
        .filter(file_list_cache::Column::JobId.eq(job_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(
        cached.len(),
        1,
        "stale row should be deleted, no duplicate rows"
    );
    assert_eq!(cached[0].path, "/output/result.txt");
}

/// Tests that `spawn_background_cache` replaces (rather than duplicates) existing
/// `file_list_cache` rows for a completed job.
///
/// # Setup
/// Inserts a completed job and pre-populates the `file_list_cache` table with 2 stale
/// entries for that job. Wires a cluster whose `send_message` mock populates the
/// `FileListState` with a single fresh file.
///
/// # Act
/// Calls `spawn_background_cache` directly.
///
/// # Assert
/// Verifies the cache now contains exactly the fresh file — the stale rows are deleted
/// before the new rows are inserted, so no duplicate cache rows remain.
#[tokio::test]
async fn test_spawn_background_cache_replaces_stale_rows() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    // Pre-seed stale cache rows for the job
    for (name, is_dir) in [("/stale/old.txt", false), ("/stale/", true)] {
        file_list_cache::ActiveModel {
            job_id: Set(job_id),
            path: Set(name.to_string()),
            is_dir: Set(is_dir),
            file_size: Set(1024),
            permissions: Set(0o644),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let file_list_map: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<FileListState>>>> =
        Arc::new(dashmap::DashMap::new());
    let file_list_map_clone = Arc::clone(&file_list_map);

    let cluster = {
        let flm = Arc::clone(&file_list_map_clone);
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(move |msg| {
            let mut m =
                adacs_job_controller::protocol::message::Message::from_bytes(msg.into_data());
            let _job_id = m.pop_uint();
            let uuid = m.pop_string();

            let flm2 = Arc::clone(&flm);
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                if let Some(state_arc) = flm2.get(&uuid) {
                    let mut locked = state_arc.lock().await;
                    locked.files = vec![FileInfo {
                        file_name: "/output/result.txt".to_string(),
                        file_size: 512,
                        permissions: 0o644,
                        is_directory: false,
                    }];
                    locked.data_ready = true;
                    locked.notify.notify_waiters();
                }
            });
            Box::pin(async {})
        });
        Arc::new(c)
    };

    let state = adacs_job_controller::app::AppState {
        db: db.clone(),
        cluster_manager: Arc::new(MockClusterManagerTrait::new()),
        file_list_map,
        jwt_secrets: std::sync::Arc::new(test_jwt_secrets()),
        client_timeout_seconds: None,
    };

    adacs_job_controller::http::file::spawn_background_cache(
        state,
        cluster,
        "b".to_string(),
        job_id as u64,
    )
    .await
    .unwrap();

    // Stale rows must be gone; only the fresh file remains (no duplicates)
    let cached = file_list_cache::Entity::find()
        .filter(file_list_cache::Column::JobId.eq(job_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(
        cached.len(),
        1,
        "stale cache rows should be replaced, not duplicated"
    );
    assert_eq!(cached[0].path, "/output/result.txt");
}

/// Tests that `spawn_background_cache` preserves existing `file_list_cache` rows when the
/// remote cluster fails to respond before the client timeout.
///
/// # Setup
/// Inserts a completed job and pre-populates the `file_list_cache` table with 2 entries.
/// Wires a cluster whose `send_message` mock never populates the `FileListState`, so the
/// request can only end via the client timeout.
///
/// # Act
/// Calls `spawn_background_cache` directly with `client_timeout_seconds = Some(1)`.
///
/// # Assert
/// Verifies the pre-seeded cache rows are still present — the timed-out request must not
/// delete them (previously the timeout left `error = false`, wiping a valid cache).
#[tokio::test]
async fn test_spawn_background_cache_timeout_preserves_cache() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    // Pre-seed valid cache rows for the job
    for (name, is_dir) in [("/out/results.txt", false), ("/out/", true)] {
        file_list_cache::ActiveModel {
            job_id: Set(job_id),
            path: Set(name.to_string()),
            is_dir: Set(is_dir),
            file_size: Set(1024),
            permissions: Set(0o644),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // Non-responding cluster: send_message accepts the message but never populates state
    let cluster = {
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(|_| Box::pin(async {}));
        Arc::new(c)
    };

    let state = adacs_job_controller::app::AppState {
        db: db.clone(),
        cluster_manager: Arc::new(MockClusterManagerTrait::new()),
        file_list_map: Arc::new(dashmap::DashMap::new()),
        jwt_secrets: std::sync::Arc::new(test_jwt_secrets()),
        client_timeout_seconds: Some(1),
    };

    adacs_job_controller::http::file::spawn_background_cache(
        state,
        cluster,
        "b".to_string(),
        job_id as u64,
    )
    .await
    .unwrap();

    // Pre-seeded rows must survive the timed-out request
    let cached = file_list_cache::Entity::find()
        .filter(file_list_cache::Column::JobId.eq(job_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(
        cached.len(),
        2,
        "timed-out background cache request must not wipe existing cache rows"
    );
}

/// Tests that `spawn_background_cache` returns `Err("Cluster offline")` when the
/// cluster is offline, covering the `is_online()` early-return path.
///
/// # Setup
/// Wires an offline cluster via the `offline_cluster()` helper. No job is needed
/// because the function returns before touching the database.
///
/// # Act
/// Calls `spawn_background_cache` directly.
///
/// # Assert
/// Verifies the returned error is exactly "Cluster offline" and that no cache rows
/// are written.
#[tokio::test]
async fn test_spawn_background_cache_offline_cluster_returns_error() {
    let db = setup_test_db().await;
    let cluster = Arc::new(offline_cluster());

    let state = adacs_job_controller::app::AppState {
        db: db.clone(),
        cluster_manager: Arc::new(MockClusterManagerTrait::new()),
        file_list_map: Arc::new(dashmap::DashMap::new()),
        jwt_secrets: std::sync::Arc::new(test_jwt_secrets()),
        client_timeout_seconds: None,
    };

    let err = adacs_job_controller::http::file::spawn_background_cache(
        state,
        cluster,
        "b".to_string(),
        1,
    )
    .await
    .unwrap_err();

    assert_eq!(err, "Cluster offline");

    // No cache rows should have been written for the offline path
    let cached = file_list_cache::Entity::find().all(&db).await.unwrap();
    assert!(cached.is_empty(), "offline path must not write cache rows");
}

/// Tests that PATCH /file/ returns 503 when the cluster is offline.
///
/// # Setup
/// Inserts a Running job. Wires an offline cluster.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with the job ID.
///
/// # Assert
/// Verifies 503 Service Unavailable.
#[tokio::test]
async fn test_list_files_cluster_offline_returns_503() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Running as i32, "system").await;
    let cluster = Arc::new(offline_cluster());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "path": "",
                        "recursive": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// Tests that PATCH /file/ for a job whose ID exceeds `u32::MAX` returns 400
/// instead of silently truncating the job ID in the `FILE_LIST` message.
///
/// # Setup
/// Inserts a job with ID = `u32::MAX + 1`. Wires an online cluster.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with `{"jobId": {huge}, "path": "", "recursive": true}`.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "exceeds maximum supported value".
#[tokio::test]
async fn test_list_files_job_id_exceeding_u32_returns_400() {
    let db = setup_test_db().await;
    let huge: i64 = i64::from(u32::MAX) + 1;
    insert_test_job_with_id(&db, huge, "ozstar", "b", "testapp").await;

    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_get_cluster_by_name()
        .returning(|_| Some(Arc::new(online_cluster_no_messages())));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": huge,
                        "path": "",
                        "recursive": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("exceeds maximum supported value"),
        "body: {}",
        String::from_utf8_lossy(&body)
    );
}

/// Tests that PATCH /file/ without a jobId and missing cluster + bundle returns 400.
///
/// # Setup
/// Empty DB. Wires an online cluster.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with only `{"path": "", "recursive": false}` (no jobId, no cluster).
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_list_files_no_job_id_requires_cluster_and_bundle() {
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_get_cluster_by_name()
        .returning(|_| Some(Arc::new(online_cluster_no_messages())));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    // Missing both cluster and bundle — should be 400
    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({ "path": "", "recursive": false }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that PATCH /file/ without a jobId but with a forbidden cluster returns 400.
///
/// # Setup
/// Empty DB. Wires an online cluster.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with `{"cluster": "forbidden_cluster", "bundle": "b"}`
/// for a token that does not allow that cluster.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "does not have access".
#[tokio::test]
async fn test_list_files_no_job_id_wrong_cluster_access_returns_400() {
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_get_cluster_by_name()
        .returning(|_| Some(Arc::new(online_cluster_no_messages())));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "path": "",
                        "recursive": false,
                        "cluster": "forbidden_cluster",
                        "bundle": "b"
                    })
                    .to_string(),
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
// PUT /job/apiv1/file/upload/ — file upload (HTTP→WS→HTTP flow)
// ---------------------------------------------------------------------------

/// Tests that PUT /file/upload/ without a targetPath query parameter returns 400.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends PUT /job/apiv1/file/upload/ without the `targetPath` parameter.
///
/// # Assert
/// Verifies 400 Bad Request with body mentioning "targetPath".
#[tokio::test]
async fn test_upload_file_no_target_path_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_get_cluster_by_name()
        .returning(|_| Some(Arc::new(online_cluster_no_messages())));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/job/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b"
                ))
                .header("authorization", &token)
                .header("content-length", "10")
                .body(Body::from("0123456789"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("targetPath"));
}

/// Tests that PUT /file/upload/ for a job whose ID exceeds `u32::MAX` returns 400
/// instead of silently truncating the job ID in the `UPLOAD_FILE` message.
///
/// # Setup
/// Inserts a job with ID = `u32::MAX + 1`. Wires an online cluster and a file-upload cluster.
///
/// # Act
/// Sends PUT /job/apiv1/file/upload/?jobId={huge}&targetPath=/dest.txt.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "exceeds maximum supported value".
#[tokio::test]
async fn test_upload_file_job_id_exceeding_u32_returns_400() {
    let db = setup_test_db().await;
    let huge: i64 = i64::from(u32::MAX) + 1;
    insert_test_job_with_id(&db, huge, "ozstar", "b", "testapp").await;

    let cluster_main = Arc::new(online_cluster_no_messages());
    let upload_cluster = {
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar-up".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(|_| Box::pin(async {}));
        Arc::new(c)
    };

    let uc = Arc::clone(&upload_cluster);
    let mut manager = MockClusterManagerTrait::new();
    let cm = Arc::clone(&cluster_main);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(cm.clone()));
    manager.expect_create_file_upload().returning(move |_, _| {
        let c = Arc::clone(&uc);
        Box::pin(async move { c as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait> })
    });

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/job/apiv1/file/upload/?jobId={huge}&targetPath=/dest.txt"
                ))
                .header("authorization", &token)
                .header("content-length", "5")
                .body(Body::from("hello"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("exceeds maximum supported value"),
        "body: {}",
        String::from_utf8_lossy(&body)
    );
}

/// Tests that PUT /file/upload/ when the cluster is offline returns 503.
///
/// # Setup
/// Inserts a test job. Wires an offline cluster.
///
/// # Act
/// Sends PUT /job/apiv1/file/upload/ with valid parameters.
///
/// # Assert
/// Verifies 503 Service Unavailable.
#[tokio::test]
async fn test_upload_file_cluster_offline_returns_503() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    let cluster = Arc::new(offline_cluster());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/job/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b&targetPath=/dest.txt"
                ))
                .header("authorization", &token)
                .header("content-length", "5")
                .body(Body::from("hello"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// Tests the full file upload flow: `SERVER_READY` then `FILE_UPLOAD_COMPLETE` signals lead to success.
///
/// # Setup
/// Inserts a test job. A background task simulates `SERVER_READY` then `FILE_UPLOAD_COMPLETE`
/// arriving in the `FileUploadState`. Cluster captures sent WS messages.
///
/// # Act
/// Sends PUT /job/apiv1/file/upload/ with a 17-byte body.
///
/// # Assert
/// Verifies 200 OK, body contains `status: "completed"` and a non-null `uploadId`.
#[tokio::test]
async fn test_upload_file_success_full_flow() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    let fu_state = Arc::new(FileUploadState::new());
    let fu_sim = Arc::clone(&fu_state);
    tokio::spawn(async move {
        // Simulate SERVER_READY arriving from the cluster
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        fu_sim.data_ready.store(true, Ordering::Release);
        fu_sim.data_notify.notify_waiters();

        // Simulate FILE_UPLOAD_COMPLETE arriving
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        fu_sim.complete.store(true, Ordering::Release);
        fu_sim.data_notify.notify_waiters();
    });

    let fu_for_manager = Arc::clone(&fu_state);

    let sent_msgs = Arc::new(Mutex::new(vec![]));
    let sent_clone = Arc::clone(&sent_msgs);
    let cluster_main = {
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(move |msg| {
            sent_clone.lock().unwrap().push(msg);
            Box::pin(async {})
        });
        Arc::new(c)
    };

    let upload_cluster = {
        let sent2 = Arc::clone(&sent_msgs);
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar-upload".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(move |msg| {
            sent2.lock().unwrap().push(msg);
            Box::pin(async {})
        });
        c.expect_wait_for_queue_drain()
            .returning(|_| Box::pin(async { true }));
        Arc::new(c)
    };

    let uc = Arc::clone(&upload_cluster);
    let mut manager = MockClusterManagerTrait::new();
    let cm = Arc::clone(&cluster_main);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(cm.clone()));
    manager.expect_create_file_upload().returning(move |_, _| {
        let c = Arc::clone(&uc);
        Box::pin(async move { c as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait> })
    });
    manager
        .expect_get_file_upload()
        .returning(move |_| Some(Arc::clone(&fu_for_manager)));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));
    let payload = b"file content here";

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/job/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b&targetPath=/dest/file.txt"
                ))
                .header("authorization", &token)
                .header("content-length", payload.len().to_string())
                .body(Body::from(payload.as_slice()))
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
    assert_eq!(body["status"].as_str().unwrap(), "completed");
    let upload_id = body["uploadId"]
        .as_str()
        .expect("uploadId should be present");
    assert!(uuid::Uuid::parse_str(upload_id).is_ok());
}

/// Tests that a cluster error during upload propagates to a 400 response.
///
/// # Setup
/// Inserts a test job. A background task sets the error flag and error details before
/// `SERVER_READY` arrives in the `FileUploadState`.
///
/// # Act
/// Sends PUT /job/apiv1/file/upload/ with a small body.
///
/// # Assert
/// Verifies 400 Bad Request with body containing the cluster error message.
#[tokio::test]
async fn test_upload_file_server_error_returns_400() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    let fu_state = Arc::new(FileUploadState::new());
    let fu_sim = Arc::clone(&fu_state);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        *fu_sim.error_details.lock().await = "Cluster rejected upload".to_string();
        fu_sim.error.store(true, Ordering::Release);
        fu_sim.data_ready.store(true, Ordering::Release);
        fu_sim.data_notify.notify_waiters();
    });

    let fu_for_manager = Arc::clone(&fu_state);

    let cluster_main = Arc::new(online_cluster_no_messages());
    let upload_cluster = Arc::new(upload_cluster());

    let uc = Arc::clone(&upload_cluster);
    let mut manager = MockClusterManagerTrait::new();
    let cm = Arc::clone(&cluster_main);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(cm.clone()));
    manager.expect_create_file_upload().returning(move |_, _| {
        let c = Arc::clone(&uc);
        Box::pin(async move { c as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait> })
    });
    manager
        .expect_get_file_upload()
        .returning(move |_| Some(Arc::clone(&fu_for_manager)));

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/job/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b&targetPath=/dest.txt"
                ))
                .header("authorization", &token)
                .header("content-length", "5")
                .body(Body::from("hello"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Cluster rejected upload"));
}

// ---------------------------------------------------------------------------
// Cross-app access tests
// ---------------------------------------------------------------------------

/// Verifies that app2, which lists "app1" in its `applications`, can create a file download
/// for a job owned by app1.
#[tokio::test]
async fn test_create_download_app2_can_access_app1_job() {
    // app2 has "app1" in its applications list → can access app1's jobs
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "app1").await;

    let cluster = Arc::new(online_cluster("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[1], &serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"jobId": job_id, "path": "/test/path"}).to_string(),
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
    let file_id = body["fileId"].as_str().expect("fileId should be present");
    assert!(uuid::Uuid::parse_str(file_id).is_ok());
}

/// Tests that app4 (without app1 in its applications list) cannot create a download for an app1 job.
///
/// # Setup
/// Inserts an app1 job. Uses the multi-secret configuration where secret[3] (app4) does NOT list app1.
///
/// # Act
/// Sends POST /job/apiv1/file/ with app4's token and the app1 job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_download_app4_cannot_access_app1_job() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "app1").await;

    let cluster = Arc::new(online_cluster("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[3], &serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"jobId": job_id, "path": "/test/path"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// POST /job/apiv1/file/ — no-jobId path
// ---------------------------------------------------------------------------

/// Tests the no-jobId path: POST /file/ with cluster and bundle (no jobId) succeeds.
///
/// # Setup
/// Empty DB. Uses multi-secret config. Wires an online cluster for app1's token.
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"cluster": "ozstar", "bundle": "...", "path": "/test/path"}`.
///
/// # Assert
/// Verifies 200 OK with a non-null `fileId`, and the DB record has the correct path,
/// cluster, bundle, and job=0 (no jobId resolved).
#[tokio::test]
async fn test_create_download_no_jobid_success_with_cluster_and_bundle() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;

    let cluster = Arc::new(online_cluster("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state_with_secrets(
        db.clone(),
        manager,
        secrets.clone(),
    ));
    let token = encode_jwt_for_secret(&secrets[0], &serde_json::json!({"userId": 10}));

    // No jobId key at all
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "cluster": "ozstar",
                        "bundle": "test_bundle",
                        "path": "/test/path"
                    })
                    .to_string(),
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
    let file_id = body["fileId"].as_str().expect("fileId should be present");
    assert!(uuid::Uuid::parse_str(file_id).is_ok());

    // Verify the record is in the DB with the resolved cluster/bundle and job=0
    let record = file_download::Entity::find()
        .filter(file_download::Column::Uuid.eq(file_id))
        .one(&db)
        .await
        .unwrap()
        .expect("file download record should be in DB");

    assert_eq!(record.path, "/test/path");
    assert_eq!(record.cluster, "ozstar");
    assert_eq!(record.bundle, "test_bundle");
    assert_eq!(record.job, 0);
}

/// Tests that jobId=0 is treated the same as no-jobId and succeeds with cluster+bundle.
///
/// # Setup
/// Empty DB. Uses multi-secret config. Wires an online cluster.
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"jobId": 0, "cluster": "ozstar", "bundle": "...", "path": "..."}`.
///
/// # Assert
/// Verifies 200 OK with a non-null `fileId`, and the DB record has the correct path,
/// cluster, bundle, and job=0 (jobId=0 treated as no-jobId).
#[tokio::test]
async fn test_create_download_no_jobid_with_zero_jobid_success() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let cluster = Arc::new(online_cluster("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    let app = create_router(make_test_state_with_secrets(
        db.clone(),
        manager,
        secrets.clone(),
    ));
    let token = encode_jwt_for_secret(&secrets[0], &serde_json::json!({"userId": 10}));
    // jobId key with value 0
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "jobId": 0,
                        "cluster": "ozstar",
                        "bundle": "test_bundle",
                        "path": "/test/path"
                    })
                    .to_string(),
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
    let file_id = body["fileId"].as_str().expect("fileId should be present");
    assert!(uuid::Uuid::parse_str(file_id).is_ok());

    // Verify the record is in the DB with the resolved cluster/bundle and job=0
    let record = file_download::Entity::find()
        .filter(file_download::Column::Uuid.eq(file_id))
        .one(&db)
        .await
        .unwrap()
        .expect("file download record should be in DB");

    assert_eq!(record.path, "/test/path");
    assert_eq!(record.cluster, "ozstar");
    assert_eq!(record.bundle, "test_bundle");
    assert_eq!(record.job, 0);
}

/// Tests that the no-jobId path without a cluster field returns 400.
///
/// # Setup
/// Empty DB. Uses multi-secret config.
///
/// # Act
/// Sends POST /job/apiv1/file/ with only `{"bundle": "...", "path": "..."}`.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_download_no_jobid_missing_cluster_returns_400() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);
    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[0], &serde_json::json!({"userId": 10}));
    // Only bundle, no cluster
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "bundle": "test_bundle",
                        "path": "/test/path"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that the no-jobId path without a bundle field returns 400.
///
/// # Setup
/// Empty DB. Uses multi-secret config.
///
/// # Act
/// Sends POST /job/apiv1/file/ with only `{"cluster": "ozstar", "path": "..."}`.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_download_no_jobid_missing_bundle_returns_400() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);
    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[0], &serde_json::json!({"userId": 10}));
    // Only cluster, no bundle
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "cluster": "ozstar",
                        "path": "/test/path"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that app4 (no clusters) gets 400 on the no-jobId path.
///
/// # Setup
/// Empty DB. Uses multi-secret config. secret[3] (app4) has no cluster access.
///
/// # Act
/// Sends POST /job/apiv1/file/ with app4's token and cluster+bundle.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_download_no_jobid_no_cluster_access_returns_400() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;

    let cluster = Arc::new(online_cluster("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[3], &serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "cluster": "ozstar",
                        "bundle": "test_bundle",
                        "path": "/test/path"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that an invalid (unknown) cluster on the no-jobId path returns 400.
///
/// # Setup
/// Empty DB. Wires a manager that returns None for all clusters.
/// Uses app4's token (which fails the cluster access check first).
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"cluster": "not_a_real_cluster", "bundle": "...", "path": "..."}`.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_download_no_jobid_invalid_cluster_returns_400() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    // Unknown cluster → get_cluster_by_name returns None
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    // app4 has no cluster access so it will fail on cluster check first
    let token = encode_jwt_for_secret(&secrets[3], &serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "cluster": "not_a_real_cluster",
                        "bundle": "test_bundle",
                        "path": "/test/path"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// POST /job/apiv1/file/ — empty paths list
// ---------------------------------------------------------------------------

/// Tests that an empty paths array returns 200 with an empty fileIds array.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /job/apiv1/file/ with `{"jobId": ..., "paths": []}`.
///
/// # Assert
/// Verifies 200 OK with `fileIds` being an empty array.
#[tokio::test]
async fn test_create_download_empty_path_list_returns_empty_file_ids() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let manager = manager_with_online_cluster_no_messages();

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "paths": []
                    })
                    .to_string(),
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
    assert!(body["fileIds"].is_array());
    assert!(body["fileIds"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// PATCH /job/apiv1/file/ — cross-app access
// ---------------------------------------------------------------------------

/// Tests that app2 can access an app1 job via PATCH /file/ and reaches the cluster check.
///
/// # Setup
/// Inserts an app1 job. Wires an offline cluster. Uses secret[1] (app2) which lists app1.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with app2's token and the app1 job ID.
///
/// # Assert
/// Verifies 503 Service Unavailable (access granted but cluster is offline).
#[tokio::test]
async fn test_list_files_app2_can_access_app1_job() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "app1").await;

    // Provide an offline cluster so we get 503 (not 400 "Invalid cluster")
    let mut offline = MockClusterTrait::new();
    offline.expect_name().returning(|| "ozstar".to_string());
    offline.expect_is_online().returning(|| false);
    let offline_arc = Arc::new(offline);

    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&offline_arc);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[1], &serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "recursive": true,
                        "path": "/test/path"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // app2 CAN access app1's job — but cluster is offline, so 503
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// Tests that app4 cannot access an app1 job via PATCH /file/ and receives 400.
///
/// # Setup
/// Inserts an app1 job. Wires an online cluster. Uses secret[3] (app4) which does NOT list app1.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ with app4's token and the app1 job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_list_files_app4_cannot_access_app1_job() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "app1").await;

    let cluster = Arc::new(online_cluster("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[3], &serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "recursive": true,
                        "path": "/test/path"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // app4 CANNOT access app1's job → 400
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Content-Type tolerance tests
// ---------------------------------------------------------------------------

/// Tests that POST /file/ succeeds without Content-Type header.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /job/apiv1/file/ WITHOUT content-type header.
///
/// # Assert
/// Verifies 200 OK (not 415 Unsupported Media Type).
#[tokio::test]
async fn test_create_file_download_works_without_content_type_header() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let manager = manager_with_online_cluster_no_messages();

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 10}));

    // Send request WITHOUT Content-Type header
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                // NOTE: No content-type header
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "path": "/result/output.txt"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 200 OK, not 415
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let file_id = body["fileId"].as_str().expect("fileId should be present");
    assert!(uuid::Uuid::parse_str(file_id).is_ok());
}

/// Tests that PATCH /file/ (list files) succeeds without Content-Type header.
///
/// # Setup
/// Inserts a completed job with cached files.
///
/// # Act
/// Sends PATCH /job/apiv1/file/ WITHOUT content-type header.
///
/// # Assert
/// Verifies 200 OK.
#[tokio::test]
async fn test_list_files_works_without_content_type_header() {
    let db = setup_test_db().await;

    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job_id, JobStatus::Completed as i32, "_job_completion_").await;

    for (name, is_dir) in [("/out/results.txt", false), ("/out/", true)] {
        file_list_cache::ActiveModel {
            job_id: Set(job_id),
            path: Set(name.to_string()),
            is_dir: Set(is_dir),
            file_size: Set(1024),
            permissions: Set(0o644),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let manager = manager_with_online_cluster_no_messages();

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    // Send request WITHOUT Content-Type header
    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/file/")
                // NOTE: No content-type header
                .header("authorization", &token)
                .body(Body::from(
                    serde_json::json!({
                        "jobId": job_id,
                        "path": "",
                        "recursive": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

/// Tests that invalid JSON is still rejected in file endpoints without Content-Type header.
///
/// # Setup
/// Empty database.
///
/// # Act
/// Sends POST /job/apiv1/file/ with invalid JSON and no Content-Type header.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_file_download_rejects_invalid_json_without_content_type() {
    let db = setup_test_db().await;
    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(|_| None);

    let app = create_router(make_test_state(db, manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    // Send request with invalid JSON and no Content-Type header
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/file/")
                .header("authorization", &token)
                .body(Body::from(r#"{"jobId": 1, invalid json}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should still reject invalid JSON
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// resolve_cluster_bundle_for_file_list — direct unit tests
// ---------------------------------------------------------------------------

/// Tests that `resolve_cluster_bundle_for_file_list` returns the cluster and bundle
/// for a job the application is allowed to access.
///
/// # Setup
/// Inserts a job owned by `testapp`.
///
/// # Act
/// Calls `resolve_cluster_bundle_for_file_list` with `applications = ["testapp"]`.
///
/// # Assert
/// Verifies `Ok(("ozstar", "b"))`.
#[tokio::test]
async fn test_resolve_cluster_bundle_for_file_list_success() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let state = make_test_state(db, MockClusterManagerTrait::new());
    let result = adacs_job_controller::http::file::resolve_cluster_bundle_for_file_list(
        &state,
        &["testapp".to_string()],
        "testapp",
        job_id as u64,
    )
    .await;

    assert_eq!(result, Ok(("ozstar".to_string(), "b".to_string())));
}

/// Tests that `resolve_cluster_bundle_for_file_list` returns an error when the
/// application does not have access to the job.
///
/// # Setup
/// Inserts a job owned by `testapp`.
///
/// # Act
/// Calls `resolve_cluster_bundle_for_file_list` with `applications = ["other_app"]`.
///
/// # Assert
/// Verifies a 400 Bad Request error whose message mentions the job ID and app name.
#[tokio::test]
async fn test_resolve_cluster_bundle_for_file_list_app_without_access_returns_error() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let state = make_test_state(db, MockClusterManagerTrait::new());
    let result = adacs_job_controller::http::file::resolve_cluster_bundle_for_file_list(
        &state,
        &["other_app".to_string()],
        "other_app",
        job_id as u64,
    )
    .await;

    let (status, msg) = result.expect_err("expected an error for a job the app cannot access");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        msg.contains(&format!("Unable to find job with ID {job_id}")),
        "msg: {msg}"
    );
    assert!(msg.contains("other_app"), "msg: {msg}");
}

/// Tests that `resolve_cluster_bundle_for_file_list` returns an error when the job
/// does not exist.
///
/// # Setup
/// Empty DB.
///
/// # Act
/// Calls `resolve_cluster_bundle_for_file_list` with a non-existent job ID.
///
/// # Assert
/// Verifies a 400 Bad Request error whose message mentions the job ID and app name.
#[tokio::test]
async fn test_resolve_cluster_bundle_for_file_list_missing_job_returns_error() {
    let db = setup_test_db().await;
    let missing_job_id: u64 = 999_999;

    let state = make_test_state(db, MockClusterManagerTrait::new());
    let result = adacs_job_controller::http::file::resolve_cluster_bundle_for_file_list(
        &state,
        &["testapp".to_string()],
        "testapp",
        missing_job_id,
    )
    .await;

    let (status, msg) = result.expect_err("expected an error for a non-existent job");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        msg.contains(&format!("Unable to find job with ID {missing_job_id}")),
        "msg: {msg}"
    );
    assert!(msg.contains("testapp"), "msg: {msg}");
}

// ---------------------------------------------------------------------------
// Download-session cleanup guards and real-manager cleanup regression
// ---------------------------------------------------------------------------

mod download_session_cleanup {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use dashmap::DashMap;
    use sea_orm::Database;

    use adacs_job_controller::cluster::file_download::{
        DownloadCleanupRequest, DownloadSession, DownloadSessionState, DownloadShutdownReason,
        FileDownloadState,
    };
    use adacs_job_controller::cluster::manager::ClusterManager;
    use adacs_job_controller::cluster::traits::{ClusterManagerTrait, ClusterTrait};
    use adacs_job_controller::config::clusters::ClusterConfig;
    use adacs_job_controller::http::file::PreResponseGuard;

    fn new_session() -> (
        Arc<DownloadSession>,
        tokio::sync::mpsc::UnboundedReceiver<DownloadCleanupRequest>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            DownloadSession::new(
                "cleanup-1".to_string(),
                Arc::new(FileDownloadState::new()),
                tx,
            ),
            rx,
        )
    }

    async fn real_manager() -> (
        sea_orm::DatabaseConnection,
        Arc<ClusterManager>,
        Arc<dyn ClusterTrait>,
    ) {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("sqlite in-memory connection failed");
        adacs_job_controller::db::schema::create_test_schema(&db).await;
        let file_list_map = Arc::new(DashMap::new());
        let mgr = ClusterManager::new(
            vec![ClusterConfig {
                name: "real_cluster".to_string(),
                host: "127.0.0.1".to_string(),
                username: "test".to_string(),
                path: "/tmp".to_string(),
                key: String::new(),
                connection_type: "manual".to_string(),
                keytab: String::new(),
                kerberos_principal: String::new(),
                ltk: None,
            }],
            db.clone(),
            file_list_map,
        );
        let cluster = mgr
            .get_cluster_by_name("real_cluster")
            .expect("real manager should expose the configured cluster");
        (db, mgr, cluster)
    }

    async fn wait_for_cleanup(mgr: &ClusterManager, uuid: &str, deadline: Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < deadline {
            if mgr.get_file_download(uuid).is_none() && mgr.dedicated_download_clusters().is_empty()
            {
                return true;
            }
            tokio::task::yield_now().await;
        }
        false
    }

    // ---- Pre-response guard primitives ----

    #[test]
    fn pre_response_guard_drop_fires_response_error_when_not_consumed() {
        let (session, _rx) = new_session();
        let trigger = session.cleanup_trigger();
        let guard = PreResponseGuard::new(trigger, DownloadShutdownReason::ResponseError);
        drop(guard);
        assert_eq!(
            session.state(),
            DownloadSessionState::Closing {
                connection_id: None,
                reason: DownloadShutdownReason::ResponseError,
            }
        );
    }

    #[test]
    fn pre_response_guard_explicit_trigger_disarms_drop() {
        let (session, _rx) = new_session();
        let trigger = session.cleanup_trigger();
        let guard = PreResponseGuard::new(trigger, DownloadShutdownReason::ResponseError);
        let won = guard.trigger(DownloadShutdownReason::Complete);
        assert!(won);
        assert_eq!(
            session.state(),
            DownloadSessionState::Closing {
                connection_id: None,
                reason: DownloadShutdownReason::Complete,
            }
        );
    }

    #[test]
    fn pre_response_guard_into_trigger_disarms_response_error_drop() {
        let (session, mut rx) = new_session();
        let trigger = session.cleanup_trigger();
        let guard = PreResponseGuard::new(trigger, DownloadShutdownReason::ResponseError);
        let streaming_trigger = guard
            .into_trigger()
            .expect("trigger should transfer to streaming owner");
        assert_eq!(session.state(), DownloadSessionState::Pending);
        assert!(rx.try_recv().is_err());

        assert!(streaming_trigger.trigger(DownloadShutdownReason::Complete));
        assert_eq!(
            session.state(),
            DownloadSessionState::Closing {
                connection_id: None,
                reason: DownloadShutdownReason::Complete,
            }
        );
    }

    #[test]
    fn pre_response_guard_trigger_clone_does_not_disarm_guard() {
        let (session, mut rx) = new_session();
        let trigger = session.cleanup_trigger();
        let guard = PreResponseGuard::new(trigger, DownloadShutdownReason::ResponseError);

        let cloned = guard
            .trigger_clone()
            .expect("trigger_clone should yield a trigger without disarming the guard");
        let remaining = guard
            .into_trigger()
            .expect("guard should still hold its trigger after trigger_clone");

        assert_eq!(session.state(), DownloadSessionState::Pending);
        assert!(rx.try_recv().is_err());

        assert!(cloned.trigger(DownloadShutdownReason::Complete));
        assert_eq!(
            session.state(),
            DownloadSessionState::Closing {
                connection_id: None,
                reason: DownloadShutdownReason::Complete,
            }
        );

        assert!(!remaining.trigger(DownloadShutdownReason::FileError));
    }

    // ---- Trigger racing ----

    #[test]
    fn two_triggers_racing_share_one_notification() {
        let (session, mut rx) = new_session();
        let first = session.cleanup_trigger();
        let second = session.cleanup_trigger();

        assert!(first.trigger(DownloadShutdownReason::ResponseError));
        assert!(!second.trigger(DownloadShutdownReason::Complete));

        let request = rx.try_recv().expect("one cleanup notification expected");
        assert_eq!(request.reason, DownloadShutdownReason::ResponseError);
        assert!(rx.try_recv().is_err());
        assert_eq!(
            session.state(),
            DownloadSessionState::Closing {
                connection_id: None,
                reason: DownloadShutdownReason::ResponseError,
            }
        );
    }

    // ---- Real-manager cleanup to baseline ----

    async fn fire_and_drain(reason: DownloadShutdownReason) {
        let (_db, mgr, cluster) = real_manager().await;
        let uuid = format!("cleanup-{reason:?}");
        let _dl = mgr.create_file_download(&cluster, &uuid).await;
        assert!(
            mgr.get_file_download(&uuid).is_some(),
            "session must exist after create_file_download"
        );
        let trigger = mgr
            .get_file_download_cleanup_trigger(&uuid)
            .expect("cleanup trigger must be present");
        assert!(trigger.trigger(reason));
        assert!(
            wait_for_cleanup(&mgr, &uuid, Duration::from_secs(10)).await,
            "cleanup should drain maps to baseline for {reason:?}"
        );
        assert!(
            mgr.get_file_download_cleanup_trigger(&uuid).is_none(),
            "cleanup trigger lookup must return None after cleanup"
        );
    }

    #[tokio::test]
    async fn real_manager_complete_returns_to_baseline() {
        fire_and_drain(DownloadShutdownReason::Complete).await;
    }

    #[tokio::test]
    async fn real_manager_chunk_timeout_returns_to_baseline() {
        fire_and_drain(DownloadShutdownReason::ChunkTimeout).await;
    }

    #[tokio::test]
    async fn real_manager_file_error_returns_to_baseline() {
        fire_and_drain(DownloadShutdownReason::FileError).await;
    }

    #[tokio::test]
    async fn real_manager_cluster_offline_returns_to_baseline() {
        fire_and_drain(DownloadShutdownReason::ClusterOffline).await;
    }

    #[tokio::test]
    async fn real_manager_response_error_returns_to_baseline() {
        fire_and_drain(DownloadShutdownReason::ResponseError).await;
    }

    #[tokio::test]
    async fn real_manager_no_cleanup_without_trigger() {
        let (_db, mgr, cluster) = real_manager().await;
        let uuid = "no-trigger-uuid";
        let _dl = mgr.create_file_download(&cluster, uuid).await;
        let dl = mgr
            .get_file_download(uuid)
            .expect("download session must be registered after create_file_download");
        assert!(
            !dl.error.load(Ordering::Relaxed),
            "fresh session must not be in error state"
        );
        assert!(
            !dl.data_ready.load(Ordering::Relaxed),
            "fresh session must not be data-ready"
        );
        assert_eq!(
            dl.file_size.load(Ordering::Relaxed),
            0,
            "fresh session must have zero file size"
        );
        assert!(
            mgr.get_file_download_cleanup_trigger(uuid).is_some(),
            "cleanup trigger must be present for fresh session"
        );
        tokio::task::yield_now().await;
        assert!(
            mgr.get_file_download(uuid).is_some(),
            "session must remain registered when no trigger fires"
        );
    }

    // ---- Size bound ----
}
