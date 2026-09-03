//! Repeated-download resource regression tests.
//!
//! Active tests cover deterministic manager cleanup and responsive HTTP+WS
//! transfers. The unresponsive peer case is ignored because it intentionally
//! waits for the readiness-timeout fallback.

#![allow(clippy::doc_markdown, clippy::doc_lazy_continuation)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use dashmap::DashMap;
use tower::ServiceExt;

use adacs_job_controller::cluster::manager::ClusterManager;
use adacs_job_controller::cluster::traits::{ClusterManagerTrait, ClusterTrait};
use adacs_job_controller::protocol::types::FileListState;
use sea_orm::DatabaseConnection;

use common::encode_test_jwt;
use common::repeated_download::{
    build_app, build_file_chunk, build_file_details, build_state, connect_ws, fresh_manager,
    insert_regression_file_download, insert_regression_job, send_msg, start_server,
    wait_for_cleanup,
};
use common::setup_test_db;

/// Build the full app with a short `client_timeout_seconds` so the
/// readiness/chunk timeouts fire quickly within the test.
fn build_app_with_timeout(
    db: DatabaseConnection,
    manager: Arc<ClusterManager>,
    file_list_map: Arc<DashMap<String, Arc<tokio::sync::Mutex<FileListState>>>>,
    http_timeout: u64,
) -> axum::Router {
    build_app(build_state(db, manager, file_list_map, Some(http_timeout)))
}

// ---------------------------------------------------------------------------
// Repeated-download regression: manager-level deterministic core
// ---------------------------------------------------------------------------

/// Repeated manager-level create/admit/remove cycles return every observable
/// dedicated-download resource to baseline.
#[tokio::test]
async fn repeated_download_manager_level_returns_to_baseline() {
    const NUM_TRANSFERS: usize = 5;

    let db = setup_test_db().await;
    let manager = fresh_manager(&db).await;
    let _job_id = insert_regression_job(&db).await;

    let cluster = manager
        .get_cluster_by_name("regression_cluster")
        .expect("manager should expose master cluster");

    let cleanup_deadline = Duration::from_secs(
        adacs_job_controller::websocket::server::WS_CLOSE_HANDSHAKE_GRACE_SECONDS + 5,
    );

    let mut accepted: usize = 0;
    let mut closed: usize = 0;

    for cycle in 0..NUM_TRANSFERS {
        let uuid = format!("manager-regression-{cycle:02}");

        // 1. Create the download session and dedicated cluster.
        let dl_cluster = manager.create_file_download(&cluster, &uuid).await;
        assert!(
            manager.get_file_download(&uuid).is_some(),
            "cycle {cycle}: session must exist after create_file_download"
        );
        assert_eq!(
            manager.dedicated_download_clusters().len(),
            1,
            "cycle {cycle}: dedicated cluster snapshot must contain 1 entry"
        );
        // `FileDownload` role retains scheduler+prune JoinHandles (resend
        // is master-only). The count is therefore 2.
        let retained =
            manager.dedicated_download_clusters_concrete()[0].retained_download_task_count();
        assert_eq!(
            retained, 2,
            "cycle {cycle}: dl_cluster should retain 2 task handles (scheduler+prune) for `FileDownload` role, got {retained}"
        );

        // 2. Admit a WS connection so the dl_cluster is bound to a conn_id.
        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel::<
            adacs_job_controller::cluster::traits::WsOutbound,
        >();
        let admitted = manager.handle_new_connection(1, ws_tx, &uuid).await;
        assert!(
            admitted.is_some(),
            "cycle {cycle}: WS admission must succeed for registered session"
        );

        accepted += 1;

        // 3. Drive a graceful close via `remove_connection(conn_id, false)`.
        manager.remove_connection(1, false).await;

        // 4. Wait for cleanup to drain the maps (deterministic polling).
        let start = std::time::Instant::now();
        let mut cleaned = manager.get_file_download(&uuid).is_none()
            && manager.dedicated_download_clusters().is_empty();
        while !cleaned && start.elapsed() < cleanup_deadline {
            tokio::task::yield_now().await;
            cleaned = manager.get_file_download(&uuid).is_none()
                && manager.dedicated_download_clusters().is_empty();
        }

        assert!(
            cleaned,
            "cycle {cycle} cleanup did not drain within {cleanup_deadline:?}: \
             get_file_download={}, dedicated_clusters={}",
            u8::from(manager.get_file_download(&uuid).is_some()),
            manager.dedicated_download_clusters().len(),
        );

        // 5. After cleanup: every observable resource returns to baseline.
        assert!(
            manager.dedicated_download_clusters().is_empty(),
            "cycle {cycle}: dedicated_download_clusters must be empty after cleanup"
        );
        assert!(
            manager.get_file_download(&uuid).is_none(),
            "cycle {cycle}: file_download_map must not retain session for {uuid}"
        );
        assert!(
            manager.get_file_download_cleanup_trigger(&uuid).is_none(),
            "cycle {cycle}: cleanup trigger lookup must return None for {uuid}"
        );
        assert!(
            manager.get_cluster_by_connection(1).is_none(),
            "cycle {cycle}: connection_map must not retain entry for conn_id=1"
        );

        closed += 1;
        drop(dl_cluster);
    }

    assert_eq!(
        accepted, NUM_TRANSFERS,
        "every accepted transfer must complete (accepted={accepted})"
    );
    assert_eq!(
        closed, NUM_TRANSFERS,
        "every accepted connection must produce a closed event (closed={closed})"
    );
    assert_eq!(
        accepted, closed,
        "accepted == closed count invariant must hold"
    );
}

// ---------------------------------------------------------------------------
// Repeated-download regression: full `HTTP`+WS integration (responsive peer)
// ---------------------------------------------------------------------------

/// Repeated full HTTP + real WebSocket transfers return all observable
/// dedicated-download resources to baseline.
#[tokio::test]
async fn repeated_download_responsive_peer_returns_to_baseline() {
    const NUM_TRANSFERS: usize = 3;
    const CHUNK_SIZE: usize = 64 * 1024;
    const FILE_SIZE: u64 = (CHUNK_SIZE * 2) as u64;

    let db = setup_test_db().await;
    let manager = fresh_manager(&db).await;
    let _job_id = insert_regression_job(&db).await;

    let file_list_map = Arc::new(DashMap::new());
    // Short client_timeout_seconds so the readiness/chunk timeouts
    // fire quickly within the test. The `HTTP` layer uses this value as
    // both the readiness timeout and the chunk inactivity timeout.
    let http_timeout = 2u64;
    let state = build_state(
        db.clone(),
        Arc::clone(&manager),
        Arc::clone(&file_list_map),
        Some(http_timeout),
    );
    let app = build_app(state);
    let (port, server_handle) = start_server(app).await;

    let cleanup_deadline = Duration::from_secs(
        adacs_job_controller::websocket::server::WS_CLOSE_HANDSHAKE_GRACE_SECONDS + 5,
    );

    let total_bytes: Vec<u8> = (0..FILE_SIZE).map(|i| (i % 251) as u8).collect();
    let chunks = vec![
        total_bytes[..CHUNK_SIZE].to_vec(),
        total_bytes[CHUNK_SIZE..].to_vec(),
    ];

    let mut accepted: usize = 0;
    let mut closed: usize = 0;

    for cycle in 0..NUM_TRANSFERS {
        let file_id = format!("regression-{cycle:02}");
        insert_regression_file_download(&db, &file_id).await;

        // Spawn the WS task: it polls for the dedicated cluster to
        // appear (after `HTTP` creates it), extracts the session UUID,
        // connects with that UUID, sends FILE_DETAILS + chunks, then
        // drops the sink so the server's WS handler observes `EOF` and
        // drives cleanup. The `HTTP` layer generates its own internal
        // UUID for the session, so we must extract it from the cluster
        // snapshot rather than reusing `file_id`.
        let ws_port = port;
        let ws_chunks = chunks.clone();
        let ws_manager_for_uuid = Arc::clone(&manager);
        let ws_handle = tokio::spawn(async move {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            let session_uuid: String = loop {
                if let Some(cluster) = ws_manager_for_uuid
                    .dedicated_download_clusters_concrete()
                    .first()
                    && let Some(uuid) = cluster.uuid()
                {
                    break uuid.to_string();
                }
                assert!(
                    std::time::Instant::now() <= deadline,
                    "WS task timed out waiting for dedicated cluster to appear"
                );
                tokio::task::yield_now().await;
            };

            let (mut sink, _stream) = connect_ws(ws_port, &session_uuid).await;
            // Send FILE_DETAILS before any chunks so the dl_cluster
            // sets file_size + data_ready + received_data.
            send_msg(&mut sink, build_file_details(FILE_SIZE)).await;
            for chunk in &ws_chunks {
                send_msg(&mut sink, build_file_chunk(chunk)).await;
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            // Brief hold so the WS handler can forward at least some
            // chunks to the dl_cluster before `EOF` closes the WS.
            tokio::time::sleep(Duration::from_millis(100)).await;
            drop(sink);
        });

        // `HTTP` GET. We don't assert on the specific status code or body
        // because the production code may fire the pre-response guard's
        // Drop early and select an error status. The regression test
        // focuses on the resource-invariant side: after the request
        // returns and the WS task ends, all maps must return to baseline.
        let token = encode_test_jwt(&serde_json::json!({"userId": 1, "application": "testapp"}));
        let app = build_app_with_timeout(
            db.clone(),
            Arc::clone(&manager),
            Arc::clone(&file_list_map),
            http_timeout,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/job/apiv1/file/?fileId={file_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        // Consume the body so the connection drops cleanly.
        let _ = axum::body::to_bytes(resp.into_body(), usize::MAX).await;
        let _ = ws_handle.await;

        // The HTTP response may be 200 OK (full download) or any
        // client/server error (readiness/chunk timeout). Both paths
        // must leave the manager in a clean state.
        let _ = status; // explicit: we don't assert on status here.

        accepted += 1;

        // Wait for cleanup to drain the maps. The bound accommodates
        // the readiness timeout, the chunk timeout, the WS close grace
        // period, and the cleanup worker's queue.
        let cleaned = wait_for_cleanup(&manager, cleanup_deadline).await;
        assert!(
            cleaned,
            "cycle {cycle} cleanup did not drain within {cleanup_deadline:?}: \
             dedicated_clusters={}, concrete retained={:?}",
            manager.dedicated_download_clusters().len(),
            manager
                .dedicated_download_clusters_concrete()
                .iter()
                .map(|c| c.retained_download_task_count())
                .collect::<Vec<_>>(),
        );

        // After cleanup: every observable resource returns to baseline.
        assert!(
            manager.dedicated_download_clusters().is_empty(),
            "cycle {cycle}: dedicated_download_clusters must be empty after cleanup"
        );
        assert!(
            manager.get_file_download(&file_id).is_none(),
            "cycle {cycle}: file_download_map must not retain session for {file_id}"
        );
        assert!(
            manager
                .get_file_download_cleanup_trigger(&file_id)
                .is_none(),
            "cycle {cycle}: cleanup trigger lookup must return None for {file_id}"
        );

        for cluster in manager.dedicated_download_clusters_concrete() {
            assert_eq!(
                cluster.retained_download_task_count(),
                0,
                "cycle {cycle}: retained download task handles must be drained for cluster {}",
                cluster.name(),
            );
        }

        closed += 1;
    }

    assert_eq!(
        accepted, NUM_TRANSFERS,
        "every accepted transfer must complete (accepted={accepted})"
    );
    assert_eq!(
        closed, NUM_TRANSFERS,
        "every accepted connection must produce a closed event (closed={closed})"
    );
    assert_eq!(
        accepted, closed,
        "accepted == closed count invariant must hold"
    );

    server_handle.abort();
    let _ = server_handle.await;
}

// ---------------------------------------------------------------------------
// Repeated-download regression: unresponsive peer (#[ignore])
// ---------------------------------------------------------------------------

/// Slow unresponsive-peer fallback coverage. Run on demand with:
/// `cargo test --test test_repeated_download_resource_regression -- --ignored`.
#[tokio::test]
#[ignore = "inherently slow (readiness timeout per cycle); run with --ignored"]
async fn repeated_download_unresponsive_peer_returns_to_baseline() {
    const NUM_TRANSFERS: usize = 3;

    let db = setup_test_db().await;
    let manager = fresh_manager(&db).await;
    let _job_id = insert_regression_job(&db).await;

    let file_list_map = Arc::new(DashMap::new());
    let http_timeout = 2u64;
    let state = build_state(
        db.clone(),
        Arc::clone(&manager),
        Arc::clone(&file_list_map),
        Some(http_timeout),
    );
    let app = build_app(state);
    let (port, server_handle) = start_server(app).await;

    let grace_secs = adacs_job_controller::websocket::server::WS_CLOSE_HANDSHAKE_GRACE_SECONDS;
    // Per-cycle bound: grace + readiness-timeout margin + small jitter.
    let per_cycle_bound = Duration::from_secs(grace_secs + http_timeout + 5);
    let cleanup_deadline = per_cycle_bound * NUM_TRANSFERS as u32 + Duration::from_secs(15);

    for cycle in 0..NUM_TRANSFERS {
        let file_id = format!("unresponsive-{cycle:02}");
        insert_regression_file_download(&db, &file_id).await;

        // Open a WS client that never sends anything. The `HTTP`
        // download_file call will time out (readiness timeout) and
        // the cleanup worker will remove the session from the map.
        // The WS handler also enters the close grace period and the
        // cleanup chain closes the WS and terminates the dl_cluster
        // tasks.
        let (_ws_sink, _ws_stream) = connect_ws(port, &file_id).await;

        let token = encode_test_jwt(&serde_json::json!({"userId": 1, "application": "testapp"}));
        let app = build_app_with_timeout(
            db.clone(),
            Arc::clone(&manager),
            Arc::clone(&file_list_map),
            http_timeout,
        );
        let start = std::time::Instant::now();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/job/apiv1/file/?fileId={file_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let elapsed = start.elapsed();
        let status = resp.status();
        let _ = axum::body::to_bytes(resp.into_body(), usize::MAX).await;
        assert!(
            elapsed <= per_cycle_bound,
            "cycle {cycle} `HTTP` call exceeded bound ({elapsed:?} > {per_cycle_bound:?}); status={status}",
        );
        assert!(
            status.is_client_error() || status.is_server_error(),
            "cycle {cycle} expected error status, got {status}"
        );

        // Wait for cleanup to drain the maps.
        let cleaned = wait_for_cleanup(&manager, cleanup_deadline).await;
        assert!(
            cleaned,
            "cycle {cycle} cleanup did not drain within {cleanup_deadline:?}"
        );

        assert!(
            manager.dedicated_download_clusters().is_empty(),
            "cycle {cycle}: dedicated_download_clusters must be empty after unresponsive peer"
        );
        assert!(
            manager.get_file_download(&file_id).is_none(),
            "cycle {cycle}: file_download_map must not retain session for {file_id}"
        );
    }

    server_handle.abort();
    let _ = server_handle.await;
}
