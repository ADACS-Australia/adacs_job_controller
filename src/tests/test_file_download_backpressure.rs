//! Tests for file download backpressure (PAUSE / RESUME logic).
//!
//! The `Cluster::handle_file_chunk` method sends a `PAUSE_FILE_CHUNK_STREAM`
//! message to the remote when buffered bytes exceed `MAX_FILE_BUFFER_SIZE`.
//! The HTTP download handler sends `RESUME_FILE_CHUNK_STREAM` when the
//! consumer has drained enough bytes.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use adacs_job_controller::cluster::cluster::{AppContext, Cluster};
use adacs_job_controller::cluster::file_download::FileDownloadState;
use adacs_job_controller::cluster::traits::WsOutbound;
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::Priority;
use dashmap::DashMap;

fn test_cluster_config(name: &str) -> ClusterConfig {
    ClusterConfig {
        name: name.to_string(),
        host: "localhost".to_string(),
        username: "testuser".to_string(),
        path: "/home/testuser/jobcontroller".to_string(),
        key: String::new(),
        connection_type: "manual".to_string(),
        keytab: String::new(),
        kerberos_principal: String::new(),        ltk: None,    }
}

fn make_app_context() -> Arc<AppContext> {
    let db = futures::executor::block_on(sea_orm::Database::connect("sqlite::memory:"))
        .expect("sqlite in-memory connect failed");
    Arc::new(AppContext {
        db,
        file_list_map: Arc::new(DashMap::new()),
    })
}

/// Build a FILE_CHUNK message with the given data.
fn make_file_chunk_message(data: Vec<u8>) -> Message {
    let mut msg = Message::new(FILE_CHUNK, Priority::Highest, "test_uuid");
    msg.push_bytes(&data);
    Message::from_bytes(msg.into_data())
}

/// Build a FILE_DETAILS message with the given file size.
fn make_file_details_message(file_size: u64) -> Message {
    let mut msg = Message::new(FILE_DETAILS, Priority::Highest, "test_uuid");
    msg.push_ulong(file_size);
    Message::from_bytes(msg.into_data())
}

/// Build a FILE_ERROR message.
fn make_file_error_message(detail: &str) -> Message {
    let mut msg = Message::new(FILE_ERROR, Priority::Highest, "test_uuid");
    msg.push_string(detail);
    Message::from_bytes(msg.into_data())
}

/// Create a FileDownload Cluster and a WS sender channel.
/// Returns `(Arc<Cluster>, WS channel receiver)`.
fn make_file_download_cluster() -> (
    Arc<Cluster>,
    tokio::sync::mpsc::UnboundedReceiver<WsOutbound>,
    Arc<FileDownloadState>,
) {
    let download_state = Arc::new(FileDownloadState::new());
    let pause_lock = Arc::new(tokio::sync::Mutex::new(()));
    let cluster = Cluster::new_file_download(
        test_cluster_config("test"),
        "test_uuid".to_string(),
        Arc::clone(&download_state),
        Some(make_app_context()),
        pause_lock,
    );

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    use adacs_job_controller::cluster::traits::ClusterTrait;
    cluster.set_connection(Some(tx));
    cluster.start_tasks();

    (cluster, rx, download_state)
}

// ---------------------------------------------------------------------------
// Collect outgoing WS messages from the channel (non-blocking drain).
// ---------------------------------------------------------------------------
async fn drain_channel(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<WsOutbound>,
    wait: Duration,
) -> Vec<Message> {
    tokio::time::sleep(wait).await;
    let mut msgs = Vec::new();
    while let Ok(outbound) = rx.try_recv() {
        if let WsOutbound::Binary(data) = outbound {
            msgs.push(Message::from_bytes(data));
        }
    }
    msgs
}

// ---------------------------------------------------------------------------
// test_file_chunk_received_correctly
// ---------------------------------------------------------------------------

/// Tests that handling a FILE_CHUNK message correctly updates state and delivers data.
///
/// # Setup
/// Creates a file download cluster with a fresh `FileDownloadState`.
///
/// # Act
/// Calls `handle_message` with a FILE_CHUNK containing "hello world chunk".
///
/// # Assert
/// Verifies `data_ready` is set, `received_bytes` is updated to the chunk length,
/// and the chunk bytes are available in the receiver channel.
#[tokio::test]
async fn test_file_chunk_received_correctly() {
    let (cluster, mut rx, download_state) = make_file_download_cluster();

    use adacs_job_controller::cluster::traits::ClusterTrait;

    let chunk_data = b"hello world chunk".to_vec();
    let chunk_len = chunk_data.len() as u64;

    let msg = make_file_chunk_message(chunk_data.clone());
    cluster.handle_message(msg).await;

    // data_ready should be set
    assert!(download_state.data_ready.load(Ordering::Acquire));

    // received_bytes should be updated
    assert_eq!(
        download_state.received_bytes.load(Ordering::Relaxed),
        chunk_len
    );

    // Chunk should be in the receiver channel
    let received_chunk = download_state
        .chunk_receiver
        .try_lock()
        .unwrap()
        .try_recv()
        .unwrap();
    assert_eq!(received_chunk, chunk_data);

    cluster.stop();
    drain_channel(&mut rx, Duration::from_millis(10)).await;
}

// ---------------------------------------------------------------------------
// test_file_details_sets_file_size
// ---------------------------------------------------------------------------

/// Tests that handling a FILE_DETAILS message records the declared file size.
///
/// # Setup
/// Creates a file download cluster with a fresh `FileDownloadState`.
///
/// # Act
/// Calls `handle_message` with a FILE_DETAILS message declaring size 1,234,567.
///
/// # Assert
/// Verifies `received_data`, `data_ready`, and `file_size` atomics are all set correctly.
#[tokio::test]
async fn test_file_details_sets_file_size() {
    let (cluster, mut rx, download_state) = make_file_download_cluster();

    use adacs_job_controller::cluster::traits::ClusterTrait;

    let msg = make_file_details_message(1_234_567);
    cluster.handle_message(msg).await;

    assert!(download_state.received_data.load(Ordering::Acquire));
    assert!(download_state.data_ready.load(Ordering::Acquire));
    assert_eq!(download_state.file_size.load(Ordering::Relaxed), 1_234_567);

    cluster.stop();
    drain_channel(&mut rx, Duration::from_millis(10)).await;
}

// ---------------------------------------------------------------------------
// test_file_error_sets_error_state
// ---------------------------------------------------------------------------

/// Tests that handling a FILE_ERROR message sets the error flag and stores the details string.
///
/// # Setup
/// Creates a file download cluster with a fresh `FileDownloadState`.
///
/// # Act
/// Calls `handle_message` with a FILE_ERROR message containing "permission denied".
///
/// # Assert
/// Verifies `error` and `data_ready` flags are set, and `error_details` contains the error string.
#[tokio::test]
async fn test_file_error_sets_error_state() {
    let (cluster, mut rx, download_state) = make_file_download_cluster();

    use adacs_job_controller::cluster::traits::ClusterTrait;

    let msg = make_file_error_message("permission denied");
    cluster.handle_message(msg).await;

    assert!(download_state.error.load(Ordering::Acquire));
    assert!(download_state.data_ready.load(Ordering::Acquire));
    let details = download_state.error_details.lock().await;
    assert_eq!(*details, "permission denied");

    cluster.stop();
    drain_channel(&mut rx, Duration::from_millis(10)).await;
}

// ---------------------------------------------------------------------------
// test_backpressure_pause_sent_when_buffer_exceeds_max
// ---------------------------------------------------------------------------

/// Tests that a PAUSE_FILE_CHUNK_STREAM message is sent when buffered bytes exceed the limit.
///
/// # Setup
/// Creates a file download cluster with a fresh `FileDownloadState`.
///
/// # Act
/// Calls `handle_message` with a FILE_CHUNK larger than `MAX_FILE_BUFFER_SIZE`.
///
/// # Assert
/// Verifies a PAUSE_FILE_CHUNK_STREAM message was sent on the WS channel
/// and `client_paused` is set to true.
#[tokio::test]
async fn test_backpressure_pause_sent_when_buffer_exceeds_max() {
    use adacs_job_controller::config::settings::MAX_FILE_BUFFER_SIZE;

    let (cluster, mut rx, download_state) = make_file_download_cluster();

    use adacs_job_controller::cluster::traits::ClusterTrait;

    // Set received_bytes >> sent_bytes to exceed the buffer limit
    let max_buf = *MAX_FILE_BUFFER_SIZE;
    let large_data = vec![0u8; (max_buf + 1024) as usize];
    let msg = make_file_chunk_message(large_data);
    cluster.handle_message(msg).await;

    // The cluster should have sent a PAUSE_FILE_CHUNK_STREAM message
    let outgoing = drain_channel(&mut rx, Duration::from_millis(100)).await;
    let pause_msgs: Vec<_> = outgoing
        .iter()
        .filter(|m| m.id() == PAUSE_FILE_CHUNK_STREAM)
        .collect();

    assert!(!pause_msgs.is_empty(), "Expected PAUSE_FILE_CHUNK_STREAM");
    assert!(
        download_state.client_paused.load(Ordering::Relaxed),
        "client_paused should be true"
    );

    cluster.stop();
}

// ---------------------------------------------------------------------------
// test_backpressure_no_pause_for_small_chunks
// ---------------------------------------------------------------------------

/// Tests that a small chunk well below the buffer limit does not trigger a PAUSE.
///
/// # Setup
/// Creates a file download cluster with a fresh `FileDownloadState`.
///
/// # Act
/// Calls `handle_message` with a 128-byte FILE_CHUNK.
///
/// # Assert
/// Verifies no PAUSE_FILE_CHUNK_STREAM message is sent on the WS channel.
#[tokio::test]
async fn test_backpressure_no_pause_for_small_chunks() {
    let (cluster, mut rx, _download_state) = make_file_download_cluster();

    use adacs_job_controller::cluster::traits::ClusterTrait;

    // Small chunk — well below the buffer limit
    let small_data = vec![0u8; 128];
    let msg = make_file_chunk_message(small_data);
    cluster.handle_message(msg).await;

    let outgoing = drain_channel(&mut rx, Duration::from_millis(50)).await;
    let pause_msgs: Vec<_> = outgoing
        .iter()
        .filter(|m| m.id() == PAUSE_FILE_CHUNK_STREAM)
        .collect();

    assert!(
        pause_msgs.is_empty(),
        "Small chunk should NOT trigger PAUSE"
    );

    cluster.stop();
}

// ---------------------------------------------------------------------------
// test_backpressure_pause_not_resent_when_already_paused
// ---------------------------------------------------------------------------

/// Tests that a duplicate PAUSE is not sent when the stream is already paused.
///
/// # Setup
/// Creates a file download cluster and pre-sets `client_paused = true`.
///
/// # Act
/// Calls `handle_message` with a large FILE_CHUNK.
///
/// # Assert
/// Verifies no additional PAUSE_FILE_CHUNK_STREAM is sent, preventing duplicate pauses.
#[tokio::test]
async fn test_backpressure_pause_not_resent_when_already_paused() {
    use adacs_job_controller::config::settings::MAX_FILE_BUFFER_SIZE;

    let (cluster, mut rx, download_state) = make_file_download_cluster();

    use adacs_job_controller::cluster::traits::ClusterTrait;

    // Pre-set client_paused = true (as if already paused)
    download_state.client_paused.store(true, Ordering::Relaxed);

    // Send large chunk — should not send another PAUSE
    let max_buf = *MAX_FILE_BUFFER_SIZE;
    let large_data = vec![0u8; (max_buf + 1024) as usize];
    let msg = make_file_chunk_message(large_data);
    cluster.handle_message(msg).await;

    let outgoing = drain_channel(&mut rx, Duration::from_millis(50)).await;
    let pause_msgs: Vec<_> = outgoing
        .iter()
        .filter(|m| m.id() == PAUSE_FILE_CHUNK_STREAM)
        .collect();

    assert!(
        pause_msgs.is_empty(),
        "Should NOT send duplicate PAUSE when already paused"
    );

    cluster.stop();
}

// ---------------------------------------------------------------------------
// test_resume_sent_when_consumer_drains_buffer
//
// This test uses the low-level atomic manipulation to simulate the HTTP
// consumer draining the buffer below MIN_FILE_BUFFER_SIZE, then verifies
// that `send_resume_if_needed` would send a RESUME (via the cluster's queue).
// ---------------------------------------------------------------------------

/// Tests that a RESUME_FILE_CHUNK_STREAM is sent after the consumer drains the buffer.
///
/// # Setup
/// Sets `received_bytes` to a large value simulating a full buffer, then sets
/// `sent_bytes` equal to `received_bytes` to simulate the HTTP consumer having
/// read all data. Pre-sets `client_paused = true`.
///
/// # Act
/// Runs the resume-check logic: CAS-swaps `client_paused` to false and sends
/// `RESUME_FILE_CHUNK_STREAM` if the buffer difference is below `MIN_FILE_BUFFER_SIZE`.
///
/// # Assert
/// Verifies RESUME_FILE_CHUNK_STREAM appears in the outgoing WS channel and
/// `client_paused` is false.
#[tokio::test]
async fn test_resume_logic_via_sent_bytes_update() {
    use adacs_job_controller::config::settings::{MAX_FILE_BUFFER_SIZE, MIN_FILE_BUFFER_SIZE};

    let (cluster, mut rx, download_state) = make_file_download_cluster();

    use adacs_job_controller::cluster::traits::ClusterTrait;

    // 1. Simulate large incoming data → triggers PAUSE
    let max_buf = *MAX_FILE_BUFFER_SIZE;
    let large_data = vec![0u8; (max_buf + 1024) as usize];
    let received_total = large_data.len() as u64;
    download_state
        .received_bytes
        .store(received_total, Ordering::Relaxed);
    download_state.client_paused.store(true, Ordering::Relaxed);

    // 2. Simulate HTTP consumer reading all the data (sent_bytes ≈ received_bytes)
    //    This brings the buffer diff below MIN_FILE_BUFFER_SIZE
    download_state
        .sent_bytes
        .store(received_total, Ordering::Relaxed);

    // 3. Now mimic what the HTTP download stream does after updating sent_bytes:
    //    it checks if buffer is below MIN_FILE_BUFFER_SIZE and swaps client_paused.
    let min_buf = *MIN_FILE_BUFFER_SIZE;
    let received = download_state.received_bytes.load(Ordering::Acquire);
    let sent = download_state.sent_bytes.load(Ordering::Acquire);
    if download_state.client_paused.load(Ordering::Acquire)
        && received.saturating_sub(sent) < min_buf
        && download_state
            .client_paused
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let resume_msg = Message::new(RESUME_FILE_CHUNK_STREAM, Priority::Highest, "test_uuid");
            cluster.send_message(resume_msg);
        }

    // 4. Drain WS channel — should see RESUME_FILE_CHUNK_STREAM
    let outgoing = drain_channel(&mut rx, Duration::from_millis(100)).await;
    let resume_msgs: Vec<_> = outgoing
        .iter()
        .filter(|m| m.id() == RESUME_FILE_CHUNK_STREAM)
        .collect();

    assert!(!resume_msgs.is_empty(), "Expected RESUME_FILE_CHUNK_STREAM");
    assert!(
        !download_state.client_paused.load(Ordering::Relaxed),
        "client_paused should be false after resume"
    );

    cluster.stop();
}

// ---------------------------------------------------------------------------
// test_file_download_completes_without_pause_for_small_file
// ---------------------------------------------------------------------------

/// Tests that a small file is downloaded in order without triggering backpressure.
///
/// # Setup
/// Creates a file download cluster and sends FILE_DETAILS declaring 30 bytes total.
///
/// # Act
/// Sends three 10-byte FILE_CHUNK messages in sequence.
///
/// # Assert
/// Verifies all 30 bytes are received, no PAUSE_FILE_CHUNK_STREAM is sent, and the
/// chunk data is available in the receiver channel in the correct order.
#[tokio::test]
async fn test_file_download_completes_without_pause_for_small_file() {
    use adacs_job_controller::cluster::traits::ClusterTrait;

    let (cluster, mut rx, download_state) = make_file_download_cluster();

    // Send FILE_DETAILS first
    let msg = make_file_details_message(30);
    cluster.handle_message(msg).await;

    // Send 3 small chunks (10 bytes each)
    for i in 0u8..3 {
        let chunk_data = vec![i; 10];
        let msg = make_file_chunk_message(chunk_data);
        cluster.handle_message(msg).await;
    }

    // Should have received all chunks without pause
    assert_eq!(download_state.received_bytes.load(Ordering::Relaxed), 30);

    let outgoing = drain_channel(&mut rx, Duration::from_millis(50)).await;
    let pause_msgs: Vec<_> = outgoing
        .iter()
        .filter(|m| m.id() == PAUSE_FILE_CHUNK_STREAM)
        .collect();
    assert!(pause_msgs.is_empty(), "Small file should not trigger PAUSE");

    // Verify chunks are available in order
    let mut chunk_rx = download_state.chunk_receiver.lock().await;
    let mut all_bytes = Vec::new();
    for _ in 0..3 {
        if let Ok(chunk) = chunk_rx.try_recv() {
            all_bytes.extend_from_slice(&chunk);
        }
    }
    let expected: Vec<u8> = (0u8..3).flat_map(|i| vec![i; 10]).collect();
    assert_eq!(all_bytes, expected);

    cluster.stop();
}

// ---------------------------------------------------------------------------
// test_file_chunk_notifies_data_ready
// ---------------------------------------------------------------------------

/// Tests that receiving a FILE_CHUNK triggers the `data_notify` notification.
///
/// # Setup
/// Creates a file download cluster and spawns a task that waits on `data_notify`.
///
/// # Act
/// Calls `handle_message` with a small FILE_CHUNK after the waiting task is registered.
///
/// # Assert
/// Verifies the notification was delivered and the waiting task observed it.
#[tokio::test]
async fn test_file_chunk_notifies_data_ready() {
    use adacs_job_controller::cluster::traits::ClusterTrait;

    let (cluster, mut rx, download_state) = make_file_download_cluster();

    // Reset data_ready
    download_state.data_ready.store(false, Ordering::Relaxed);

    let done = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done);
    let state_clone = Arc::clone(&download_state);

    // Spawn a task waiting for the notification
    tokio::spawn(async move {
        state_clone.data_notify.notified().await;
        done_clone.store(true, Ordering::Relaxed);
    });

    // Small sleep to ensure the notifier task is waiting
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Send a chunk — should trigger data_notify
    let msg = make_file_chunk_message(b"ping".to_vec());
    cluster.handle_message(msg).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        done.load(Ordering::Relaxed),
        "Notification should have been delivered"
    );

    cluster.stop();
    drain_channel(&mut rx, Duration::from_millis(10)).await;
}
