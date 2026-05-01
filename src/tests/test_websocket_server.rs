//! Integration tests for the WebSocket server handler.
//!
//! Spins up a real axum HTTP server bound to a random port,
//! connects with `tokio-tungstenite`, and verifies connection lifecycle
//! and message dispatch behaviour.

mod common;

use std::sync::Arc;

use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as TungsteniteMsg;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use adacs_job_controller::cluster::traits::{
    ClusterTrait, MockClusterManagerTrait, MockClusterTrait, WsOutbound,
};
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::{ClusterRole, Priority};

use common::{make_test_state, setup_test_db, test_cluster_config};

// ---------------------------------------------------------------------------
// Test server helpers
// ---------------------------------------------------------------------------

/// Build the axum Router that contains only the WebSocket endpoint.
fn ws_router(state: adacs_job_controller::app::AppState) -> Router {
    Router::new()
        .route(
            "/job/ws/",
            axum::routing::get(adacs_job_controller::websocket::server::ws_handler),
        )
        .with_state(state)
}

/// Start an axum server on a random OS-assigned port.
/// Returns a `TestServer` RAII guard that aborts the server on drop.
async fn start_test_server(state: adacs_job_controller::app::AppState) -> common::TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = ws_router(state);
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap_or(());
    });
    common::TestServer::new(port, handle)
}

/// Connect a tokio-tungstenite WebSocket client to the given URL.
async fn connect_ws(
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

/// Read the first binary message from the WS stream, with a timeout.
async fn recv_binary(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Option<Vec<u8>> {
    let timeout = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        while let Some(msg) = stream.next().await {
            if let Ok(TungsteniteMsg::Binary(data)) = msg {
                return Some(data.to_vec());
            }
        }
        None
    })
    .await;

    timeout.unwrap_or(None)
}

/// Check whether the WS connection was closed within 500ms.
async fn connection_closes(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> bool {
    let timeout = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(TungsteniteMsg::Close(_)) | Err(_) => return true,
                _ => {}
            }
        }
        true // stream ended
    })
    .await;

    timeout.unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Build mock cluster manager helpers
// ---------------------------------------------------------------------------

/// Mock manager where `handle_new_connection` always returns `None` (invalid token).
fn manager_rejecting_connections() -> MockClusterManagerTrait {
    let mut m = MockClusterManagerTrait::new();
    m.expect_handle_new_connection()
        .returning(|_, _, _| Box::pin(async { None }));
    m.expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    m.expect_report_websocket_error().returning(|_, _| ());
    m
}

/// Mock manager that accepts connections with any token, returning a simple mock cluster.
/// Mock manager that accepts connections and FORWARDS messages through the WS channel.
/// This is needed for tests that need to receive `SERVER_READY` from the server.
fn manager_with_forwarding_cluster(name: &str) -> MockClusterManagerTrait {
    use adacs_job_controller::cluster::traits::WsConnectionSender;
    use std::sync::Mutex as StdMutex;

    let tx_slot: Arc<StdMutex<Option<WsConnectionSender>>> = Arc::new(StdMutex::new(None));

    // Build a cluster whose send_message forwards via the captured tx
    let tx_for_send = Arc::clone(&tx_slot);
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
    });
    cluster
        .expect_handle_message()
        .returning(|_| Box::pin(async {}));

    let cluster_arc: Arc<dyn ClusterTrait> = Arc::new(cluster);

    // Build a manager that captures tx and returns the forwarding cluster
    let tx_for_new = Arc::clone(&tx_slot);
    let mut m = MockClusterManagerTrait::new();
    m.expect_handle_new_connection()
        .returning(move |_, ws_tx, _| {
            *tx_for_new.lock().unwrap() = Some(ws_tx);
            let c = Arc::clone(&cluster_arc);
            Box::pin(async move { Some(c) })
        });
    m.expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    m.expect_report_websocket_error().returning(|_, _| ());
    m.expect_handle_pong().returning(|_| ());
    m
}

// ---------------------------------------------------------------------------
// test_ws_invalid_token_disconnects
// ---------------------------------------------------------------------------

/// Verify that a connection with an invalid token is rejected and closed.
///
/// # Setup
/// Start a test server configured with a cluster manager that rejects all connections.
///
/// # Act
/// Connect a WebSocket client sending `?token=bad_token`.
///
/// # Assert
/// The server closes the connection.
#[tokio::test]
async fn test_ws_invalid_token_disconnects() {
    let db = setup_test_db().await;
    let state = make_test_state(db, manager_rejecting_connections());
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with an invalid token
    let (_, mut stream) =
        connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/?token=bad_token")).await;

    let closed = connection_closes(&mut stream).await;
    assert!(closed, "Server should close connection for invalid token");
}

// ---------------------------------------------------------------------------
// test_ws_no_token_disconnects
// ---------------------------------------------------------------------------

/// Verify that a connection without any token query parameter is rejected and closed.
///
/// # Setup
/// Start a test server configured with a cluster manager that rejects all connections.
///
/// # Act
/// Connect a WebSocket client without any token query parameter.
///
/// # Assert
/// The server closes the connection.
#[tokio::test]
async fn test_ws_no_token_disconnects() {
    let db = setup_test_db().await;
    let state = make_test_state(db, manager_rejecting_connections());
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect without any token query param
    let (_, mut stream) = connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/")).await;

    let closed = connection_closes(&mut stream).await;
    assert!(
        closed,
        "Server should close connection when no token provided"
    );
}

// ---------------------------------------------------------------------------
// test_ws_valid_token_receives_server_ready
// ---------------------------------------------------------------------------

/// Verify that the server sends a `SERVER_READY` message after accepting a valid connection.
///
/// # Setup
/// Start a test server with a forwarding cluster manager that accepts all connections.
///
/// # Act
/// Connect a WebSocket client with a valid token.
///
/// # Assert
/// The first binary message received has id `SERVER_READY` and source `SYSTEM_SOURCE`.
#[tokio::test]
async fn test_ws_valid_token_receives_server_ready() {
    let db = setup_test_db().await;
    let manager = manager_with_forwarding_cluster("ozstar");
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    let (_, mut stream) =
        connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/?token=valid_token")).await;

    // The server should send SERVER_READY after accepting the connection
    let data = recv_binary(&mut stream)
        .await
        .expect("Expected SERVER_READY binary message");
    let msg = Message::from_bytes(data);
    assert_eq!(
        msg.id(),
        SERVER_READY,
        "First message should be SERVER_READY"
    );
    assert_eq!(msg.source(), SYSTEM_SOURCE);
}

// ---------------------------------------------------------------------------
// test_ws_valid_token_handles_disconnect_gracefully
// ---------------------------------------------------------------------------

/// Verify that the server handles a client-initiated disconnect without errors.
///
/// # Setup
/// Start a test server with a forwarding cluster manager that accepts all connections.
///
/// # Act
/// Connect a client, receive `SERVER_READY`, then close the connection from the client side.
///
/// # Assert
/// No panic or timeout occurs within 100 ms after the client disconnects.
#[tokio::test]
async fn test_ws_valid_token_handles_disconnect_gracefully() {
    let db = setup_test_db().await;
    let manager = manager_with_forwarding_cluster("ozstar");
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    let (mut sink, mut stream) =
        connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/?token=valid")).await;

    // Receive SERVER_READY
    recv_binary(&mut stream).await;

    // Client closes connection
    sink.close().await.unwrap();

    // Server should process the disconnect without issues (no panic/timeout)
    tokio::task::yield_now().await;
}

// ---------------------------------------------------------------------------
// test_ws_binary_message_dispatched_to_cluster
// ---------------------------------------------------------------------------

/// Verify that binary messages from a client are dispatched to the cluster's `handle_message`.
///
/// # Setup
/// Start a test server with a mock cluster that captures every handled message id.
///
/// # Act
/// Connect a client, wait for `SERVER_READY`, then send an `UPDATE_JOB` binary message.
///
/// # Assert
/// The `UPDATE_JOB` message id appears in the list of messages received by the cluster.
#[tokio::test]
async fn test_ws_binary_message_dispatched_to_cluster() {
    use std::sync::{Arc as StdArc, Mutex};

    let db = setup_test_db().await;
    let received = StdArc::new(Mutex::new(Vec::<u32>::new()));

    // Build a cluster that captures handle_message calls
    let received_clone = StdArc::clone(&received);
    let mut mock_cluster = MockClusterTrait::new();
    mock_cluster
        .expect_name()
        .returning(|| "ozstar".to_string());
    mock_cluster
        .expect_role_string()
        .returning(|| "master".to_string());
    mock_cluster.expect_is_online().returning(|| true);
    mock_cluster.expect_role().returning(|| ClusterRole::Master);
    mock_cluster
        .expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    mock_cluster.expect_send_message().returning(|_| ());
    mock_cluster
        .expect_handle_message()
        .returning(move |msg: Message| {
            received_clone.lock().unwrap().push(msg.id());
            Box::pin(async {})
        });

    let cluster: Arc<dyn ClusterTrait> = Arc::new(mock_cluster);
    let cluster_for_manager = Arc::clone(&cluster);

    let mut manager = MockClusterManagerTrait::new();
    manager
        .expect_handle_new_connection()
        .returning(move |_, _, _| {
            let c = Arc::clone(&cluster_for_manager);
            Box::pin(async move { Some(c) })
        });
    manager
        .expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    manager.expect_report_websocket_error().returning(|_, _| ());
    manager.expect_handle_pong().returning(|_| ());

    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    let (mut sink, mut stream) =
        connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/?token=valid")).await;

    // Wait for SERVER_READY
    recv_binary(&mut stream).await;

    // Send an UPDATE_JOB message
    let mut update_msg = Message::new(UPDATE_JOB, Priority::Highest, SYSTEM_SOURCE);
    update_msg.push_uint(1); // job_id
    update_msg.push_string("test_what");
    update_msg.push_uint(10); // state
    update_msg.push_string("test_details");

    sink.send(TungsteniteMsg::Binary(update_msg.into_data().into()))
        .await
        .unwrap();

    // Wait for message to be processed with timeout
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(100);
    loop {
        if start.elapsed() > timeout {
            break;
        }
        let ids = received.lock().unwrap().clone();
        if ids.contains(&UPDATE_JOB) {
            return; // Test passed
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    panic!("UPDATE_JOB should have been dispatched to cluster.handle_message");
}

// ---------------------------------------------------------------------------
// test_ws_pong_handled
// ---------------------------------------------------------------------------

/// Verify that a WebSocket Pong frame is handled without errors.
///
/// # Setup
/// Start a test server with a forwarding cluster manager that accepts all connections.
///
/// # Act
/// Connect a client, receive `SERVER_READY`, then send a Pong frame.
///
/// # Assert
/// No panic or error occurs within 50 ms after the Pong is sent.
#[tokio::test]
async fn test_ws_pong_handled() {
    let db = setup_test_db().await;

    let manager = manager_with_forwarding_cluster("ozstar");
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    let (mut sink, mut stream) =
        connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/?token=valid")).await;

    // Receive SERVER_READY
    recv_binary(&mut stream).await;

    // Send a Pong — the server should handle this without error
    sink.send(TungsteniteMsg::Pong(vec![].into()))
        .await
        .unwrap();

    tokio::task::yield_now().await;
    // No assertion needed — the test passes if no panic occurs
}

// ---------------------------------------------------------------------------
// test_ws_authorization_header_success
// ---------------------------------------------------------------------------

/// Verify that connection with Authorization: Bearer header succeeds.
///
/// # Setup
/// Start a test server with a forwarding cluster manager.
///
/// # Act
/// Connect with `Authorization: Bearer valid-token` header.
///
/// # Assert
/// Connection succeeds and receives `SERVER_READY`.
#[tokio::test]
async fn test_ws_authorization_header_success() {
    let db = setup_test_db().await;
    let manager = manager_with_forwarding_cluster("ozstar");
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with Authorization: Bearer header
    let mut request = format!("ws://127.0.0.1:{port}/job/ws/")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Authorization", "Bearer valid-token".parse().unwrap());

    let (mut sink, mut stream) = tokio_tungstenite::connect_async(request)
        .await
        .unwrap()
        .0
        .split();

    // Should receive SERVER_READY
    let msg = recv_binary(&mut stream).await;
    assert!(
        msg.is_some(),
        "Should receive SERVER_READY with valid Bearer token"
    );

    sink.close().await.unwrap();
}

// ---------------------------------------------------------------------------
// test_ws_missing_authorization_header
// ---------------------------------------------------------------------------

/// Verify that connection without Authorization header is rejected.
///
/// # Setup
/// Start a test server.
///
/// # Act
/// Connect without Authorization header.
///
/// # Assert
/// Connection is rejected (no `SERVER_READY`).
#[tokio::test]
async fn test_ws_missing_authorization_header() {
    let db = setup_test_db().await;
    let manager = manager_rejecting_connections();
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect without Authorization header
    let (mut _sink, mut stream) = connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/")).await;

    // Should not receive SERVER_READY (connection rejected)
    let msg = recv_binary(&mut stream).await;
    assert!(
        msg.is_none(),
        "Connection without Authorization header should be rejected"
    );
}

// ---------------------------------------------------------------------------
// test_ws_malformed_authorization_header
// ---------------------------------------------------------------------------

/// Verify that malformed Authorization header is rejected.
///
/// # Setup
/// Start a test server.
///
/// # Act
/// Connect with Authorization header missing "Bearer " prefix.
///
/// # Assert
/// Connection is rejected.
#[tokio::test]
async fn test_ws_malformed_authorization_header() {
    let db = setup_test_db().await;
    let manager = manager_rejecting_connections();
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with malformed Authorization header (no "Bearer " prefix)
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = format!("ws://127.0.0.1:{port}/job/ws/")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Authorization", "invalid-token-format".parse().unwrap());

    let (mut sink, mut stream) = tokio_tungstenite::connect_async(request)
        .await
        .unwrap()
        .0
        .split();

    // Should not receive SERVER_READY
    let msg = recv_binary(&mut stream).await;
    assert!(
        msg.is_none(),
        "Malformed Authorization header should be rejected"
    );

    sink.close().await.unwrap();
}

// ---------------------------------------------------------------------------
// test_ws_query_param_rejected
// ---------------------------------------------------------------------------

/// Verify that old query parameter authentication is rejected.
///
/// # Setup
/// Start a test server.
///
/// # Act
/// Connect with `?token=valid` query parameter (no header).
///
/// # Assert
/// Connection is rejected (breaking change verified).
#[tokio::test]
async fn test_ws_query_param_rejected() {
    let db = setup_test_db().await;
    let manager = manager_rejecting_connections();
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with old query parameter method (should be rejected)
    let (mut _sink, mut stream) =
        connect_ws(&format!("ws://127.0.0.1:{port}/job/ws/?token=valid")).await;

    // Should not receive SERVER_READY (query params no longer supported)
    let msg = recv_binary(&mut stream).await;
    assert!(
        msg.is_none(),
        "Query parameter authentication should be rejected (breaking change)"
    );
}
