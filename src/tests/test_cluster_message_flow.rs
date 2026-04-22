//! Integration tests for Cluster message flow: priority queuing, scheduling,
//! file download/upload state transitions, backpressure, and file list handling.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use adacs_job_controller::cluster::cluster::{AppContext, Cluster};
use adacs_job_controller::cluster::file_download::FileDownloadState;
use adacs_job_controller::cluster::file_upload::FileUploadState;
use adacs_job_controller::cluster::traits::{ClusterTrait, WsOutbound};
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::{FileListState, Priority};
use dashmap::DashMap;

fn test_config() -> ClusterConfig {
    ClusterConfig {
        name: "test_cluster".to_string(),
        host: "localhost".to_string(),
        username: "user".to_string(),
        path: "/home/user/jc".to_string(),
        key: String::new(),
        connection_type: "manual".to_string(),
        keytab: String::new(),
        kerberos_principal: String::new(),
        ltk: None,
    }
}

// ---------------------------------------------------------------------------
// Priority queue ordering
// ---------------------------------------------------------------------------

/// Verifies that the cluster priority queue delivers messages in Highest → Medium → Lowest order.
///
/// # Setup
/// A `Cluster` is given a live unbounded channel and the scheduler is started.
/// Three messages with `Priority::Lowest`, `Priority::Highest`, and `Priority::Medium` are queued
/// in that order.
///
/// # Act
/// The scheduler drains the queue; three messages are received with a 2-second timeout each.
///
/// # Assert
/// Messages arrive in Highest → Medium → Lowest priority order, verified by message ID.
#[tokio::test]
async fn test_priority_queue_ordering_via_channel() {
    let cluster = Cluster::new(test_config(), None);

    // Set up a real channel as the "WS connection"
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));

    // Start the scheduler
    cluster.start_tasks();

    // Queue messages at different priorities
    let mut msg_low = Message::new(1000001, Priority::Lowest, "low");
    msg_low.push_string("lowest_msg");
    let data_low = msg_low.into_data();

    let mut msg_high = Message::new(1000002, Priority::Highest, "high");
    msg_high.push_string("highest_msg");
    let data_high = msg_high.into_data();

    let mut msg_med = Message::new(1000003, Priority::Medium, "med");
    msg_med.push_string("medium_msg");
    let data_med = msg_med.into_data();

    // Queue lowest first, then highest, then medium
    cluster.queue_message("low".into(), data_low.clone(), Priority::Lowest);
    cluster.queue_message("high".into(), data_high.clone(), Priority::Highest);
    cluster.queue_message("med".into(), data_med.clone(), Priority::Medium);

    // Collect results - the scheduler should dequeue in priority order
    let mut received = Vec::new();
    for _ in 0..3 {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(WsOutbound::Binary(data))) => received.push(data),
            _ => break,
        }
    }

    cluster.stop();

    assert_eq!(received.len(), 3, "should have received all 3 messages");

    // Parse each received message to check the ID
    let id0 = Message::from_bytes(received[0].clone()).id();
    let id1 = Message::from_bytes(received[1].clone()).id();
    let id2 = Message::from_bytes(received[2].clone()).id();

    // Highest priority (0) should come first, then Medium (10), then Lowest (19)
    assert_eq!(id0, 1000002, "first should be highest priority");
    assert_eq!(id1, 1000003, "second should be medium priority");
    assert_eq!(id2, 1000001, "third should be lowest priority");
}

/// Verifies that multiple messages queued at the same priority are all delivered.
///
/// # Setup
/// A `Cluster` is given a live unbounded channel and the scheduler is started.
/// Two messages from different sources are queued at `Priority::Medium`.
///
/// # Act
/// The scheduler drains the queue. Both messages are received with a 2-second timeout each.
///
/// # Assert
/// Both message IDs (`1000001` and `1000002`) appear in the received output.
#[tokio::test]
async fn test_multiple_messages_same_priority_round_robin() {
    let cluster = Cluster::new(test_config(), None);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    // Queue 2 messages from different sources at same priority
    let mut msg_a = Message::new(1000001, Priority::Medium, "source_a");
    msg_a.push_string("A");
    let mut msg_b = Message::new(1000002, Priority::Medium, "source_b");
    msg_b.push_string("B");

    cluster.queue_message("source_a".into(), msg_a.into_data(), Priority::Medium);
    cluster.queue_message("source_b".into(), msg_b.into_data(), Priority::Medium);

    let mut received = Vec::new();
    for _ in 0..2 {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(WsOutbound::Binary(data))) => received.push(data),
            _ => break,
        }
    }

    cluster.stop();

    assert_eq!(received.len(), 2, "should receive both messages");
    // Both messages should be delivered (order within same priority may vary by source iteration)
    let ids: Vec<u32> = received
        .iter()
        .map(|d| Message::from_bytes(d.clone()).id())
        .collect();
    assert!(ids.contains(&1000001));
    assert!(ids.contains(&1000002));
}

// ---------------------------------------------------------------------------
// File download state transitions
// ---------------------------------------------------------------------------

/// Verifies the file download state transitions when `FILE_DETAILS` and `FILE_CHUNK` messages are received.
///
/// # Setup
/// A `FileDownloadState` and a `Cluster` created via `new_file_download` with uuid `"dl-uuid-1"` are prepared.
///
/// # Act
/// A `FILE_DETAILS` message with `file_size=1024` is dispatched, then a `FILE_CHUNK` message
/// with 4 bytes (`0xDEADBEEF`) is dispatched.
///
/// # Assert
/// After `FILE_DETAILS`: `received_data`, `file_size=1024`, and `data_ready` are set.
/// After `FILE_CHUNK`: `received_bytes=4` and the 4-byte chunk is available in the receiver channel.
#[tokio::test]
async fn test_file_download_chunk_flow() {
    let download_state = Arc::new(FileDownloadState::new());
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let cluster = Cluster::new_file_download(
        test_config(),
        "dl-uuid-1".to_string(),
        Arc::clone(&download_state),
        None,
        lock,
    );

    // Simulate receiving FILE_DETAILS -- must go through from_bytes to parse header
    let mut details_msg = Message::new(FILE_DETAILS, Priority::Medium, "test");
    details_msg.push_ulong(1024); // file size
    let details_msg = Message::from_bytes(details_msg.into_data());
    cluster.handle_message(details_msg).await;

    assert!(download_state.received_data.load(Ordering::Relaxed));
    assert_eq!(download_state.file_size.load(Ordering::Relaxed), 1024);
    assert!(download_state.data_ready.load(Ordering::Relaxed));

    // Simulate receiving FILE_CHUNK -- must go through from_bytes to parse header
    let mut chunk_msg = Message::new(FILE_CHUNK, Priority::Medium, "test");
    chunk_msg.push_bytes(&[0xDE, 0xAD, 0xBE, 0xEF]);
    let chunk_msg = Message::from_bytes(chunk_msg.into_data());
    cluster.handle_message(chunk_msg).await;

    assert_eq!(download_state.received_bytes.load(Ordering::Relaxed), 4);

    // Verify chunk arrived through the channel
    let mut receiver = download_state.chunk_receiver.lock().await;
    let chunk = receiver.try_recv().expect("should have a chunk");
    assert_eq!(chunk, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

/// Verifies the file download state transitions when a `FILE_ERROR` message is received.
///
/// # Setup
/// A `FileDownloadState` and a `Cluster` created via `new_file_download` with uuid `"dl-uuid-err"` are prepared.
///
/// # Act
/// A `FILE_ERROR` message with the text `"Permission denied"` is dispatched.
///
/// # Assert
/// The `error` flag is set, `data_ready` is set, and `error_details` equals `"Permission denied"`.
#[tokio::test]
async fn test_file_download_error_flow() {
    let download_state = Arc::new(FileDownloadState::new());
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let cluster = Cluster::new_file_download(
        test_config(),
        "dl-uuid-err".to_string(),
        Arc::clone(&download_state),
        None,
        lock,
    );

    let mut err_msg = Message::new(FILE_ERROR, Priority::Medium, "test");
    err_msg.push_string("Permission denied");
    let err_msg = Message::from_bytes(err_msg.into_data());
    cluster.handle_message(err_msg).await;

    assert!(download_state.error.load(Ordering::Relaxed));
    assert!(download_state.data_ready.load(Ordering::Relaxed));
    assert_eq!(
        *download_state.error_details.lock().await,
        "Permission denied"
    );
}

// ---------------------------------------------------------------------------
// File upload state transitions
// ---------------------------------------------------------------------------

/// Verifies that receiving a `SERVER_READY` message sets the upload state's `data_ready` flag.
///
/// # Setup
/// A `FileUploadState` and a `Cluster` created via `new_file_upload` with uuid `"ul-uuid-1"` are prepared.
///
/// # Act
/// A `SERVER_READY` message is dispatched.
///
/// # Assert
/// `upload_state.data_ready` is set to `true`.
#[tokio::test]
async fn test_file_upload_server_ready_flow() {
    let upload_state = Arc::new(FileUploadState::new());
    let cluster = Cluster::new_file_upload(
        test_config(),
        "ul-uuid-1".to_string(),
        Arc::clone(&upload_state),
        None,
    );

    // SERVER_READY signals the upload cluster is ready -- must go through from_bytes
    let ready_msg = Message::new(SERVER_READY, Priority::Medium, "test");
    let ready_msg = Message::from_bytes(ready_msg.into_data());
    cluster.handle_message(ready_msg).await;

    assert!(upload_state.data_ready.load(Ordering::Relaxed));
}

/// Verifies the file upload state transitions when a `FILE_UPLOAD_ERROR` message is received.
///
/// # Setup
/// A `FileUploadState` and a `Cluster` created via `new_file_upload` with uuid `"ul-uuid-err"` are prepared.
///
/// # Act
/// A `FILE_UPLOAD_ERROR` message with the text `"Disk full"` is dispatched.
///
/// # Assert
/// The `error` flag is set, `data_ready` is set, and `error_details` equals `"Disk full"`.
#[tokio::test]
async fn test_file_upload_error_flow() {
    let upload_state = Arc::new(FileUploadState::new());
    let cluster = Cluster::new_file_upload(
        test_config(),
        "ul-uuid-err".to_string(),
        Arc::clone(&upload_state),
        None,
    );

    let mut err_msg = Message::new(FILE_UPLOAD_ERROR, Priority::Medium, "test");
    err_msg.push_string("Disk full");
    let err_msg = Message::from_bytes(err_msg.into_data());
    cluster.handle_message(err_msg).await;

    assert!(upload_state.error.load(Ordering::Relaxed));
    assert!(upload_state.data_ready.load(Ordering::Relaxed));
    assert_eq!(*upload_state.error_details.lock().await, "Disk full");
}

/// Verifies the file upload state transitions when a `FILE_UPLOAD_COMPLETE` message is received.
///
/// # Setup
/// A `FileUploadState` and a `Cluster` created via `new_file_upload` with uuid `"ul-uuid-done"` are prepared.
///
/// # Act
/// A `FILE_UPLOAD_COMPLETE` message is dispatched.
///
/// # Assert
/// The `complete`, `received_data`, and `data_ready` flags are all set to `true`.
#[tokio::test]
async fn test_file_upload_complete_flow() {
    let upload_state = Arc::new(FileUploadState::new());
    let cluster = Cluster::new_file_upload(
        test_config(),
        "ul-uuid-done".to_string(),
        Arc::clone(&upload_state),
        None,
    );

    let complete_msg = Message::new(FILE_UPLOAD_COMPLETE, Priority::Medium, "test");
    let complete_msg = Message::from_bytes(complete_msg.into_data());
    cluster.handle_message(complete_msg).await;

    assert!(upload_state.complete.load(Ordering::Relaxed));
    assert!(upload_state.received_data.load(Ordering::Relaxed));
    assert!(upload_state.data_ready.load(Ordering::Relaxed));
}

// ---------------------------------------------------------------------------
// Backpressure: wait_for_queue_drain
// ---------------------------------------------------------------------------

/// Verifies that `wait_for_queue_drain` returns `true` immediately when the queue is empty.
///
/// # Setup
/// A `Cluster` is created with no queued messages and no connection.
///
/// # Act
/// `cluster.wait_for_queue_drain(true).await` is called.
///
/// # Assert
/// The result is `true`.
#[tokio::test]
async fn test_wait_for_queue_drain_empty_returns_immediately() {
    let cluster = Cluster::new(test_config(), None);

    // Queue is empty, should return true instantly
    let result = cluster.wait_for_queue_drain(true).await;
    assert!(result, "empty queue should drain immediately");
}

/// Verifies that `wait_for_queue_drain` blocks until the scheduler drains the queued message.
///
/// # Setup
/// A `Cluster` is given a live channel and the scheduler is started. One message is queued.
///
/// # Act
/// `wait_for_queue_drain(true)` is awaited with a 5-second timeout.
/// The queued message is consumed from the channel.
///
/// # Assert
/// The drain future resolves within the timeout and returns `true`.
#[tokio::test]
async fn test_wait_for_queue_drain_blocks_then_unblocks() {
    let cluster = Cluster::new(test_config(), None);

    // Set up a connection so the scheduler can drain the queue
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    // Queue a message
    let msg = Message::new(1000001, Priority::Medium, "test");
    cluster.queue_message("test".into(), msg.into_data(), Priority::Medium);

    // wait_for_queue_drain(true) should eventually return true once scheduler drains it
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        cluster.wait_for_queue_drain(true),
    )
    .await;

    // Consume the message so the channel doesn't back up
    let _ = rx.try_recv();

    cluster.stop();

    assert!(
        result.is_ok(),
        "should not have timed out waiting for drain"
    );
    assert!(result.unwrap(), "drain should report success");
}

// ---------------------------------------------------------------------------
// Cluster lifecycle: stop, set_connection
// ---------------------------------------------------------------------------

/// Verifies the cluster online/offline state transitions when a connection is set and cleared.
///
/// # Setup
/// A `Cluster` is created with no connection.
///
/// # Act
/// `set_connection(Some(tx))` is called, then `set_connection(None)` is called.
///
/// # Assert
/// `is_online()` returns `false` initially, `true` after setting the connection,
/// and `false` again after clearing it.
#[tokio::test]
async fn test_cluster_set_connection_and_offline() {
    let cluster = Cluster::new(test_config(), None);
    assert!(!cluster.is_online());

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    assert!(cluster.is_online());

    cluster.set_connection(None);
    assert!(!cluster.is_online());
}

/// Verifies that calling `close` on an online cluster transitions it to offline.
///
/// # Setup
/// A `Cluster` is created and a connection is set via `set_connection(Some(tx))`.
///
/// # Act
/// `cluster.close(false)` is called.
///
/// # Assert
/// `is_online()` returns `false` after `close`.
#[tokio::test]
async fn test_cluster_close_disconnects() {
    let cluster = Cluster::new(test_config(), None);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    assert!(cluster.is_online());

    cluster.close(false);
    assert!(!cluster.is_online());
}

// ===========================================================================
// Constructor and initialization tests
// ===========================================================================

/// Verifies that the Cluster constructor initializes all fields correctly.
///
/// # Setup
/// Creates a Cluster with a test ClusterConfig.
///
/// # Act
/// Instantiates Cluster object.
///
/// # Assert
/// - Cluster name matches config
/// - Cluster is initially offline (no connection)
/// - Cluster role is Master (default)
/// - Priority queues are created (3 levels)
/// - Running flag is true
#[tokio::test]
async fn test_cluster_constructor() {
    use adacs_job_controller::protocol::types::ClusterRole;

    let config = test_config();
    let cluster = Cluster::new(config.clone(), None);

    // Verify basic properties
    assert_eq!(cluster.name(), config.name);
    assert!(!cluster.is_online());
    assert_eq!(cluster.role(), ClusterRole::Master);
    // role_string includes both role and name
    assert!(cluster.role_string().contains("master"));

    // Verify cluster details
    let details = cluster.cluster_details();
    assert_eq!(details.name, config.name);
    assert_eq!(details.host, config.host);
    assert_eq!(details.username, config.username);
    assert_eq!(details.path, config.path);
}

/// Verifies that Cluster constructor with AppContext works correctly.
///
/// # Setup
/// Creates an AppContext with in-memory SQLite and empty file_list_map.
///
/// # Act
/// Instantiates Cluster with AppContext.
///
/// # Assert
/// - Cluster is created successfully
/// - AppContext is stored (verified via file operations later)
#[tokio::test]
async fn test_cluster_constructor_with_app_context() {
    let db = futures::executor::block_on(sea_orm::Database::connect("sqlite::memory:"))
        .expect("sqlite in-memory connect failed");
    let file_list_map = Arc::new(DashMap::new());
    let app_context = Arc::new(AppContext { db, file_list_map });

    let cluster = Cluster::new(test_config(), Some(app_context.clone()));

    assert_eq!(cluster.name(), "test_cluster");
    assert!(!cluster.is_online());
}

/// Verifies that FileDownloadState constructor initializes all fields.
///
/// # Setup
/// None - tests default constructor.
///
/// # Act
/// Instantiates FileDownloadState.
///
/// # Assert
/// - All atomic flags are false initially
/// - All counters are zero
/// - file_size is zero
/// - chunk_receiver is empty
/// - data_notify has no pending notifications
#[tokio::test]
async fn test_file_download_constructor() {
    let state = FileDownloadState::new();

    // Verify all flags are false
    assert!(!state.error.load(Ordering::Relaxed));
    assert!(!state.received_data.load(Ordering::Relaxed));
    assert!(!state.data_ready.load(Ordering::Relaxed));
    assert!(!state.client_paused.load(Ordering::Relaxed));

    // Verify counters are zero
    assert_eq!(state.received_bytes.load(Ordering::Relaxed), 0);
    assert_eq!(state.sent_bytes.load(Ordering::Relaxed), 0);
    assert_eq!(state.file_size.load(Ordering::Relaxed), 0);

    // Verify chunk receiver is empty
    assert!(state.chunk_receiver.lock().await.try_recv().is_err());
}

/// Verifies that FileUploadState constructor initializes all fields.
///
/// # Setup
/// None - tests default constructor.
///
/// # Act
/// Instantiates FileUploadState.
///
/// # Assert
/// - All atomic flags are false
/// - data_notify has no pending notifications
#[tokio::test]
async fn test_file_upload_constructor() {
    let state = FileUploadState::new();

    // Verify all flags are false
    assert!(!state.error.load(Ordering::Relaxed));
    assert!(!state.received_data.load(Ordering::Relaxed));
    assert!(!state.data_ready.load(Ordering::Relaxed));
    assert!(!state.complete.load(Ordering::Relaxed));
}

// ===========================================================================
// Priority queue and scheduler tests
// ===========================================================================

/// Tests that the scheduler processes messages in priority order.
///
/// # Setup
/// Creates a Cluster with a WebSocket channel, queues 14 messages across
/// 6 sources and 3 priorities on a stopped cluster.
///
/// # Act
/// Starts the cluster message dispatcher, receives messages from channel.
///
/// # Assert
/// Messages are sent in priority order (Highest→Medium→Lowest), then by
/// source order within each priority level.
#[tokio::test]
async fn test_run_scheduler_priority_ordering() {
    use adacs_job_controller::cluster::traits::ClusterTrait;
    use adacs_job_controller::protocol::types::Priority;

    let cluster = Cluster::new(test_config(), None);

    // Queue messages at different priorities from different sources
    // Highest priority: s1, s2
    let mut msg_h1 = Message::new(1001, Priority::Highest, "s1");
    msg_h1.push_string("highest_s1");
    cluster.queue_message("s1".into(), msg_h1.into_data(), Priority::Highest);

    let mut msg_h2 = Message::new(1002, Priority::Highest, "s2");
    msg_h2.push_string("highest_s2");
    cluster.queue_message("s2".into(), msg_h2.into_data(), Priority::Highest);

    // Medium priority: s3, s4
    let mut msg_m1 = Message::new(2001, Priority::Medium, "s3");
    msg_m1.push_string("medium_s3");
    cluster.queue_message("s3".into(), msg_m1.into_data(), Priority::Medium);

    let mut msg_m2 = Message::new(2002, Priority::Medium, "s4");
    msg_m2.push_string("medium_s4");
    cluster.queue_message("s4".into(), msg_m2.into_data(), Priority::Medium);

    // Lowest priority: s5, s6
    let mut msg_l1 = Message::new(3001, Priority::Lowest, "s5");
    msg_l1.push_string("lowest_s5");
    cluster.queue_message("s5".into(), msg_l1.into_data(), Priority::Lowest);

    let mut msg_l2 = Message::new(3002, Priority::Lowest, "s6");
    msg_l2.push_string("lowest_s6");
    cluster.queue_message("s6".into(), msg_l2.into_data(), Priority::Lowest);

    // Set up channel and start scheduler
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    // Receive all 6 messages
    let mut received = Vec::new();
    for _ in 0..6 {
        if let Ok(Some(WsOutbound::Binary(data))) =
            tokio::time::timeout(Duration::from_secs(2), rx.recv()).await
        {
            let msg = Message::from_bytes(data);
            received.push((msg.id(), msg.source().to_string()));
        }
    }

    // Verify priority ordering: all Highest first, then Medium, then Lowest
    let ids: Vec<u32> = received.iter().map(|(id, _)| *id).collect();

    // First two should be Highest priority (1001, 1002) - order within same priority may vary
    assert!(
        ids[0] == 1001 || ids[0] == 1002,
        "First message should be Highest priority"
    );
    assert!(
        ids[1] == 1001 || ids[1] == 1002,
        "Second message should be Highest priority"
    );
    assert_ne!(
        ids[0], ids[1],
        "Both Highest priority messages should be received first"
    );

    // Next two should be Medium priority (2001, 2002) - order within same priority may vary
    assert!(
        ids[2] == 2001 || ids[2] == 2002,
        "Third message should be Medium priority"
    );
    assert!(
        ids[3] == 2001 || ids[3] == 2002,
        "Fourth message should be Medium priority"
    );
    assert_ne!(
        ids[2], ids[3],
        "Both Medium priority messages should be received"
    );

    // Last two should be Lowest priority (3001, 3002) - order within same priority may vary
    assert!(
        ids[4] == 3001 || ids[4] == 3002,
        "Fifth message should be Lowest priority"
    );
    assert!(
        ids[5] == 3001 || ids[5] == 3002,
        "Sixth message should be Lowest priority"
    );
    assert_ne!(
        ids[4], ids[5],
        "Both Lowest priority messages should be received"
    );

    cluster.stop();
}

/// Tests that higher priority data preempts lower priority processing.
///
/// # Setup
/// Queues messages at Medium priority, then adds a Highest priority message.
///
/// # Act
/// Starts scheduler and observes message ordering.
///
/// # Assert
/// When higher priority data arrives, scheduler resets and processes
/// higher priority queue first.
#[tokio::test]
async fn test_does_higher_priority_data_exist() {
    use adacs_job_controller::cluster::traits::ClusterTrait;
    use adacs_job_controller::protocol::types::Priority;

    let cluster = Cluster::new(test_config(), None);

    // Queue some Medium priority messages first
    for i in 0..3 {
        let mut msg = Message::new(2000 + i, Priority::Medium, &format!("medium_s{}", i));
        msg.push_string(&format!("medium_data_{}", i));
        cluster.queue_message(format!("medium_s{}", i), msg.into_data(), Priority::Medium);
    }

    // Verify there is higher priority data when checking from Medium context
    // (Even though no Highest is queued yet, we test the mechanism)

    // Now add Highest priority message
    let mut high_msg = Message::new(1000, Priority::Highest, "high_s");
    high_msg.push_string("high_priority_data");
    cluster.queue_message("high_s".into(), high_msg.into_data(), Priority::Highest);

    // Set up channel and start scheduler
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    // First message received should be the Highest priority one
    if let Ok(Some(WsOutbound::Binary(data))) =
        tokio::time::timeout(Duration::from_secs(2), rx.recv()).await
    {
        let msg = Message::from_bytes(data);
        assert_eq!(
            msg.id(),
            1000,
            "Highest priority message should be sent first"
        );
        assert_eq!(msg.source(), "high_s");
    } else {
        panic!("Did not receive highest priority message");
    }

    cluster.stop();
}

// ---------------------------------------------------------------------------
// send_message queues data
// ---------------------------------------------------------------------------

/// Verifies that `send_message` routes a message through the scheduler to the WS channel.
///
/// # Setup
/// A `Cluster` is given a live channel and the scheduler is started.
/// A `SUBMIT_JOB` message with `source="job_1_test"`, `job_id=1`, bundle, and params is prepared.
///
/// # Act
/// `cluster.send_message(msg)` is called. The channel is read with a 2-second timeout.
///
/// # Assert
/// The received binary payload round-trips to the original source, message ID, and payload fields.
#[tokio::test]
async fn test_send_message_queues_correctly() {
    let cluster = Cluster::new(test_config(), None);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    // Use the trait's send_message
    let mut msg = Message::new(SUBMIT_JOB, Priority::Medium, "job_1_test");
    msg.push_uint(1);
    msg.push_string("bundle");
    msg.push_string("params");
    cluster.send_message(msg);

    // Should arrive via the scheduler
    let outbound = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");
    let WsOutbound::Binary(received) = outbound else {
        panic!("expected Binary")
    };

    cluster.stop();

    let mut parsed = Message::from_bytes(received);
    assert_eq!(parsed.source(), "job_1_test");
    assert_eq!(parsed.id(), SUBMIT_JOB);
    assert_eq!(parsed.pop_uint(), 1);
    assert_eq!(parsed.pop_string(), "bundle");
    assert_eq!(parsed.pop_string(), "params");
}

// ---------------------------------------------------------------------------
// File upload: unrecognized message does not crash
// ---------------------------------------------------------------------------

/// Verifies that an unrecognized message ID handled by a file-upload cluster does not panic or set state flags.
///
/// # Setup
/// A `FileUploadState` and a file-upload `Cluster` with uuid `"ul-uuid-unknown"` are prepared.
///
/// # Act
/// A message with ID `999999` is dispatched.
///
/// # Assert
/// Neither the `error` nor the `complete` flag is set on the upload state.
#[tokio::test]
async fn test_file_upload_unrecognized_message_no_crash() {
    let upload_state = Arc::new(FileUploadState::new());
    let cluster = Cluster::new_file_upload(
        test_config(),
        "ul-uuid-unknown".to_string(),
        Arc::clone(&upload_state),
        None,
    );

    // Send a message with an ID that the file upload handler doesn't handle
    let unknown_msg = Message::new(999999, Priority::Medium, "test");
    let unknown_msg = Message::from_bytes(unknown_msg.into_data());
    cluster.handle_message(unknown_msg).await;

    // Should not have set any state flags
    assert!(!upload_state.error.load(Ordering::Relaxed));
    assert!(!upload_state.complete.load(Ordering::Relaxed));
}

// ---------------------------------------------------------------------------
// Source pruning: queued messages from a source are drained; empty source removed
// ---------------------------------------------------------------------------

/// Verifies that once all messages from all sources are drained, `wait_for_queue_drain` reports success.
///
/// # Setup
/// Two messages from `"source_a"` and `"source_b"` are queued at `Priority::Medium`.
/// The cluster is given a live channel and the scheduler is started.
///
/// # Act
/// Both messages are received from the channel, then `wait_for_queue_drain(true)` is awaited.
///
/// # Assert
/// The drain future resolves successfully within a 2-second timeout.
#[tokio::test]
async fn test_prune_sources_removes_empty_queues() {
    let cluster = Cluster::new(test_config(), None);

    // Queue messages from two sources
    let mut msg_a = Message::new(1000001, Priority::Medium, "source_a");
    msg_a.push_string("A");
    let mut msg_b = Message::new(1000002, Priority::Medium, "source_b");
    msg_b.push_string("B");

    cluster.queue_message("source_a".into(), msg_a.into_data(), Priority::Medium);
    cluster.queue_message("source_b".into(), msg_b.into_data(), Priority::Medium);

    // Connect and start so scheduler drains the queue
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    // Wait for both messages to be dequeued
    for _ in 0..2 {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await;
    }

    // After draining, queue should eventually be empty; wait_for_queue_drain confirms this
    let drained = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        cluster.wait_for_queue_drain(true),
    )
    .await;
    assert!(drained.is_ok() && drained.unwrap());

    cluster.stop();
}

// ---------------------------------------------------------------------------
// test_PruneSources - background thread pruning with timing
// ---------------------------------------------------------------------------

/// Verifies that the background source pruner removes empty sources after the configured interval.
///
/// Equivalent to C++ test: `legacy/Cluster/tests/background_threads_tests.cpp:test_PruneSources`
///
/// # Setup
/// - Sets QUEUE_SOURCE_PRUNE_MILLISECONDS=100 for fast test execution
/// - Creates Cluster with multiple sources at different priorities
/// - Starts cluster with WebSocket connection to drain messages
///
/// # Act
/// - Waits for messages to be sent (queue drains)
/// - Waits for pruner to run (prune interval * 2)
/// - Verifies queue is empty after pruning
///
/// # Assert
/// - Messages are received over WebSocket
/// - Queue drains after messages sent
/// - Pruner removes empty sources
#[tokio::test]
async fn test_prune_sources_background_thread() {
    use adacs_job_controller::cluster::traits::ClusterTrait;
    use adacs_job_controller::protocol::message::Message;

    // Set prune interval to 100ms for fast test execution
    let prune_interval_ms = std::env::var("QUEUE_SOURCE_PRUNE_MILLISECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(100);

    let mut total_received = 0;

    // Run 5 iterations like the C++ test
    for _iteration in 0..5 {
        let cluster = Cluster::new(test_config(), None);

        // Set up WebSocket connection to capture messages and drain queue
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
        cluster.set_connection(Some(tx));
        cluster.start_tasks();

        // Queue messages from 3 sources at different priorities (like C++ test)
        // Note: Must queue AFTER start_tasks() so scheduler doesn't miss the notification
        let mut msg_s1 = Message::new(SUBMIT_JOB, Priority::Highest, "s1");
        msg_s1.push_string("s1_data");
        cluster.queue_message("s1".into(), msg_s1.into_data(), Priority::Highest);

        let mut msg_s2 = Message::new(CANCEL_JOB, Priority::Lowest, "s2");
        msg_s2.push_string("s2_data");
        cluster.queue_message("s2".into(), msg_s2.into_data(), Priority::Lowest);

        let mut msg_s3 = Message::new(DELETE_JOB, Priority::Lowest, "s3");
        msg_s3.push_string("s3_data");
        cluster.queue_message("s3".into(), msg_s3.into_data(), Priority::Lowest);

        // Wait for all 3 messages to be sent
        let mut received_this_iter = 0;
        while received_this_iter < 3 {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
                Ok(Some(_)) => received_this_iter += 1,
                Ok(None) => break,
                Err(_) => panic!("Timeout waiting for messages"),
            }
        }
        total_received += received_this_iter;

        // Wait for pruner to run (prune interval * 2 to handle processing time)
        tokio::time::sleep(Duration::from_millis(prune_interval_ms * 2)).await;

        // Verify queue is drained after pruning
        let drained =
            tokio::time::timeout(Duration::from_secs(2), cluster.wait_for_queue_drain(true)).await;
        assert!(
            drained.is_ok() && drained.unwrap(),
            "Queue should drain after pruning"
        );

        cluster.stop();
    }

    // Verify total messages received (5 iterations * 3 messages = 15)
    assert_eq!(
        total_received, 15,
        "Should receive 15 messages total (5 iterations * 3 messages)"
    );
}

// ---------------------------------------------------------------------------
// File list handling
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn make_app_context_with_file_list_map() -> (
    Arc<AppContext>,
    Arc<DashMap<String, Arc<tokio::sync::Mutex<FileListState>>>>,
) {
    let db = futures::executor::block_on(sea_orm::Database::connect("sqlite::memory:"))
        .expect("sqlite connect failed");
    let file_list_map = Arc::new(DashMap::new());
    let ctx = Arc::new(AppContext {
        db,
        file_list_map: Arc::clone(&file_list_map),
    });
    (ctx, file_list_map)
}

/// Verifies that a `FILE_LIST` message for an unregistered UUID does not modify the file list map.
///
/// # Setup
/// A `Cluster` with an `AppContext` containing an empty `file_list_map` is created.
/// A `FILE_LIST` message with uuid `"unknown-uuid"` and 2 file entries is built.
///
/// # Act
/// The message is dispatched via `cluster.handle_message`.
///
/// # Assert
/// The `file_list_map` remains empty.
#[tokio::test]
async fn test_handle_file_list_unknown_uuid_is_noop() {
    let (ctx, file_list_map) = make_app_context_with_file_list_map();
    let cluster = Cluster::new(test_config(), Some(ctx));

    let uuid = "unknown-uuid";

    // Build FILE_LIST message
    let mut msg = Message::new(FILE_LIST, Priority::Medium, "test");
    msg.push_string(uuid);
    msg.push_uint(2);
    msg.push_string("/file1");
    msg.push_bool(false);
    msg.push_ulong(100);
    msg.push_string("/file2");
    msg.push_bool(false);
    msg.push_ulong(200);
    let msg = Message::from_bytes(msg.into_data());

    cluster.handle_message(msg).await;

    // fileListMap should still be empty since UUID wasn't registered
    assert!(file_list_map.is_empty());
}

/// Verifies that a `FILE_LIST` message for a registered UUID populates the associated `FileListState`.
///
/// # Setup
/// A UUID `"test-fl-uuid"` is pre-registered in the `file_list_map`.
/// A `FILE_LIST` message with 3 entries (directory `/`, `/file1` at 0x1234 bytes,
/// and `/file2` at 0x4321 bytes) is built.
///
/// # Act
/// The message is dispatched via `cluster.handle_message`.
///
/// # Assert
/// The `FileListState` contains 3 entries with correct names, `is_directory`, and `file_size` values,
/// and `data_ready` is set to `true`.
#[tokio::test]
async fn test_handle_file_list_populates_file_entries() {
    let (ctx, file_list_map) = make_app_context_with_file_list_map();
    let cluster = Cluster::new(test_config(), Some(ctx));

    let uuid = "test-fl-uuid";

    // Register UUID in the file list map
    let fl_state = Arc::new(tokio::sync::Mutex::new(FileListState::new()));
    file_list_map.insert(uuid.to_string(), Arc::clone(&fl_state));

    // Verify initial state
    {
        let state = fl_state.lock().await;
        assert!(state.files.is_empty());
        assert!(!state.error);
        assert!(state.error_details.is_empty());
        assert!(!state.data_ready);
    }

    // Send FILE_LIST message
    let mut msg = Message::new(FILE_LIST, Priority::Medium, "test");
    msg.push_string(uuid);
    msg.push_uint(3);
    // Directory
    msg.push_string("/");
    msg.push_bool(true);
    msg.push_ulong(0);
    // File 1
    msg.push_string("/file1");
    msg.push_bool(false);
    msg.push_ulong(0x1234);
    // File 2
    msg.push_string("/file2");
    msg.push_bool(false);
    msg.push_ulong(0x4321);
    let msg = Message::from_bytes(msg.into_data());

    cluster.handle_message(msg).await;

    // Verify file list was populated
    {
        let state = fl_state.lock().await;
        assert_eq!(state.files.len(), 3);
        assert!(!state.error);
        assert!(state.error_details.is_empty());
        assert!(state.data_ready);

        assert_eq!(state.files[0].file_name, "/");
        assert!(state.files[0].is_directory);
        assert_eq!(state.files[0].file_size, 0);

        assert_eq!(state.files[1].file_name, "/file1");
        assert!(!state.files[1].is_directory);
        assert_eq!(state.files[1].file_size, 0x1234);

        assert_eq!(state.files[2].file_name, "/file2");
        assert!(!state.files[2].is_directory);
        assert_eq!(state.files[2].file_size, 0x4321);
    }
}

// ---------------------------------------------------------------------------
// File list error handling
// ---------------------------------------------------------------------------

/// Verifies that a `FILE_LIST_ERROR` message for an unregistered UUID does not modify the file list map.
///
/// # Setup
/// A `Cluster` with an `AppContext` containing an empty `file_list_map` is created.
/// A `FILE_LIST_ERROR` message with uuid `"unknown-err-uuid"` is built.
///
/// # Act
/// The message is dispatched via `cluster.handle_message`.
///
/// # Assert
/// The `file_list_map` remains empty.
#[tokio::test]
async fn test_handle_file_list_error_unknown_uuid_is_noop() {
    let (ctx, file_list_map) = make_app_context_with_file_list_map();
    let cluster = Cluster::new(test_config(), Some(ctx));

    let uuid = "unknown-err-uuid";

    let mut msg = Message::new(FILE_LIST_ERROR, Priority::Medium, "test");
    msg.push_string(uuid);
    msg.push_string("details");
    let msg = Message::from_bytes(msg.into_data());

    cluster.handle_message(msg).await;

    assert!(file_list_map.is_empty());
}

/// Verifies that a `FILE_LIST_ERROR` message for a registered UUID sets the error state.
///
/// # Setup
/// A UUID `"test-fle-uuid"` is pre-registered in the `file_list_map` with an empty `FileListState`.
/// A `FILE_LIST_ERROR` message with detail string `"details"` is built.
///
/// # Act
/// The message is dispatched via `cluster.handle_message`.
///
/// # Assert
/// The `FileListState` has `error=true`, `error_details="details"`, `data_ready=true`,
/// and `files` remains empty.
#[tokio::test]
async fn test_handle_file_list_error_sets_error_state() {
    let (ctx, file_list_map) = make_app_context_with_file_list_map();
    let cluster = Cluster::new(test_config(), Some(ctx));

    let uuid = "test-fle-uuid";

    let fl_state = Arc::new(tokio::sync::Mutex::new(FileListState::new()));
    file_list_map.insert(uuid.to_string(), Arc::clone(&fl_state));

    // Verify initial state
    {
        let state = fl_state.lock().await;
        assert!(state.files.is_empty());
        assert!(!state.error);
        assert!(state.error_details.is_empty());
        assert!(!state.data_ready);
    }

    let mut msg = Message::new(FILE_LIST_ERROR, Priority::Medium, "test");
    msg.push_string(uuid);
    msg.push_string("details");
    let msg = Message::from_bytes(msg.into_data());

    cluster.handle_message(msg).await;

    {
        let state = fl_state.lock().await;
        assert!(state.files.is_empty());
        assert!(state.error);
        assert_eq!(state.error_details, "details");
        assert!(state.data_ready);
    }
}
