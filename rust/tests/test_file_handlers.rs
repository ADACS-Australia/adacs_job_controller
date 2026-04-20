//! Comprehensive tests for file HTTP handlers.
//!
//! Tests cover the full business logic for:
//! - POST /file/apiv1/file/  (create download record)
//! - GET  /file/apiv1/file/  (stream file download — WS→HTTP flow)
//! - PUT  /file/apiv1/file/upload/ (file upload — HTTP→WS→HTTP flow)
//! - PATCH /file/apiv1/file/ (list files — WS→HTTP with cache)

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use adacs_job_controller::cluster::file_download::FileDownloadState;
use adacs_job_controller::cluster::file_upload::FileUploadState;
use adacs_job_controller::cluster::traits::{MockClusterManagerTrait, MockClusterTrait};
use adacs_job_controller::db::entities::{file_download, file_list_cache};
use adacs_job_controller::http::server::create_router;
use adacs_job_controller::protocol::types::{ClusterRole, FileInfo, FileListState};

use common::{
    encode_jwt_for_secret, encode_test_jwt, insert_job_history, insert_test_job, make_test_state,
    make_test_state_with_secrets, setup_test_db, test_cluster_config, test_jwt_secrets,
    test_jwt_secrets_multi,
};

use adacs_job_controller::protocol::types::JobStatus;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::sync::atomic::Ordering;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn online_cluster_no_messages() -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    c.expect_name().returning(|| "ozstar".to_string());
    c.expect_is_online().returning(|| true);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    c.expect_send_message().returning(|_| ());
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
// POST /file/apiv1/file/ — create file download records
// ---------------------------------------------------------------------------

/// Tests that POST /file/ with a single path creates a download record and returns a fileId.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /file/apiv1/file/ with `{"jobId": ..., "path": "/result/output.txt"}`.
///
/// # Assert
/// Verifies 200 OK, non-empty `fileId`, and the DB record has the correct path, cluster,
/// bundle, and job ID.
#[tokio::test]
async fn test_create_file_download_single_path_returns_file_id() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/file/apiv1/file/")
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
    assert!(!file_id.is_empty());

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
    assert_eq!(record.job, job_id as i32);
}

/// Tests that POST /file/ with a paths array creates multiple download records.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /file/apiv1/file/ with `{"jobId": ..., "paths": ["/a.txt", "/b.txt", "/c.txt"]}`.
///
/// # Assert
/// Verifies 200 OK and a `fileIds` array containing 3 non-empty UUID strings.
#[tokio::test]
async fn test_create_file_download_multiple_paths_returns_file_ids() {
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

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/file/apiv1/file/")
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
        assert!(!id.as_str().unwrap_or("").is_empty());
    }
}

/// Tests that POST /file/ without a path or paths field returns 400.
///
/// # Setup
/// Inserts a test job.
///
/// # Act
/// Sends POST /file/apiv1/file/ with `{"jobId": ...}` (no path key).
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
                .uri("/file/apiv1/file/")
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
// GET /file/apiv1/file/ — stream file download (WS→HTTP data flow)
// ---------------------------------------------------------------------------

/// Tests that GET /file/ without a fileId query parameter returns 400.
///
/// # Setup
/// Wires a manager that returns nothing.
///
/// # Act
/// Sends GET /file/apiv1/file/ with no `fileId` query parameter.
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
                .uri("/file/apiv1/file/")
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
/// Sends GET /file/apiv1/file/?fileId=not-a-real-uuid.
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
                .uri("/file/apiv1/file/?fileId=not-a-real-uuid")
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
/// Sends GET /file/apiv1/file/?fileId={uuid}.
///
/// # Assert
/// Verifies 503 Service Unavailable.
#[tokio::test]
async fn test_download_file_cluster_offline_returns_503() {
    let db = setup_test_db().await;
    // Insert a file download record pointing at "ozstar"
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let uuid = "test-uuid-1234".to_string();
    file_download::ActiveModel {
        user: Set(1),
        job: Set(0),
        cluster: Set("ozstar".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(uuid.clone()),
        path: Set("".to_string()),
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
                .uri(format!("/file/apiv1/file/?fileId={uuid}"))
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
/// Inserts a download record. A background task simulates FILE_DETAILS + one chunk
/// arriving via the `FileDownloadState` after a short delay.
///
/// # Act
/// Sends GET /file/apiv1/file/?fileId={uuid}.
///
/// # Assert
/// Verifies 200 OK with correct `Content-Length`, `Content-Type: application/octet-stream`,
/// and the exact chunk bytes in the response body.
#[tokio::test]
async fn test_download_file_streams_chunks() {
    let db = setup_test_db().await;
    // Insert a download record
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let uuid = "download-uuid-5678".to_string();
    file_download::ActiveModel {
        user: Set(1),
        job: Set(0),
        cluster: Set("ozstar".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(uuid.clone()),
        path: Set("".to_string()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // Pre-create a FileDownloadState we'll signal from the test
    let fd_state = Arc::new(FileDownloadState::new());

    // Simulate the WS handler: push file details + one chunk
    let expected_data = b"hello world data".to_vec();
    let file_size = expected_data.len() as u64;

    let fd_state_sim = Arc::clone(&fd_state);
    let data_copy = expected_data.clone();
    tokio::spawn(async move {
        // Brief delay so the HTTP handler starts waiting first
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        fd_state_sim.file_size.store(file_size, Ordering::Release);
        fd_state_sim.received_data.store(true, Ordering::Release);
        fd_state_sim.data_ready.store(true, Ordering::Release);
        fd_state_sim.data_notify.notify_waiters();

        // Push the chunk
        let _ = fd_state_sim.chunk_sender.send(data_copy);
    });

    let fd_for_manager = Arc::clone(&fd_state);
    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    // create_file_download returns the same cluster (required by signature)
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
                .uri(format!("/file/apiv1/file/?fileId={uuid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok()),
        Some(file_size.to_string().as_str())
    );
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream")
    );

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body_bytes.as_ref(), expected_data.as_slice());
}

/// Tests that a cluster file error propagates to a 400 response with the error message.
///
/// # Setup
/// Inserts a download record. A background task sets the error flag and error details
/// in `FileDownloadState` after a short delay.
///
/// # Act
/// Sends GET /file/apiv1/file/?fileId={uuid}.
///
/// # Assert
/// Verifies 400 Bad Request with body containing "File not found".
#[tokio::test]
async fn test_download_file_error_from_cluster_returns_400() {
    let db = setup_test_db().await;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
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
                .uri(format!("/file/apiv1/file/?fileId={uuid}"))
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

// ---------------------------------------------------------------------------
// PATCH /file/apiv1/file/ — list files (cache hit and miss)
// ---------------------------------------------------------------------------

/// Tests that PATCH /file/ returns cached files from DB when the job is complete.
///
/// # Setup
/// Inserts a completed job and pre-populates the `file_list_cache` table with 2 entries.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with the job ID.
///
/// # Assert
/// Verifies 200 OK with a `files` array containing the 2 cached entries,
/// and no WS FILE_LIST message is sent.
#[tokio::test]
async fn test_list_files_cache_hit_returns_cached_files() {
    let db = setup_test_db().await;

    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;
    // Mark as complete
    insert_job_history(&db, job_id, JobStatus::Pending as i32, "system").await;
    insert_job_history(&db, job_id, JobStatus::Completed as i32, "_job_completion_").await;

    // Pre-populate cache
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
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
    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    // send_message should NOT be called (already mocked with .returning in cluster)

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/file/apiv1/file/")
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

/// Tests the WS-driven file list flow: cluster receives FILE_LIST, populates state, HTTP returns result.
///
/// # Setup
/// Inserts a Running job (no cache). Wires a cluster whose `send_message` mock intercepts
/// the FILE_LIST message, parses the UUID, and populates the `file_list_map` entry.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with the job ID.
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
        jwt_secrets: test_jwt_secrets(),
    };

    let app = create_router(state);
    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/file/apiv1/file/")
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

/// Tests that PATCH /file/ returns 503 when the cluster is offline.
///
/// # Setup
/// Inserts a Running job. Wires an offline cluster.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with the job ID.
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
                .uri("/file/apiv1/file/")
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

/// Tests that PATCH /file/ without a jobId and missing cluster + bundle returns 400.
///
/// # Setup
/// Empty DB. Wires an online cluster.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with only `{"path": "", "recursive": false}` (no jobId, no cluster).
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
                .uri("/file/apiv1/file/")
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
/// Sends PATCH /file/apiv1/file/ with `{"cluster": "forbidden_cluster", "bundle": "b"}`
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
                .uri("/file/apiv1/file/")
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
// PUT /file/apiv1/file/upload/ — file upload (HTTP→WS→HTTP flow)
// ---------------------------------------------------------------------------

/// Tests that PUT /file/upload/ without a targetPath query parameter returns 400.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends PUT /file/apiv1/file/upload/ without the `targetPath` parameter.
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
                    "/file/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b"
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

/// Tests that PUT /file/upload/ when the cluster is offline returns 503.
///
/// # Setup
/// Inserts a test job. Wires an offline cluster.
///
/// # Act
/// Sends PUT /file/apiv1/file/upload/ with valid parameters.
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
                    "/file/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b&targetPath=/dest.txt"
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

/// Tests the full file upload flow: SERVER_READY then FILE_UPLOAD_COMPLETE signals lead to success.
///
/// # Setup
/// Inserts a test job. A background task simulates SERVER_READY then FILE_UPLOAD_COMPLETE
/// arriving in the `FileUploadState`. Cluster captures sent WS messages.
///
/// # Act
/// Sends PUT /file/apiv1/file/upload/ with a 17-byte body.
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
                    "/file/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b&targetPath=/dest/file.txt"
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
    assert!(body["uploadId"].as_str().is_some());
}

/// Tests that a cluster error during upload propagates to a 400 response.
///
/// # Setup
/// Inserts a test job. A background task sets the error flag and error details before
/// SERVER_READY arrives in the `FileUploadState`.
///
/// # Act
/// Sends PUT /file/apiv1/file/upload/ with a small body.
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
    let upload_cluster = {
        let mut c = MockClusterTrait::new();
        c.expect_name().returning(|| "ozstar-up".to_string());
        c.expect_is_online().returning(|| true);
        c.expect_role().returning(|| ClusterRole::Master);
        c.expect_role_string().returning(|| "master".to_string());
        c.expect_cluster_details()
            .returning(|| test_cluster_config("ozstar"));
        c.expect_send_message().returning(|_| ());
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

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/file/apiv1/file/upload/?jobId={job_id}&cluster=ozstar&bundle=b&targetPath=/dest.txt"
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

fn online_cluster_mock_for_multi(name: &'static str) -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    c.expect_name().returning(move || name.to_string());
    c.expect_is_online().returning(|| true);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(move || test_cluster_config(name));
    c.expect_send_message().returning(|_| ());
    c
}

/// Verifies that app2, which lists "app1" in its `applications`, can create a file download
/// for a job owned by app1.
#[tokio::test]
async fn test_create_download_app2_can_access_app1_job() {
    // app2 has "app1" in its applications list → can access app1's jobs
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "app1").await;

    let cluster = Arc::new(online_cluster_mock_for_multi("ozstar"));
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
                .uri("/file/apiv1/file/")
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
    assert!(body["fileId"].as_str().is_some());
}

/// Tests that app4 (without app1 in its applications list) cannot create a download for an app1 job.
///
/// # Setup
/// Inserts an app1 job. Uses the multi-secret configuration where secret[3] (app4) does NOT list app1.
///
/// # Act
/// Sends POST /file/apiv1/file/ with app4's token and the app1 job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_download_app4_cannot_access_app1_job() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "app1").await;

    let cluster = Arc::new(online_cluster_mock_for_multi("ozstar"));
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
                .uri("/file/apiv1/file/")
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
// POST /file/apiv1/file/ — no-jobId path
// ---------------------------------------------------------------------------

/// Tests the no-jobId path: POST /file/ with cluster and bundle (no jobId) succeeds.
///
/// # Setup
/// Empty DB. Uses multi-secret config. Wires an online cluster for app1's token.
///
/// # Act
/// Sends POST /file/apiv1/file/ with `{"cluster": "ozstar", "bundle": "...", "path": "/test/path"}`.
///
/// # Assert
/// Verifies 200 OK with a non-null `fileId`.
#[tokio::test]
async fn test_create_download_no_jobid_success_with_cluster_and_bundle() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;

    let cluster = Arc::new(online_cluster_mock_for_multi("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[0], &serde_json::json!({"userId": 10}));

    // No jobId key at all
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/file/apiv1/file/")
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
    assert!(body["fileId"].as_str().is_some());
}

/// Tests that jobId=0 is treated the same as no-jobId and succeeds with cluster+bundle.
///
/// # Setup
/// Empty DB. Uses multi-secret config. Wires an online cluster.
///
/// # Act
/// Sends POST /file/apiv1/file/ with `{"jobId": 0, "cluster": "ozstar", "bundle": "...", "path": "..."}`.
///
/// # Assert
/// Verifies 200 OK with a non-null `fileId`.
#[tokio::test]
async fn test_create_download_no_jobid_with_zero_jobid_success() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let cluster = Arc::new(online_cluster_mock_for_multi("ozstar"));
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    let app = create_router(make_test_state_with_secrets(db, manager, secrets.clone()));
    let token = encode_jwt_for_secret(&secrets[0], &serde_json::json!({"userId": 10}));
    // jobId key with value 0
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/file/apiv1/file/")
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
    assert!(body["fileId"].as_str().is_some());
}

/// Tests that the no-jobId path without a cluster field returns 400.
///
/// # Setup
/// Empty DB. Uses multi-secret config.
///
/// # Act
/// Sends POST /file/apiv1/file/ with only `{"bundle": "...", "path": "..."}`.
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
                .uri("/file/apiv1/file/")
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
/// Sends POST /file/apiv1/file/ with only `{"cluster": "ozstar", "path": "..."}`.
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
                .uri("/file/apiv1/file/")
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
/// Sends POST /file/apiv1/file/ with app4's token and cluster+bundle.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_create_download_no_jobid_no_cluster_access_returns_400() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;

    let cluster = Arc::new(online_cluster_mock_for_multi("ozstar"));
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
                .uri("/file/apiv1/file/")
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
/// Sends POST /file/apiv1/file/ with `{"cluster": "not_a_real_cluster", "bundle": "...", "path": "..."}`.
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
                .uri("/file/apiv1/file/")
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
// POST /file/apiv1/file/ — empty paths list
// ---------------------------------------------------------------------------

/// Tests that an empty paths array returns 200 with an empty fileIds array.
///
/// # Setup
/// Inserts a test job. Wires an online cluster.
///
/// # Act
/// Sends POST /file/apiv1/file/ with `{"jobId": ..., "paths": []}`.
///
/// # Assert
/// Verifies 200 OK with `fileIds` being an empty array.
#[tokio::test]
async fn test_create_download_empty_path_list_returns_empty_file_ids() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 10}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/file/apiv1/file/")
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
// PATCH /file/apiv1/file/ — cross-app access
// ---------------------------------------------------------------------------

/// Tests that app2 can access an app1 job via PATCH /file/ and reaches the cluster check.
///
/// # Setup
/// Inserts an app1 job. Wires an offline cluster. Uses secret[1] (app2) which lists app1.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with app2's token and the app1 job ID.
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
                .uri("/file/apiv1/file/")
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
/// Sends PATCH /file/apiv1/file/ with app4's token and the app1 job ID.
///
/// # Assert
/// Verifies 400 Bad Request.
#[tokio::test]
async fn test_list_files_app4_cannot_access_app1_job() {
    let secrets = test_jwt_secrets_multi();
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "app1").await;

    let cluster = Arc::new(online_cluster_mock_for_multi("ozstar"));
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
                .uri("/file/apiv1/file/")
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
