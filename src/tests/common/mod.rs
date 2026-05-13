//! Shared test helpers for integration tests.
#![allow(dead_code)]

use std::sync::Arc;

use adacs_job_controller::cluster::traits::{MockClusterManagerTrait, MockClusterTrait};
use adacs_job_controller::config::access_secrets::AccessSecret;
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::protocol::types::ClusterRole;

/// Create a test `ClusterConfig` with reasonable defaults.
pub fn test_cluster_config(name: &str) -> ClusterConfig {
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

/// Create a test `ClusterConfig` with LTK configured.
pub fn test_cluster_config_with_ltk(name: &str, ltk: &str) -> ClusterConfig {
    ClusterConfig {
        name: name.to_string(),
        host: "localhost".to_string(),
        username: "testuser".to_string(),
        path: "/home/testuser/jobcontroller".to_string(),
        key: String::new(),
        connection_type: "manual".to_string(),
        keytab: String::new(),
        kerberos_principal: String::new(),
        ltk: Some(ltk.to_string()),
    }
}

/// Create test JWT secrets for HTTP handler tests.
pub fn test_jwt_secrets() -> Vec<AccessSecret> {
    vec![AccessSecret {
        name: "testapp".to_string(),
        secret: "test_secret_key_12345".to_string(),
        applications: vec!["bilby".to_string()],
        clusters: vec!["ozstar".to_string(), "nci".to_string()],
    }]
}

/// Encode a JWT token using the test secret.
pub fn encode_test_jwt(claims: &serde_json::Value) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(test_jwt_secrets()[0].secret.as_bytes()),
    )
    .expect("failed to encode test JWT")
}

/// Build a mock `ClusterManagerTrait` with a default `get_cluster_by_name` expectation
/// that returns a mock cluster for `cluster_name`.
pub fn mock_cluster_manager_with_online_cluster(
    cluster_name: &str,
) -> (MockClusterManagerTrait, Arc<MockClusterTrait>) {
    let mut mock_cluster = MockClusterTrait::new();
    let name = cluster_name.to_string();
    mock_cluster.expect_name().returning(move || name.clone());
    mock_cluster.expect_is_online().returning(|| true);
    mock_cluster.expect_role().returning(|| ClusterRole::Master);
    mock_cluster
        .expect_role_string()
        .returning(|| "master test".to_string());
    mock_cluster
        .expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    mock_cluster
        .expect_send_message()
        .returning(|_| Box::pin(async {}));

    let cluster_arc: Arc<MockClusterTrait> = Arc::new(mock_cluster);
    let cluster_for_closure = Arc::clone(&cluster_arc);

    let mut mock_manager = MockClusterManagerTrait::new();
    mock_manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(cluster_for_closure.clone()));

    (mock_manager, cluster_arc)
}

/// Build a mock `ClusterManagerTrait` that returns None for all cluster lookups.
pub fn mock_cluster_manager_no_clusters() -> MockClusterManagerTrait {
    let mut mock_manager = MockClusterManagerTrait::new();
    mock_manager
        .expect_get_cluster_by_name()
        .returning(|_| None);
    mock_manager.expect_get_file_download().returning(|_| None);
    mock_manager.expect_get_file_upload().returning(|_| None);
    mock_manager
}

// ---------------------------------------------------------------------------
// SQLite test database helpers
// ---------------------------------------------------------------------------

/// Create a fresh in-memory `SQLite` database with all HTTP handler tables.
pub async fn setup_test_db() -> sea_orm::DatabaseConnection {
    let db = sea_orm::Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connect failed");
    adacs_job_controller::db::schema::create_test_schema(&db).await;
    db
}

/// Build a test `AppState` with a real `SQLite` DB and the given cluster manager.
pub fn make_test_state(
    db: sea_orm::DatabaseConnection,
    manager: MockClusterManagerTrait,
) -> adacs_job_controller::app::AppState {
    adacs_job_controller::app::AppState {
        db,
        cluster_manager: std::sync::Arc::new(manager),
        file_list_map: std::sync::Arc::new(dashmap::DashMap::new()),
        jwt_secrets: std::sync::Arc::new(test_jwt_secrets()),
    }
}

/// Insert a job into the test database. Returns the inserted job id.
pub async fn insert_test_job(
    db: &sea_orm::DatabaseConnection,
    cluster: &str,
    bundle: &str,
    application: &str,
) -> i64 {
    use adacs_job_controller::db::entities::job;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    job::ActiveModel {
        user: Set(1),
        parameters: Set("{}".to_string()),
        cluster: Set(cluster.to_string()),
        bundle: Set(bundle.to_string()),
        application: Set(application.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert test job failed")
    .id
}

/// Insert a job history record with the given state.
pub async fn insert_job_history(
    db: &sea_orm::DatabaseConnection,
    job_id: i64,
    state_val: i32,
    what: &str,
) {
    insert_job_history_at(db, job_id, state_val, what, chrono::Utc::now().naive_utc()).await;
}

/// Insert a job history record with an explicit timestamp.
pub async fn insert_job_history_at(
    db: &sea_orm::DatabaseConnection,
    job_id: i64,
    state_val: i32,
    what: &str,
    timestamp: chrono::NaiveDateTime,
) {
    use adacs_job_controller::db::entities::job_history;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    job_history::ActiveModel {
        job_id: Set(job_id),
        timestamp: Set(timestamp),
        what: Set(what.to_string()),
        state: Set(state_val),
        details: Set("test".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert job history failed");
}

// ---------------------------------------------------------------------------
// Multi-secret JWT helpers for cross-app access tests
// ---------------------------------------------------------------------------

/// Create 4 JWT secrets for testing:
///
/// - `secret 0` (app1): owns clusters `["ozstar", "nci"]`, can also access `"bilby"`
/// - `secret 1` (app2): owns cluster `["ozstar"]`, can access app1's jobs  (applications: `["app1"]`)
/// - `secret 2` (app3): owns cluster `["ozstar"]`, no cross-app access
/// - `secret 3` (app4): no cluster access, no cross-app access
pub fn test_jwt_secrets_multi() -> Vec<AccessSecret> {
    vec![
        AccessSecret {
            name: "app1".to_string(),
            secret: "secret_app1".to_string(),
            applications: vec!["bilby".to_string()],
            clusters: vec!["ozstar".to_string(), "nci".to_string()],
        },
        AccessSecret {
            name: "app2".to_string(),
            secret: "secret_app2".to_string(),
            applications: vec!["app1".to_string()],
            clusters: vec!["ozstar".to_string()],
        },
        AccessSecret {
            name: "app3".to_string(),
            secret: "secret_app3".to_string(),
            applications: vec![],
            clusters: vec!["ozstar".to_string()],
        },
        AccessSecret {
            name: "app4".to_string(),
            secret: "secret_app4".to_string(),
            applications: vec![],
            clusters: vec![],
        },
    ]
}

/// Encode a JWT token for a specific secret from `test_jwt_secrets_multi()`.
pub fn encode_jwt_for_secret(secret: &AccessSecret, claims: &serde_json::Value) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret.secret.as_bytes()),
    )
    .expect("failed to encode JWT")
}

/// RAII guard for a test server that aborts the server task on drop.
pub struct TestServer {
    pub port: u16,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl TestServer {
    pub fn new(port: u16, handle: tokio::task::JoinHandle<()>) -> Self {
        Self { port, handle }
    }
}

/// Build a test `AppState` with a custom list of JWT secrets.
pub fn make_test_state_with_secrets(
    db: sea_orm::DatabaseConnection,
    manager: adacs_job_controller::cluster::traits::MockClusterManagerTrait,
    secrets: Vec<AccessSecret>,
) -> adacs_job_controller::app::AppState {
    adacs_job_controller::app::AppState {
        db,
        cluster_manager: std::sync::Arc::new(manager),
        file_list_map: std::sync::Arc::new(dashmap::DashMap::new()),
        jwt_secrets: std::sync::Arc::new(secrets),
    }
}
