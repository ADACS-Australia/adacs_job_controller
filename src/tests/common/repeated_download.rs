//! Shared fixtures for repeated file-download resource regression tests.
#![allow(clippy::doc_markdown)]

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as TungsteniteMsg;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use adacs_job_controller::app::AppState;
use adacs_job_controller::cluster::manager::ClusterManager;
use adacs_job_controller::cluster::traits::ClusterManagerTrait;
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::db::entities::{file_download, job};
use adacs_job_controller::http::server::create_router;
use adacs_job_controller::protocol::constants::{FILE_CHUNK, FILE_DETAILS};
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::FileListState;
use adacs_job_controller::protocol::types::Priority;

use super::test_jwt_secrets;

/// Test cluster config used for the repeated-download regression suite.
pub fn regression_cluster_config() -> ClusterConfig {
    ClusterConfig {
        name: "regression_cluster".to_string(),
        host: "127.0.0.1".to_string(),
        username: "regression".to_string(),
        path: "/tmp/regression".to_string(),
        key: String::new(),
        connection_type: "manual".to_string(),
        keytab: String::new(),
        kerberos_principal: String::new(),
        ltk: None,
    }
}

/// Build a real `ClusterManager` with one online manual master cluster.
pub async fn fresh_manager(db: &DatabaseConnection) -> Arc<ClusterManager> {
    let file_list_map = Arc::new(DashMap::new());
    let manager = ClusterManager::new(vec![regression_cluster_config()], db.clone(), file_list_map);

    let master = manager
        .get_cluster_by_name("regression_cluster")
        .expect("manager should expose master cluster");
    let (dummy_tx, _dummy_rx) =
        tokio::sync::mpsc::unbounded_channel::<adacs_job_controller::cluster::traits::WsOutbound>();
    master.set_connection(Some(dummy_tx)).await;
    manager.start_tasks();
    manager
}

/// Insert a fresh test job and return its id.
pub async fn insert_regression_job(db: &DatabaseConnection) -> i64 {
    job::ActiveModel {
        user: Set(1),
        parameters: Set("{}".to_string()),
        cluster: Set("regression_cluster".to_string()),
        bundle: Set("b".to_string()),
        application: Set("testapp".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert regression job failed")
    .id
}

/// Create a fresh `file_download` DB record for `file_id`.
pub async fn insert_regression_file_download(db: &DatabaseConnection, file_id: &str) {
    file_download::ActiveModel {
        user: Set(1),
        job: Set(0),
        cluster: Set("regression_cluster".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(file_id.to_string()),
        path: Set("/result/file.bin".to_string()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert regression file_download failed");
}

/// Build the full axum app (HTTP + WS) backed by the supplied `AppState`.
pub fn build_app(state: AppState) -> Router {
    let mut app = create_router(state.clone());
    app = app.merge(
        Router::new()
            .route(
                "/job/ws/",
                axum::routing::get(adacs_job_controller::websocket::server::ws_handler),
            )
            .with_state(state),
    );
    app
}

/// Start a real axum server on a random port with both HTTP and WS routes.
pub async fn start_server(app: Router) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (port, handle)
}

/// Connect a real tokio-tungstenite WebSocket client to the test server.
pub async fn connect_ws(
    port: u16,
    token: &str,
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
    let url = format!("ws://127.0.0.1:{port}/job/ws/");
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let (ws_stream, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    ws_stream.split()
}

/// Send a binary protocol Message over the WS sink.
pub async fn send_msg(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        TungsteniteMsg,
    >,
    msg: Message,
) {
    sink.send(TungsteniteMsg::Binary(msg.into_data().into()))
        .await
        .unwrap();
}

/// Build a `FILE_DETAILS` message announcing `file_size` bytes.
pub fn build_file_details(file_size: u64) -> Message {
    let mut msg = Message::new(FILE_DETAILS, Priority::Highest, "regression_peer");
    msg.push_ulong(file_size);
    msg
}

/// Build a `FILE_CHUNK` message carrying `bytes`.
pub fn build_file_chunk(bytes: &[u8]) -> Message {
    let mut msg = Message::new(FILE_CHUNK, Priority::Highest, "regression_peer");
    msg.push_bytes(bytes);
    msg
}

/// Wait for dedicated-download observable state to return to baseline.
pub async fn wait_for_cleanup(manager: &ClusterManager, deadline: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        let clusters = manager.dedicated_download_clusters_concrete();
        let all_drained = clusters
            .iter()
            .all(|c| c.retained_download_task_count() == 0);
        if all_drained && manager.dedicated_download_clusters().is_empty() {
            return true;
        }
        tokio::task::yield_now().await;
    }
    false
}

/// Build an `AppState` backed by a real `ClusterManager`.
pub fn build_state(
    db: DatabaseConnection,
    manager: Arc<ClusterManager>,
    file_list_map: Arc<DashMap<String, Arc<tokio::sync::Mutex<FileListState>>>>,
    client_timeout_seconds: Option<u64>,
) -> AppState {
    AppState {
        db,
        cluster_manager: manager as Arc<dyn ClusterManagerTrait>,
        file_list_map,
        jwt_secrets: Arc::new(test_jwt_secrets()),
        client_timeout_seconds,
    }
}
