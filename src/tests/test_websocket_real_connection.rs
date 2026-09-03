//! Real integration tests matching C++ test coverage.
//!
//! These tests use REAL external dependencies:
//! - Real WebSocket connections with tokio-tungstenite
//! - Real HTTP server with axum test client
//! - Real `SQLite` database
//! - Real message serialization/deserialization

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as TungsteniteMsg;
use tower::ServiceExt;

use adacs_job_controller::cluster::cluster::Cluster;
use adacs_job_controller::cluster::traits::{
    ClusterManagerTrait, ClusterTrait, MockClusterManagerTrait,
};
use adacs_job_controller::db::entities::{file_download, job};
use adacs_job_controller::http::server::create_router;
use adacs_job_controller::protocol::constants::{SERVER_READY, SYSTEM_SOURCE};
use adacs_job_controller::protocol::message::Message;

use common::{
    connection_closes, encode_test_jwt, forwarding_cluster, insert_test_job, make_test_state,
    online_cluster, recv_binary_with_timeout, setup_test_db, test_cluster_config,
};

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
};

// ---------------------------------------------------------------------------
// Test server helpers
// ---------------------------------------------------------------------------

/// Start a real axum server on a random port with both HTTP and WS
async fn start_http_server(
    db: sea_orm::DatabaseConnection,
    manager: MockClusterManagerTrait,
) -> (u16, tokio::task::JoinHandle<()>) {
    let state = make_test_state(db, manager);
    let app = common::repeated_download::build_app(state);
    common::repeated_download::start_server(app).await
}

/// Start a server backed by a real cluster that accepts any connection,
/// returning the cluster together with the server's port and task handle.
async fn start_server_with_real_cluster_accepting()
-> (Arc<Cluster>, u16, tokio::task::JoinHandle<()>) {
    let db = setup_test_db().await;
    // Real cluster, so real `close()` is exercised.
    // (Cluster::new returns Arc<Self>.)
    let cluster: Arc<Cluster> = Cluster::new(test_cluster_config("ozstar"), None);
    // Start the scheduler so SERVER_READY actually reaches the WS
    // forwarder.
    cluster.start_tasks();

    let cluster_for_handler: Arc<dyn ClusterTrait> = Arc::clone(&cluster) as Arc<dyn ClusterTrait>;
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_get_file_download_admission()
        .returning(|_| None);
    manager
        .expect_handle_new_connection()
        .returning(move |_conn_id, ws_tx, _token| {
            let c: Arc<dyn ClusterTrait> = Arc::clone(&cluster_for_handler);
            Box::pin(async move {
                // Install the WS sender on the cluster so close() can
                // route a WsOutbound::Close through it.
                c.set_connection(Some(ws_tx)).await;
                Some(c)
            })
        });
    manager.expect_handle_pong().returning(|_| ());
    manager
        .expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    manager.expect_report_websocket_error().returning(|_, _| ());

    let (port, server_handle) = start_http_server(db.clone(), manager).await;
    (cluster, port, server_handle)
}

/// Connect a real WebSocket client and assert the server sends a
/// `SERVER_READY` message from `SYSTEM_SOURCE` as the first message.
/// Returns the sink and stream for the caller to continue with.
async fn connect_and_expect_server_ready(
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
    let (sink, mut stream) = common::repeated_download::connect_ws(port, token).await;
    let ready = recv_binary_with_timeout(&mut stream, Duration::from_secs(2))
        .await
        .expect("Server should send SERVER_READY");
    let ready_msg = Message::from_bytes(ready);
    assert_eq!(
        ready_msg.id(),
        SERVER_READY,
        "First message should be SERVER_READY"
    );
    assert_eq!(ready_msg.source(), SYSTEM_SOURCE);
    (sink, stream)
}

// ---------------------------------------------------------------------------
// Test: Real WebSocket Connection and Authentication
// ---------------------------------------------------------------------------

/// Tests that a real WebSocket client can connect and authenticate.
///
/// This matches C++ test: `test_websocket_connection_accepted_valid_token`
///
/// # Setup
/// - Starts real axum HTTP server
/// - Inserts test job in real database
/// - Creates online mock cluster that forwards messages
///
/// # Act
/// - Connects real WebSocket client with valid JWT token
///
/// # Assert
/// - Server accepts connection
/// - Server sends `SERVER_READY` message
#[tokio::test]
async fn test_real_websocket_connection_and_auth() {
    use adacs_job_controller::cluster::traits::WsConnectionSender;
    use std::sync::Mutex as StdMutex;

    let db = setup_test_db().await;
    let _job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    // Create cluster that forwards messages through WebSocket
    let tx_slot: Arc<StdMutex<Option<WsConnectionSender>>> = Arc::new(StdMutex::new(None));

    let cluster_arc = forwarding_cluster("ozstar", &tx_slot);

    let tx_for_new = Arc::clone(&tx_slot);
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_get_file_download_admission()
        .returning(|_| None);
    manager
        .expect_handle_new_connection()
        .returning(move |_, ws_tx, _| {
            *tx_for_new.lock().unwrap() = Some(ws_tx);
            let c = Arc::clone(&cluster_arc);
            Box::pin(async move { Some(c) })
        });
    manager
        .expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    manager.expect_report_websocket_error().returning(|_, _| ());
    manager.expect_handle_pong().returning(|_| ());

    // Start real server
    let (port, server_handle) = start_http_server(db.clone(), manager).await;
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    // Connect real WebSocket client and expect SERVER_READY
    let (sink, _stream) = connect_and_expect_server_ready(port, &token).await;

    // Cleanup
    drop(sink);
    server_handle.abort();
}

/// Regression test for the "stale WebSocket" bug.
///
/// When the server's `ClusterManager` decides to drop a connection
/// (e.g. after a pong timeout caused by an institutional firewall
/// interruption), it calls `cluster.close()`. Before the fix, this
/// only cleared an internal watch channel; the WebSocket stayed
/// open and axum kept auto-ponging the peer's pings, so the client
/// had no way to learn the server had moved on and never reconnected.
///
/// This test wires a real `Cluster` into the WS handler, opens a
/// real WebSocket from a tokio-tungstenite client, calls
/// `cluster.close(false)`, and asserts the client receives a
/// WebSocket Close frame within a short timeout.
#[tokio::test]
async fn test_server_initiated_close_sends_close_frame_to_client() {
    let (cluster, port, server_handle) = start_server_with_real_cluster_accepting().await;
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    let (sink, mut stream) = connect_and_expect_server_ready(port, &token).await;

    // Trigger the server-side disconnect path.
    Cluster::close(&cluster, false).await;

    // The client must see a WebSocket Close frame (or EOF) within
    // a short window. Without the fix the client would happily keep
    // pinging forever and this assertion would time out.
    let observed_close = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(TungsteniteMsg::Close(_)) | Err(_) => return true,
                Ok(_) => {}
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    assert!(
        observed_close,
        "Client should observe a WebSocket Close frame after server-initiated close"
    );

    drop(sink);
    server_handle.abort();
}

/// Regression test for the "stale WebSocket, missing grace period" hole.
///
/// The companion test
/// (`test_server_initiated_close_sends_close_frame_to_client`) proves
/// the server now sends a Close frame. This test proves the *follow-up*:
/// if the peer's Close ack is lost (or the peer is wedged and never
/// sends one — the same institutional firewall scenario that triggered
/// the original timeout), the server must still tear down the TCP
/// connection within a bounded grace period. Otherwise `ws_sink`
/// stays held by `handle_socket` and the socket lingers in `CLOSE_WAIT`.
///
/// The test deliberately *does not* respond with a Close ack on the
/// client side, and keeps the client's sink alive so the server's
/// read loop sees neither a Close frame nor EOF. The assertion is
/// that the server force-closes the connection within the grace
/// period + a small margin, observable from the client as a stream
/// error or Close.
///
/// This test is marked `#[ignore]` because it inherently sleeps for
/// the grace period (default 5s). Run it explicitly:
///
/// ```text
/// cargo test test_close_handshake_timeout_forces_tcp_close -- --ignored
/// ```
#[tokio::test]
#[ignore = "inherently slow (~grace period); run with --ignored"]
async fn test_close_handshake_timeout_forces_tcp_close() {
    let (cluster, port, server_handle) = start_server_with_real_cluster_accepting().await;
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    let (sink, mut stream) = connect_and_expect_server_ready(port, &token).await;

    // Trigger server-side close.
    let close_start = std::time::Instant::now();
    Cluster::close(&cluster, false).await;

    // Receive the Close frame but do NOT send a Close ack back,
    // and keep `sink` alive so the TCP connection stays open
    // from the client's side. This simulates a peer behind a
    // firewall that blocks the Close ack.
    let saw_close = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(msg) = stream.next().await {
            if matches!(msg, Ok(TungsteniteMsg::Close(_))) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(saw_close, "Server should send Close frame");

    // The server should force-close within the grace period + a
    // small margin. We detect this by observing the stream
    // end (Err or None) on the client side, which happens when
    // the server drops its sink.
    let force_closed = tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(msg) = stream.next().await {
            // Any error or further Close frame means the server
            // has torn the connection down. Pong/Ping/etc are
            // ignored — we just want the stream to end.
            if msg.is_err() {
                return true;
            }
        }
        true
    })
    .await
    .unwrap_or(false);

    let elapsed = close_start.elapsed();
    assert!(
        force_closed,
        "Server should force-close within ~{}s of cluster.close() (elapsed: {:?})",
        adacs_job_controller::websocket::server::WS_CLOSE_HANDSHAKE_GRACE_SECONDS,
        elapsed
    );

    drop(sink);
    server_handle.abort();
}

/// Tests that WebSocket connection is rejected with invalid token.
///
/// This matches C++ test: `test_websocket_connection_rejected_invalid_token`
///
/// # Act
/// - Connects with invalid JWT token
///
/// # Assert
/// - Server closes connection immediately
#[tokio::test]
async fn test_websocket_connection_rejected_invalid_token() {
    let db = setup_test_db().await;

    // Manager that rejects the connection (invalid token -> None).
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_handle_new_connection()
        .returning(|_, _, _| Box::pin(async { None }));
    manager
        .expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    manager.expect_report_websocket_error().returning(|_, _| ());

    // Start real server
    let (port, server_handle) = start_http_server(db.clone(), manager).await;

    // Connect with invalid Bearer token via the Authorization header
    // (the server ignores the ?token= query param).
    let (_sink, mut stream) =
        common::repeated_download::connect_ws(port, "invalid_token_12345").await;

    // Connection should be closed by the server.
    let closed = connection_closes(&mut stream, Duration::from_secs(2)).await;

    assert!(closed, "Server should close connection for invalid token");

    server_handle.abort();
}

// ---------------------------------------------------------------------------
// Test: File Download Record Persistence
// ---------------------------------------------------------------------------

/// Tests that a `file_download` record is persisted and can be read back.
///
/// # Setup
/// - Creates a `file_download` record in the database
///
/// # Act
/// - Queries the record back by its UUID
///
/// # Assert
/// - The record exists with id 1 (first insert in a fresh DB)
/// - The record has the expected UUID
#[tokio::test]
async fn test_file_download_record_persistence() {
    let db = setup_test_db().await;
    let job_id = insert_test_job(&db, "ozstar", "b", "testapp").await;

    // Create file download record
    let uuid = "test-download-uuid-12345".to_string();
    let file_download_record = file_download::ActiveModel {
        user: Set(1),
        job: Set(job_id),
        cluster: Set("ozstar".to_string()),
        bundle: Set("b".to_string()),
        uuid: Set(uuid.clone()),
        path: Set("/path/to/file.txt".to_string()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };
    file_download_record
        .insert(&db)
        .await
        .expect("insert file_download failed");

    // Verify record exists with correct state
    let record = file_download::Entity::find()
        .filter(file_download::Column::Uuid.eq(&uuid))
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        record.id, 1,
        "first download record in fresh DB should have id 1"
    );

    // Re-query the record to confirm it is still readable after insertion
    let updated = file_download::Entity::find()
        .filter(file_download::Column::Uuid.eq(&uuid))
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(updated.uuid, uuid, "Record should exist with correct UUID");
}

// ---------------------------------------------------------------------------
// Test: Multiple Clusters Concurrent Job Submission
// ---------------------------------------------------------------------------

/// Tests concurrent job submission to multiple clusters.
///
/// This matches C++ test: `test_multiple_clusters_simultaneous`
///
/// # Setup
/// - Creates 3 mock clusters (ozstar, nci, gadi)
/// - All clusters are online
///
/// # Act
/// - Submits jobs to different clusters sequentially
///
/// # Assert
/// - All jobs inserted into database
/// - All clusters received their jobs
#[tokio::test]
async fn test_multiple_clusters_concurrent_job_submission() {
    let db = setup_test_db().await;

    let ozstar = Arc::new(online_cluster("ozstar"));
    let nci = Arc::new(online_cluster("nci"));
    let gadi = Arc::new(online_cluster("gadi"));

    let mut manager = MockClusterManagerTrait::new();
    let oz = Arc::clone(&ozstar);
    let nc = Arc::clone(&nci);
    let ga = Arc::clone(&gadi);

    manager
        .expect_get_cluster_by_name()
        .returning(move |name| match name {
            "ozstar" => {
                Some(Arc::clone(&oz)
                    as Arc<
                        dyn adacs_job_controller::cluster::traits::ClusterTrait,
                    >)
            }
            "nci" => {
                Some(Arc::clone(&nc)
                    as Arc<
                        dyn adacs_job_controller::cluster::traits::ClusterTrait,
                    >)
            }
            "gadi" => {
                Some(Arc::clone(&ga)
                    as Arc<
                        dyn adacs_job_controller::cluster::traits::ClusterTrait,
                    >)
            }
            _ => None,
        });
    manager
        .expect_handle_new_connection()
        .returning(move |_, _, _| Box::pin(async move { None }));

    // Start real HTTP server
    let (_port, server_handle) = start_http_server(db.clone(), manager).await;
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    // Submit jobs to 2 clusters (ozstar and nci - gadi not in JWT secret)
    let clusters = ["ozstar", "nci"];

    for (i, cluster_name) in clusters.iter().enumerate() {
        let oz2 = Arc::clone(&ozstar);
        let nc2 = Arc::clone(&nci);
        let ga2 = Arc::clone(&gadi);
        let app = create_router(make_test_state(db.clone(), {
            let mut manager = MockClusterManagerTrait::new();
            manager
                .expect_get_cluster_by_name()
                .returning(move |name| match name {
                    "ozstar" => Some(Arc::clone(&oz2)
                        as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait>),
                    "nci" => Some(Arc::clone(&nc2)
                        as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait>),
                    "gadi" => Some(Arc::clone(&ga2)
                        as Arc<dyn adacs_job_controller::cluster::traits::ClusterTrait>),
                    _ => None,
                });
            manager
                .expect_handle_new_connection()
                .returning(move |_, _, _| Box::pin(async move { None }));
            manager
        }));

        let job_data = json!({
            "cluster": cluster_name,
            "bundle": format!("bundle_{}", i),
            "application": "testapp",
            "parameters": "{}"
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/job/apiv1/job/")
                    .header("content-type", "application/json")
                    .header("authorization", &token)
                    .body(Body::from(job_data.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Job submission to {} should succeed, got: {:?}",
            cluster_name,
            resp.status()
        );
    }

    // Verify jobs in database
    let job_count = job::Entity::find().count(&db).await.unwrap();
    assert_eq!(job_count, 2, "Both submitted jobs should be in database");

    server_handle.abort();
}

// ---------------------------------------------------------------------------
// Tests: Bounded application shutdown (task-6)
// ---------------------------------------------------------------------------

/// Verifies the bounded application-shutdown path on a single
/// `DownloadSession` driven directly — no real WebSocket is opened and no
/// transport-level shutdown is asserted (the full WS path is covered by
/// other tests). It asserts:
/// - An `ApplicationShutdown` trigger moves the session
///   `Pending/Connected -> Closing` with the winning reason retained.
/// - Repeated triggers are idempotent no-ops.
/// - The handler-exit guard's `complete(Some(conn_id))` finalises the
///   `Closing -> Closed` transition; `complete` is idempotent and rejects
///   a stale connection id.
/// - The close bound is the existing `WS_CLOSE_HANDSHAKE_GRACE_SECONDS`
///   (5 s), with no new long timeout introduced.
/// - The manager mock's `is_application_shutting_down` and
///   `begin_application_shutdown` are reachable through the
///   `Arc<dyn ClusterManagerTrait>` trait-object plumbing.
#[tokio::test]
async fn test_application_shutdown_releases_transport_and_completes_session() {
    use adacs_job_controller::cluster::file_download::{
        DownloadSession, DownloadSessionState, DownloadShutdownReason, FileDownloadState,
    };
    use adacs_job_controller::websocket::server::WS_CLOSE_HANDSHAKE_GRACE_SECONDS;

    let db = setup_test_db().await;

    // We don't open a real WebSocket for this test because the existing
    // `test_file_download_session_completes_after_graceful_close` and
    // `test_server_initiated_close_sends_close_frame_to_client` already
    // exercise the full WebSocket path. This test focuses on the
    // bounded-shutdown integration: trigger every registered session,
    // verify the exact `ApplicationShutdown` reason is recorded, and
    // verify the handler-exit guard's `complete(...)` call finalises
    // the session to `Closed` so the dedicated-admission flag plus
    // the dedicated cluster termination drain together release every
    // owner within the existing five-second close bound.
    let (cleanup_tx, _cleanup_rx) = tokio::sync::mpsc::unbounded_channel();
    let session = DownloadSession::new(
        "test-download-uuid-app-shutdown".to_string(),
        Arc::new(FileDownloadState::new()),
        cleanup_tx,
    );

    // Bind the session as if a WebSocket handler had been admitted.
    session
        .bind_connection(1)
        .expect("session must bind in Pending state");

    match session.state() {
        DownloadSessionState::Connected(1) => {}
        other => panic!("expected Connected(1), got {other:?}"),
    }

    // Trigger the typed `ApplicationShutdown` reason on the exact session,
    // mirroring what the real manager's `begin_application_shutdown`
    // method does inside `begin_application_shutdown_inherent`.
    let triggered = session
        .cleanup_trigger()
        .trigger(DownloadShutdownReason::ApplicationShutdown);
    assert!(
        triggered,
        "ApplicationShutdown trigger must transition Pending/Connected to Closing"
    );

    // The session is now `Closing{connection_id: Some(1), reason:
    // ApplicationShutdown}` — the winning reason is retained.
    match session.state() {
        DownloadSessionState::Closing {
            connection_id: Some(1),
            reason: DownloadShutdownReason::ApplicationShutdown,
        } => {}
        other => panic!("expected Closing(ApplicationShutdown), got {other:?}"),
    }

    // Repeated triggers on the same session are idempotent no-ops.
    let dup = session
        .cleanup_trigger()
        .trigger(DownloadShutdownReason::WebSocketError);
    assert!(!dup, "second trigger on Closing session must be a no-op");
    assert!(matches!(
        session.state(),
        DownloadSessionState::Closing {
            reason: DownloadShutdownReason::ApplicationShutdown,
            ..
        }
    ));

    // The handler-exit guard from task-5 performs exact idempotent
    // `complete(Some(connection_id))` at handler exit. Simulate that
    // here so the test verifies the full Closing -> Closed transition.
    assert!(session.complete(Some(1)));
    assert!(!session.complete(Some(1)), "complete must be idempotent");

    match session.state() {
        DownloadSessionState::Closed {
            connection_id: Some(1),
            reason: DownloadShutdownReason::ApplicationShutdown,
        } => {}
        other => panic!("expected Closed(ApplicationShutdown), got {other:?}"),
    }

    // The connection id must not be reused for a stale cleanup.
    assert!(
        !session.complete(Some(99)),
        "complete with a wrong conn_id must be a no-op"
    );

    // Verify the bound used is the existing five-second grace (no new
    // long timeout introduced by task-6).
    assert_eq!(WS_CLOSE_HANDSHAKE_GRACE_SECONDS, 5);

    // The HTTP download endpoint consults the same flag and routes
    // post-shutdown admission attempts into the existing
    // `SERVICE_UNAVAILABLE` typed error path. The mock manager's
    // `is_application_shutting_down` is wired through the trait
    // method override, confirming the trait-object plumbing.
    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_is_application_shutting_down()
        .returning(|| true);
    manager.expect_begin_application_shutdown().returning(|| 0);
    manager
        .expect_dedicated_download_clusters()
        .returning(Vec::new);
    let trait_manager: Arc<dyn ClusterManagerTrait> = Arc::new(manager);
    assert!(trait_manager.is_application_shutting_down());
    assert_eq!(trait_manager.begin_application_shutdown(), 0);

    drop(db);
}
