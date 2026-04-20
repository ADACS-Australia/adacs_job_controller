//! Integration tests for HTTP handlers using axum's tower::ServiceExt::oneshot.
//!
//! Tests exercise:
//!   - JWT authentication (no token, bad token, valid token)
//!   - Job API response structure
//!   - File API response structure
//!
//! All tests use mock cluster manager (no real DB or WS needed).

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use adacs_job_controller::app::AppState;
use adacs_job_controller::cluster::traits::MockClusterManagerTrait;
use adacs_job_controller::config::access_secrets::AccessSecret;
use adacs_job_controller::http::server::create_router;

use crate::common::{encode_test_jwt, test_jwt_secrets};

// ---------------------------------------------------------------------------
// Test router construction helpers
// ---------------------------------------------------------------------------

/// Create a Router wired to a mock AppState with SQLite in-memory DB.
fn test_router_with_manager(
    manager: MockClusterManagerTrait,
    secrets: Vec<AccessSecret>,
) -> Router {
    // Use SQLite in-memory for tests — no real DB needed for auth-only tests
    let db = futures::executor::block_on(sea_orm::Database::connect("sqlite::memory:"))
        .expect("sqlite in-memory connect failed");

    let state = AppState {
        db,
        cluster_manager: Arc::new(manager),
        file_list_map: Arc::new(dashmap::DashMap::new()),
        jwt_secrets: secrets,
    };

    create_router(state)
}

// ---------------------------------------------------------------------------
// Authentication tests (apply to all routes)
// ---------------------------------------------------------------------------

/// Tests that a request with no Authorization header is rejected with 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager (no clusters).
///
/// # Act
/// Sends POST /job/apiv1/job/ with no `authorization` header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_auth_no_token_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    // POST /job/apiv1/job/ with no Authorization header
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"cluster":"ozstar","parameters":"p","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Tests that a malformed JWT token is rejected with 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager (no clusters).
///
/// # Act
/// Sends POST /job/apiv1/job/ with a syntactically invalid token string.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_auth_bad_token_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", "totally.invalid.token")
                .body(Body::from(
                    r#"{"cluster":"ozstar","parameters":"p","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Tests that a JWT signed with the wrong secret is rejected with 403 Forbidden.
///
/// # Setup
/// Creates a minimal router using the test secrets. Manually encodes a JWT with
/// a different (wrong) HMAC key.
///
/// # Act
/// Sends POST /job/apiv1/job/ with the incorrectly-signed token.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_auth_wrong_secret_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let token = {
        use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
        encode(
            &Header::new(Algorithm::HS256),
            &serde_json::json!({"userId": 1}),
            &EncodingKey::from_secret(b"wrong_secret"),
        )
        .unwrap()
    };

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"cluster":"ozstar","parameters":"p","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Job API: valid token, cluster access check
// ---------------------------------------------------------------------------

/// Tests that requesting a cluster not in the token's allowed list returns 400.
///
/// # Setup
/// Creates a router with a mock manager that returns no clusters.
///
/// # Act
/// Sends POST /job/apiv1/job/ with a valid token but cluster name "unknown_cluster".
///
/// # Assert
/// Verifies 400 response with body containing "does not have access".
#[tokio::test]
async fn test_create_job_cluster_not_in_secret_returns_bad_request() {
    // The test secret has clusters: ["ozstar", "nci"]
    // Requesting cluster "unknown_cluster" should fail with access error
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let token = encode_test_jwt(&serde_json::json!({"userId": 42}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"cluster":"unknown_cluster","parameters":"p","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        body_str.contains("does not have access"),
        "body was: {body_str}"
    );
}

/// Tests that a known-cluster-name that the manager cannot find returns 400.
///
/// # Setup
/// Creates a router with a mock manager that returns no cluster for any name.
///
/// # Act
/// Sends POST /job/apiv1/job/ with a valid token and cluster "ozstar" (in the secret).
///
/// # Assert
/// Verifies 400 response with body containing "Invalid cluster".
#[tokio::test]
async fn test_create_job_cluster_not_found_returns_bad_request() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let token = encode_test_jwt(&serde_json::json!({"userId": 42}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"cluster":"ozstar","parameters":"p","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Invalid cluster"), "body was: {body_str}");
}

// ---------------------------------------------------------------------------
// File API: PATCH (list_files) tests
// ---------------------------------------------------------------------------

/// Tests that PATCH /file/apiv1/file/ with no auth returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with no Authorization header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_list_files_no_auth_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/file/apiv1/file/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"path":"/project","recursive":true,"cluster":"ozstar","bundle":"test"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// GET /job/apiv1/job/ - query jobs (auth only, DB will fail)
// ---------------------------------------------------------------------------

/// Tests that GET /job/apiv1/job/ with no auth returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends GET /job/apiv1/job/ with no Authorization header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_get_jobs_no_auth_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/job/apiv1/job/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// PATCH /job/apiv1/job/ (cancel) and DELETE /job/apiv1/job/ (delete)
// ---------------------------------------------------------------------------

/// Tests that PATCH /job/apiv1/job/ (cancel) with no auth returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends PATCH /job/apiv1/job/ with no Authorization header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_cancel_job_no_auth_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"jobId": 1}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Tests that DELETE /job/apiv1/job/ with no auth returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends DELETE /job/apiv1/job/ with no Authorization header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_delete_job_no_auth_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"jobId": 1}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// File download / upload: auth tests
// ---------------------------------------------------------------------------

/// Tests that POST /file/apiv1/file/ with no auth returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends POST /file/apiv1/file/ with no Authorization header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_create_file_download_no_auth_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/file/apiv1/file/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jobId":1,"path":"/file.txt","cluster":"ozstar","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Tests that GET /file/apiv1/file/ without a fileId query parameter returns 400.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends GET /file/apiv1/file/ with no `fileId` parameter and no auth token.
///
/// # Assert
/// Verifies the response status is 400 Bad Request (no auth required for this endpoint).
#[tokio::test]
async fn test_download_file_no_file_id_returns_bad_request() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());
    // download_file doesn't use auth
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

    // Without fileId param, should get BAD_REQUEST
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Tests that PUT /file/apiv1/file/upload/ with no auth returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends PUT /file/apiv1/file/upload/ with no Authorization header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_upload_file_no_auth_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/file/apiv1/file/upload/?jobId=1&cluster=ozstar&bundle=b&targetPath=/dest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Valid auth reaching handler logic
// ---------------------------------------------------------------------------

/// Tests that a valid token with a cluster not in the allowed list returns 400.
///
/// # Setup
/// Creates a minimal router. The test JWT secret allows "ozstar" and "nci" only.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with a valid token but cluster "unknown".
///
/// # Assert
/// Verifies 400 Bad Request because "unknown" is not in the secret's cluster list.
#[tokio::test]
async fn test_list_files_valid_auth_missing_cluster_returns_error() {
    // Authenticated but cluster not in secret -> should get BAD_REQUEST
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let token = encode_test_jwt(&serde_json::json!({"userId": 1}));

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/file/apiv1/file/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"path":"/project","recursive":true,"cluster":"unknown","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should fail because "unknown" is not in the secret's clusters
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Route existence tests (all methods on all endpoints)
// ---------------------------------------------------------------------------

/// Tests that GET /job/apiv1/job/ is a registered route (returns 403, not 404).
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends GET /job/apiv1/job/ with no auth.
///
/// # Assert
/// Verifies the response is not 404 (indicates the route exists).
#[tokio::test]
async fn test_routes_exist_for_job_api() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    // An OPTIONS or HEAD request should not return 404/405 for known routes
    // We use GET without auth - should get FORBIDDEN (not NOT_FOUND)
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/job/apiv1/job/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 403 means the route exists (auth failed), not 404
    assert_ne!(resp.status(), StatusCode::NOT_FOUND);
}

/// Tests that PATCH /file/apiv1/file/ is a registered route (returns non-404).
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends PATCH /file/apiv1/file/ with no auth.
///
/// # Assert
/// Verifies the response is not 404 (indicates the route exists).
#[tokio::test]
async fn test_routes_exist_for_file_api() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/file/apiv1/file/")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/","recursive":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(resp.status(), StatusCode::NOT_FOUND);
}

/// Tests that PUT /file/apiv1/file/upload/ is a registered route (returns non-404).
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends PUT /file/apiv1/file/upload/ with no auth.
///
/// # Assert
/// Verifies the response is not 404 (indicates the route exists).
#[tokio::test]
async fn test_upload_route_exists() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/file/apiv1/file/upload/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(resp.status(), StatusCode::NOT_FOUND);
}
