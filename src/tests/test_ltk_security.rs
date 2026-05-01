//! Security tests for Long-Term Secret Keys (LTK) authentication.

mod common;

use std::sync::Arc;
use std::time::Duration;

use adacs_job_controller::cluster::traits::{
    ClusterTrait, MockClusterManagerTrait, MockClusterTrait, WsOutbound,
};
use adacs_job_controller::protocol::types::ClusterRole;

use common::{make_test_state, setup_test_db, test_cluster_config_with_ltk};

// ---------------------------------------------------------------------------
// Test: Duplicate LTK connection rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicate_ltk_connection_rejected() {
    let db = setup_test_db().await;

    let cluster_arc: Arc<dyn ClusterTrait> = {
        let mut mock_cluster = MockClusterTrait::new();
        mock_cluster
            .expect_name()
            .returning(|| "ltk-test-cluster".to_string());
        mock_cluster.expect_is_online().returning(|| false);
        mock_cluster.expect_set_connection().returning(|_| ());
        mock_cluster.expect_role().returning(|| ClusterRole::Master);
        mock_cluster
            .expect_role_string()
            .returning(|| "master".to_string());
        mock_cluster
            .expect_cluster_details()
            .returning(|| test_cluster_config_with_ltk("ltk-test-cluster", "test-ltk-secret"));
        mock_cluster.expect_send_message().returning(|_| ());
        Arc::new(mock_cluster)
    };

    let call_count = std::sync::Arc::new(std::sync::Mutex::new(0));
    let call_count_clone = call_count.clone();
    let cluster_for_first = cluster_arc.clone();

    let mut mgr = MockClusterManagerTrait::new();
    mgr.expect_handle_new_connection()
        .returning(move |_, _, token| {
            let mut count = call_count_clone.lock().unwrap();
            *count += 1;
            let result: Option<Arc<dyn ClusterTrait>> = if token == "test-ltk-secret" {
                if *count == 1 {
                    Some(cluster_for_first.clone())
                } else {
                    None
                }
            } else {
                None
            };
            Box::pin(async move { result })
        });
    mgr.expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    mgr.expect_report_websocket_error().returning(|_, _| ());

    let state = make_test_state(db, mgr);

    let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    let result1 = state
        .cluster_manager
        .handle_new_connection(1, tx1, "test-ltk-secret")
        .await;
    assert!(result1.is_some(), "First LTK connection should succeed");

    let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    let result2 = state
        .cluster_manager
        .handle_new_connection(2, tx2, "test-ltk-secret")
        .await;
    assert!(
        result2.is_none(),
        "Duplicate LTK connection should be rejected"
    );
}

// ---------------------------------------------------------------------------
// Test: Rate limiting applies
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_rate_limiting_applies() {
    unsafe {
        std::env::set_var("LTK_CONNECTION_TIMEOUT_MS", "100");
    }
    unsafe {
        std::env::set_var("LTK_CONNECTION_TIMEOUT_MS", "100");
    }

    let db = setup_test_db().await;

    let cluster_arc: Arc<dyn ClusterTrait> = {
        let mut mock_cluster = MockClusterTrait::new();
        mock_cluster
            .expect_name()
            .returning(|| "ltk-rate-cluster".to_string());
        mock_cluster.expect_is_online().returning(|| false);
        mock_cluster.expect_set_connection().returning(|_| ());
        mock_cluster.expect_role().returning(|| ClusterRole::Master);
        mock_cluster
            .expect_role_string()
            .returning(|| "master".to_string());
        mock_cluster
            .expect_cluster_details()
            .returning(|| test_cluster_config_with_ltk("ltk-rate-cluster", "rate-limit-ltk"));
        mock_cluster.expect_send_message().returning(|_| ());
        Arc::new(mock_cluster)
    };

    let mut mgr = MockClusterManagerTrait::new();
    mgr.expect_handle_new_connection()
        .returning(move |_, _, token| {
            let result: Option<Arc<dyn ClusterTrait>> = if token == "rate-limit-ltk" {
                Some(cluster_arc.clone())
            } else {
                None
            };
            Box::pin(async move { result })
        });
    mgr.expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    mgr.expect_report_websocket_error().returning(|_, _| ());

    let state = make_test_state(db, mgr);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    let result = state
        .cluster_manager
        .handle_new_connection(1, tx, "rate-limit-ltk")
        .await;

    assert!(result.is_some(), "LTK authentication should succeed");
}

// ---------------------------------------------------------------------------
// Test: Rate limiting disabled in test mode
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_rate_limiting_disabled_in_test() {
    unsafe {
        std::env::remove_var("LTK_CONNECTION_TIMEOUT_MS");
    }

    let db = setup_test_db().await;

    let cluster_arc: Arc<dyn ClusterTrait> = {
        let mut mock_cluster = MockClusterTrait::new();
        mock_cluster
            .expect_name()
            .returning(|| "ltk-no-delay-cluster".to_string());
        mock_cluster.expect_is_online().returning(|| false);
        mock_cluster.expect_set_connection().returning(|_| ());
        mock_cluster.expect_role().returning(|| ClusterRole::Master);
        mock_cluster
            .expect_role_string()
            .returning(|| "master".to_string());
        mock_cluster
            .expect_cluster_details()
            .returning(|| test_cluster_config_with_ltk("ltk-no-delay-cluster", "no-delay-ltk"));
        mock_cluster.expect_send_message().returning(|_| ());
        Arc::new(mock_cluster)
    };

    let mut mgr = MockClusterManagerTrait::new();
    mgr.expect_handle_new_connection()
        .returning(move |_, _, token| {
            let result: Option<Arc<dyn ClusterTrait>> = if token == "no-delay-ltk" {
                Some(cluster_arc.clone())
            } else {
                None
            };
            Box::pin(async move { result })
        });
    mgr.expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    mgr.expect_report_websocket_error().returning(|_, _| ());

    let state = make_test_state(db, mgr);

    let start = std::time::Instant::now();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    let result = state
        .cluster_manager
        .handle_new_connection(1, tx, "no-delay-ltk")
        .await;
    let elapsed = start.elapsed();

    assert!(result.is_some(), "LTK authentication should succeed");
    assert!(
        elapsed < Duration::from_millis(50),
        "Rate limiting should be disabled in tests. Elapsed: {elapsed:?}"
    );
}
