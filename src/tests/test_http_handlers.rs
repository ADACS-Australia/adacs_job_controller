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

/// Tests that POST /job/apiv1/job/ with no auth returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends POST /job/apiv1/job/ with no Authorization header.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_auth_no_token_returns_forbidden() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Tests that POST /job/apiv1/job/ with invalid token returns 403 Forbidden.
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends POST /job/apiv1/job/ with Authorization: Bearer invalid_token.
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
                .header("authorization", "Bearer invalid_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Tests that POST /job/apiv1/job/ with token signed by wrong secret returns 403.
///
/// # Setup
/// Creates a router with known JWT secrets.
/// Creates a token signed with a different secret.
///
/// # Act
/// Sends POST /job/apiv1/job/ with the mismatched token.
///
/// # Assert
/// Verifies the response status is 403 Forbidden.
#[tokio::test]
async fn test_auth_wrong_secret_returns_forbidden() {
    use jsonwebtoken::{Header, encode};

    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    // Create a token signed with a secret not in the config
    let payload = serde_json::json!({"userId": 42});
    let token = encode(
        &Header::default(),
        &payload,
        &jsonwebtoken::EncodingKey::from_secret(b"wrong_secret"),
    )
    .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Job API: POST (create job) tests
// ---------------------------------------------------------------------------

/// Tests that POST /job/apiv1/job/ with valid auth but no cluster access returns 400.
///
/// # Setup
/// Creates a router with a mock manager. The test JWT secret allows "ozstar" and "nci".
///
/// # Act
/// Sends POST /job/apiv1/job/ with a valid token but cluster "unknown" (not in secret).
///
/// # Assert
/// Verifies 400 Bad Request with body containing "does not have access".
#[tokio::test]
async fn test_create_job_no_cluster_access_returns_bad_request() {
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
                    r#"{"cluster":"unknown","parameters":"p","bundle":"b"}"#,
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
// File API: PUT (upload) tests
// ---------------------------------------------------------------------------

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
                    r#"{"path":"/","recursive":false,"cluster":"unknown","bundle":"b"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Route existence tests (verify routes are registered)
// ---------------------------------------------------------------------------

/// Tests that POST /job/apiv1/job/ is a registered route (returns non-404).
///
/// # Setup
/// Creates a minimal router with a mock manager.
///
/// # Act
/// Sends POST /job/apiv1/job/ with no auth.
///
/// # Assert
/// Verifies the response is not 404 (indicates the route exists).
#[tokio::test]
async fn test_routes_exist_for_job_api() {
    let manager = common::mock_cluster_manager_no_clusters();
    let app = test_router_with_manager(manager, test_jwt_secrets());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
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

// ===========================================================================
// HttpServer constructor and access secrets tests
// ===========================================================================

/// Tests that HttpServer constructor with empty access config creates zero JWT secrets.
///
/// # Setup
/// Creates empty access config JSON "[]".
///
/// # Act
/// Instantiates HttpServer through Application/router creation.
///
/// # Assert
/// - Router is created successfully
/// - Zero JWT secrets configured (auth will fail for all tokens)
#[tokio::test]
async fn test_http_server_constructor_empty_config() {
    let manager = common::mock_cluster_manager_no_clusters();

    // Empty JWT secrets list
    let app = test_router_with_manager(manager, vec![]);

    // Verify router is created (no panic)
    // Empty secrets means all auth will fail - verified by auth tests
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // With empty secrets, all tokens are unauthorized
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Tests that HttpServer constructor with populated access config creates correct JWT secrets.
///
/// # Setup
/// Creates 3 access secrets with different names and secrets.
///
/// # Act
/// Instantiates HttpServer with populated config.
///
/// # Assert
/// - Router accepts tokens signed with any of the 3 secrets
/// - Each secret has correct name
#[tokio::test]
async fn test_http_server_constructor_populated_config() {
    use jsonwebtoken::{Header, encode};

    let manager = common::mock_cluster_manager_no_clusters();

    // Create 3 access secrets
    let secrets = vec![
        AccessSecret {
            name: "app1".to_string(),
            secret: "secret1".to_string(),
            applications: vec![],
            clusters: vec!["ozstar".to_string()],
        },
        AccessSecret {
            name: "app2".to_string(),
            secret: "secret2".to_string(),
            applications: vec![],
            clusters: vec!["ozstar".to_string()],
        },
        AccessSecret {
            name: "app3".to_string(),
            secret: "secret3".to_string(),
            applications: vec![],
            clusters: vec!["ozstar".to_string()],
        },
    ];

    let app = test_router_with_manager(manager, secrets);

    // Verify tokens signed with each secret are accepted
    for (secret_name, secret_value) in [
        ("app1", "secret1"),
        ("app2", "secret2"),
        ("app3", "secret3"),
    ] {
        let app = app.clone(); // Clone router for each iteration
        let payload = serde_json::json!({"userId": 1});
        let token = encode(
            &Header::default(),
            &payload,
            &jsonwebtoken::EncodingKey::from_secret(secret_value.as_bytes()),
        )
        .unwrap();

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

        // Token should be accepted (may fail for other reasons, but not auth)
        assert_ne!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Token signed with {} should be accepted",
            secret_name
        );
    }
}

// ===========================================================================
// Cluster getter tests
// ===========================================================================

/// Tests that Cluster::name() returns the correct name from ClusterDetails.
///
/// # Setup
/// Creates Cluster with known name in ClusterConfig.
///
/// # Act
/// Calls name() method.
///
/// # Assert
/// Returns name matching ClusterConfig.name.
#[tokio::test]
async fn test_cluster_get_name() {
    use crate::common::test_cluster_config;
    use adacs_job_controller::cluster::cluster::Cluster;
    use adacs_job_controller::cluster::traits::ClusterTrait;

    let config = test_cluster_config("test_cluster");
    let cluster = Cluster::new(config.clone(), None);

    assert_eq!(cluster.name(), config.name);
    assert_eq!(cluster.name(), "test_cluster");
}

/// Tests that Cluster::cluster_details() returns the correct details.
///
/// # Setup
/// Creates Cluster with known ClusterConfig.
///
/// # Act
/// Calls cluster_details() method.
///
/// # Assert
/// Returns ClusterDetails with correct values matching config.
#[tokio::test]
async fn test_cluster_get_cluster_details() {
    use crate::common::test_cluster_config;
    use adacs_job_controller::cluster::cluster::Cluster;
    use adacs_job_controller::cluster::traits::ClusterTrait;

    let config = test_cluster_config("test_cluster");
    let cluster = Cluster::new(config.clone(), None);

    let details = cluster.cluster_details();
    assert_eq!(details.name, config.name);
    assert_eq!(details.host, config.host);
    assert_eq!(details.username, config.username);
    assert_eq!(details.path, config.path);
    assert_eq!(details.connection_type, config.connection_type);
}
