//! Shared test helpers for integration tests.
#![allow(dead_code)]

pub mod repeated_download;

use std::sync::Mutex as StdMutex;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message as TungsteniteMsg;

use adacs_job_controller::cluster::cluster::{AppContext, Cluster};
use adacs_job_controller::cluster::traits::{
    ClusterTrait, MockClusterManagerTrait, MockClusterTrait, WsConnectionSender, WsOutbound,
};
use adacs_job_controller::config::access_secrets::AccessSecret;
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::ClusterRole;

/// Build the WebSocket-only router for WS integration tests.
pub fn ws_router(state: adacs_job_controller::app::AppState) -> axum::Router {
    axum::Router::new()
        .route(
            "/job/ws/",
            axum::routing::get(adacs_job_controller::websocket::server::ws_handler),
        )
        .with_state(state)
}

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

/// Create an online `Cluster` with a live WS sender and a started scheduler,
/// returning the cluster and the outbound receiver.
pub async fn make_online_cluster(
    name: &str,
    ctx: Option<Arc<AppContext>>,
) -> (
    Arc<Cluster>,
    tokio::sync::mpsc::UnboundedReceiver<WsOutbound>,
) {
    let cluster = Cluster::new(test_cluster_config(name), ctx);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx)).await;
    cluster.start_tasks();
    (cluster, rx)
}

/// Build a mock online cluster that never sends messages.
pub fn online_cluster_no_messages() -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    c.expect_name().returning(|| "ozstar".to_string());
    c.expect_is_online().returning(|| true);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    c.expect_send_message().returning(|_| Box::pin(async {}));
    c
}

/// Build a mock online cluster with the given name that forwards messages.
pub fn online_cluster(name: &str) -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    let name = name.to_string();
    let details_name = name.clone();
    c.expect_name().returning(move || name.clone());
    c.expect_is_online().returning(|| true);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(move || test_cluster_config(&details_name));
    c.expect_send_message().returning(|_| Box::pin(async {}));
    c
}

/// Build a mock cluster that captures all `send_message` calls.
pub fn mock_cluster_capturing(name: &str) -> (MockClusterTrait, Arc<Mutex<Vec<Message>>>) {
    let sent = Arc::new(Mutex::new(Vec::<Message>::new()));
    let sent_clone = Arc::clone(&sent);

    let mut mock = MockClusterTrait::new();
    let n = name.to_string();
    mock.expect_name().returning(move || n.clone());
    mock.expect_send_message().returning(move |msg| {
        sent_clone.lock().unwrap().push(msg);
        Box::pin(async {})
    });

    (mock, sent)
}

/// Build a mock offline cluster.
pub fn offline_cluster() -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    c.expect_name().returning(|| "ozstar".to_string());
    c.expect_is_online().returning(|| false);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    c
}

/// Build a mock cluster whose `send_message` forwards via the captured WS sender.
///
/// `tx_slot` is filled in by the manager's `handle_new_connection` before any
/// message is forwarded, so the cluster can reach the client's WS channel.
pub fn forwarding_cluster(
    name: &str,
    tx_slot: &Arc<StdMutex<Option<WsConnectionSender>>>,
) -> Arc<dyn ClusterTrait> {
    let tx_for_send = Arc::clone(tx_slot);
    let mut cluster = MockClusterTrait::new();
    let n = name.to_string();
    cluster.expect_name().returning(move || n.clone());
    cluster
        .expect_role_string()
        .returning(|| "master test".to_string());
    cluster.expect_is_online().returning(|| true);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster
        .expect_cluster_details()
        .returning(|| test_cluster_config("test"));
    cluster.expect_send_message().returning(move |msg| {
        if let Some(tx) = tx_for_send.lock().unwrap().as_ref() {
            let _ = tx.send(WsOutbound::Binary(msg.into_data()));
        }
        Box::pin(async {})
    });
    cluster
        .expect_handle_message()
        .returning(|_| Box::pin(async {}));

    Arc::new(cluster)
}

/// Build a mock upload cluster ("ozstar-up") that ignores sent messages and
/// always reports a successful queue drain.
pub fn upload_cluster() -> MockClusterTrait {
    let mut c = MockClusterTrait::new();
    c.expect_name().returning(|| "ozstar-up".to_string());
    c.expect_is_online().returning(|| true);
    c.expect_role().returning(|| ClusterRole::Master);
    c.expect_role_string().returning(|| "master".to_string());
    c.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    c.expect_send_message().returning(|_| Box::pin(async {}));
    c.expect_wait_for_queue_drain()
        .returning(|_| Box::pin(async { true }));
    c
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

/// Build a mock `ClusterManagerTrait` that returns None for all cluster lookups.
pub fn mock_cluster_manager_no_clusters() -> MockClusterManagerTrait {
    let mut mock_manager = MockClusterManagerTrait::new();
    mock_manager
        .expect_get_file_download_admission()
        .returning(|_| None);
    mock_manager
        .expect_get_cluster_by_name()
        .returning(|_| None);
    mock_manager.expect_get_file_download().returning(|_| None);
    mock_manager.expect_get_file_upload().returning(|_| None);
    mock_manager
}

/// Build a mock `ClusterManagerTrait` wired to a single online cluster that never sends messages.
pub fn manager_with_online_cluster_no_messages() -> MockClusterManagerTrait {
    let cluster = Arc::new(online_cluster_no_messages());
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    manager
}

// ---------------------------------------------------------------------------
// WebSocket test helpers
// ---------------------------------------------------------------------------

/// Check whether a WS connection was closed (Close frame / transport error / EOF)
/// within the given timeout.
pub async fn connection_closes(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    timeout: std::time::Duration,
) -> bool {
    use futures_util::StreamExt;

    let result = tokio::time::timeout(timeout, async {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_) => return true,
                _ => {}
            }
        }
        true // stream ended
    })
    .await;

    result.unwrap_or(false)
}

// ---------------------------------------------------------------------------
// SQLite test database helpers
// ---------------------------------------------------------------------------

/// Build an `Arc<AppContext>` wrapping the given database connection.
pub fn make_app_context(db: sea_orm::DatabaseConnection) -> Arc<AppContext> {
    Arc::new(AppContext {
        db,
        file_list_map: std::sync::Arc::new(dashmap::DashMap::new()),
    })
}

/// Create a fresh in-memory `SQLite` database connection (no schema).
pub async fn make_db() -> sea_orm::DatabaseConnection {
    sea_orm::Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connect failed")
}

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
        client_timeout_seconds: None,
    }
}

/// Build an HTTP router wired to an online cluster mock and a fresh test DB.
///
/// Returns `(app, db, token)`: the fully-wired router, the fresh in-memory
/// test database, and a valid JWT for `testapp`.
pub async fn make_app_with_online_cluster() -> (axum::Router, sea_orm::DatabaseConnection, String) {
    let db = setup_test_db().await;

    let cluster_arc = std::sync::Arc::new(online_cluster_no_messages());

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(move |_| {
        Some(std::sync::Arc::clone(&cluster_arc)
            as std::sync::Arc<
                dyn adacs_job_controller::cluster::traits::ClusterTrait,
            >)
    });
    manager
        .expect_handle_new_connection()
        .returning(move |_, _, _| Box::pin(async move { None }));

    let app =
        adacs_job_controller::http::server::create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 1, "application": "testapp"}));

    (app, db, token)
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

/// Insert a job with an explicit ID (used to exercise the `u32::MAX` conversion guard).
pub async fn insert_test_job_with_id(
    db: &sea_orm::DatabaseConnection,
    id: i64,
    cluster: &str,
    bundle: &str,
    application: &str,
) -> i64 {
    use adacs_job_controller::db::entities::job;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    job::ActiveModel {
        id: Set(id),
        user: Set(1),
        parameters: Set("{}".to_string()),
        cluster: Set(cluster.to_string()),
        bundle: Set(bundle.to_string()),
        application: Set(application.to_string()),
    }
    .insert(db)
    .await
    .expect("insert test job with id failed")
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
        client_timeout_seconds: None,
    }
}

// ---------------------------------------------------------------------------
// WebSocket test helpers
// ---------------------------------------------------------------------------

/// Connect a tokio-tungstenite WebSocket client to the given URL.
pub async fn connect_ws(
    url: &str,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        TungsteniteMsg,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    let (ws_stream, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    ws_stream.split()
}

/// Read the first binary message from the WS stream, with a 500ms timeout.
pub async fn recv_binary(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Option<Vec<u8>> {
    recv_binary_with_timeout(stream, std::time::Duration::from_millis(500)).await
}

/// Read the first binary message from the WS stream, with a configurable timeout.
pub async fn recv_binary_with_timeout(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    timeout: std::time::Duration,
) -> Option<Vec<u8>> {
    tokio::time::timeout(timeout, async {
        while let Some(msg) = stream.next().await {
            if let Ok(TungsteniteMsg::Binary(data)) = msg {
                return Some(data.to_vec());
            }
        }
        None
    })
    .await
    .unwrap_or(None)
}
