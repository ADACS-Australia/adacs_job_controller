//! Integration tests for the WebSocket server handler.
//!
//! Spins up a real axum HTTP server bound to a random port,
//! connects with `tokio-tungstenite`, and verifies connection lifecycle
//! and message dispatch behaviour.

mod common;

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as TungsteniteMsg;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use adacs_job_controller::cluster::traits::{ClusterTrait, MockClusterManagerTrait};
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::Priority;

use common::repeated_download::connect_ws as connect_ws_auth;
use common::{
    connect_ws, connection_closes, make_test_state, online_cluster, recv_binary, setup_test_db,
    ws_router,
};

// ---------------------------------------------------------------------------
// Test server helpers
// ---------------------------------------------------------------------------

/// Start an axum server on a random OS-assigned port.
/// Returns a `TestServer` RAII guard that aborts the server on drop.
async fn start_test_server(state: adacs_job_controller::app::AppState) -> common::TestServer {
    let app = ws_router(state);
    let (port, handle) = common::repeated_download::start_server(app).await;
    common::TestServer::new(port, handle)
}

/// Poll `cond` every 5ms until `timeout` elapses.
/// Returns `true` as soon as `cond()` returns `true`, otherwise `false`.
async fn wait_until<F>(timeout: std::time::Duration, cond: F) -> bool
where
    F: Fn() -> bool,
{
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            return false;
        }
        if cond() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// Read the first binary message and assert it is a `SERVER_READY` message
/// from `SYSTEM_SOURCE`.
async fn recv_server_ready(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    let data = recv_binary(stream)
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
// Build mock cluster manager helpers
// ---------------------------------------------------------------------------

/// Mock manager where `handle_new_connection` always returns `None` (invalid token).
fn manager_rejecting_connections() -> MockClusterManagerTrait {
    let mut m = MockClusterManagerTrait::new();
    m.expect_handle_new_connection()
        .returning(|_, _, _| Box::pin(async { None }));
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
/// Connect a WebSocket client with `Authorization: Bearer bad_token` header.
///
/// # Assert
/// The server closes the connection.
#[tokio::test]
async fn test_ws_invalid_token_disconnects() {
    let db = setup_test_db().await;
    let state = make_test_state(db, manager_rejecting_connections());
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with an invalid Bearer token
    let (_, mut stream) = connect_ws_auth(port, "bad_token").await;

    let closed = connection_closes(&mut stream, std::time::Duration::from_millis(500)).await;
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

    let closed = connection_closes(&mut stream, std::time::Duration::from_millis(500)).await;
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
/// Connect a WebSocket client with a valid `Authorization: Bearer` token.
///
/// # Assert
/// The first binary message received has id `SERVER_READY` and source `SYSTEM_SOURCE`.
#[tokio::test]
async fn test_ws_valid_token_receives_server_ready() {
    let db = setup_test_db().await;
    let (manager, _) = common::manager_with_forwarding_cluster("ozstar", None);
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with Authorization: Bearer header
    let (_, mut stream) = connect_ws_auth(port, "valid-token").await;

    // The server should send SERVER_READY after accepting the connection
    recv_server_ready(&mut stream).await;
}

// ---------------------------------------------------------------------------
// test_ws_valid_token_handles_disconnect_gracefully
// ---------------------------------------------------------------------------

/// Verify that the server forwards a client-initiated disconnect to the manager's
/// `remove_connection`.
///
/// # Setup
/// Start a test server with a forwarding cluster manager that accepts all connections.
///
/// # Act
/// Connect a client via `Authorization: Bearer` header, receive `SERVER_READY`,
/// then close the connection from the client side.
///
/// # Assert
/// The manager's `remove_connection` is invoked after the client disconnects.
#[tokio::test]
async fn test_ws_valid_token_handles_disconnect_gracefully() {
    use std::sync::atomic::Ordering;

    let db = setup_test_db().await;
    let (manager, removed_count) = common::manager_with_forwarding_cluster("ozstar", None);
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with Authorization: Bearer header
    let (mut sink, mut stream) = connect_ws_auth(port, "valid-token").await;

    // Receive SERVER_READY
    recv_binary(&mut stream).await;

    // Client closes connection
    sink.close().await.unwrap();

    // The server should forward the disconnect to remove_connection
    assert!(
        wait_until(std::time::Duration::from_millis(100), || {
            removed_count.load(Ordering::SeqCst) > 0
        })
        .await,
        "Client disconnect should have been forwarded to manager.remove_connection"
    );
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
/// Connect a client via `Authorization: Bearer` header, wait for `SERVER_READY`,
/// then send an `UPDATE_JOB` binary message.
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
    let mut mock_cluster = online_cluster("ozstar");
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
        .expect_get_file_download_admission()
        .returning(|_| None);
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

    // Connect with Authorization: Bearer header
    let (mut sink, mut stream) = connect_ws_auth(port, "valid-token").await;

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
    assert!(
        wait_until(std::time::Duration::from_millis(100), || {
            received.lock().unwrap().contains(&UPDATE_JOB)
        })
        .await,
        "UPDATE_JOB should have been dispatched to cluster.handle_message"
    );
}

// ---------------------------------------------------------------------------
// test_ws_pong_handled
// ---------------------------------------------------------------------------

/// Verify that a WebSocket Pong frame is forwarded to the manager's `handle_pong`.
///
/// # Setup
/// Start a test server with a mock manager that records `handle_pong` calls.
///
/// # Act
/// Connect a client via `Authorization: Bearer` header, receive `SERVER_READY`,
/// then send a Pong frame.
///
/// # Assert
/// The manager's `handle_pong` is invoked after the Pong is sent.
#[tokio::test]
async fn test_ws_pong_handled() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let db = setup_test_db().await;
    let pong_handled = Arc::new(AtomicBool::new(false));

    // Build a minimal cluster for the accepted connection
    let mut mock_cluster = online_cluster("ozstar");
    mock_cluster
        .expect_handle_message()
        .returning(|_| Box::pin(async {}));

    let cluster: Arc<dyn ClusterTrait> = Arc::new(mock_cluster);
    let cluster_for_manager = Arc::clone(&cluster);

    // Build a manager that records handle_pong invocations
    let pong_flag = Arc::clone(&pong_handled);
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
    manager
        .expect_get_file_download_admission()
        .returning(|_| None);
    manager.expect_handle_pong().returning(move |_| {
        pong_flag.store(true, Ordering::SeqCst);
    });

    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with Authorization: Bearer header
    let (mut sink, mut stream) = connect_ws_auth(port, "valid-token").await;

    // Wait for SERVER_READY
    recv_binary(&mut stream).await;

    // Send a Pong — the server should forward it to handle_pong
    sink.send(TungsteniteMsg::Pong(vec![].into()))
        .await
        .unwrap();

    // Wait for handle_pong to be invoked, with a timeout
    assert!(
        wait_until(std::time::Duration::from_millis(100), || {
            pong_handled.load(Ordering::SeqCst)
        })
        .await,
        "Pong should have been forwarded to manager.handle_pong"
    );
}

// ---------------------------------------------------------------------------
// test_ws_lowercase_bearer_scheme_accepted
// ---------------------------------------------------------------------------

/// Verify that a lowercase `bearer` scheme prefix is accepted per RFC 6750.
///
/// # Setup
/// Start a test server with a forwarding cluster manager.
///
/// # Act
/// Connect with `Authorization: bearer valid-token` header (lowercase scheme).
///
/// # Assert
/// Connection succeeds and receives `SERVER_READY`.
#[tokio::test]
async fn test_ws_lowercase_bearer_scheme_accepted() {
    let db = setup_test_db().await;
    let (manager, _) = common::manager_with_forwarding_cluster("ozstar", None);
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with lowercase "bearer " scheme prefix
    let mut request = format!("ws://127.0.0.1:{port}/job/ws/")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Authorization", "bearer valid-token".parse().unwrap());

    let (mut sink, mut stream) = tokio_tungstenite::connect_async(request)
        .await
        .unwrap()
        .0
        .split();

    // The server should send SERVER_READY after accepting the connection
    recv_server_ready(&mut stream).await;

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
/// Start a test server with a forwarding cluster manager that accepts only a
/// specific token.
///
/// # Act
/// Connect with Authorization header missing "Bearer " prefix.
///
/// # Assert
/// Connection is rejected because the malformed header yields no valid token.
#[tokio::test]
async fn test_ws_malformed_authorization_header() {
    let db = setup_test_db().await;
    let (manager, _) = common::manager_with_forwarding_cluster("ozstar", Some("valid"));
    let state = make_test_state(db, manager);
    let server = start_test_server(state).await;
    let port = server.port;

    // Connect with malformed Authorization header (no "Bearer " prefix)
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
    let (manager, _) = common::manager_with_forwarding_cluster("ozstar", Some("valid"));
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
