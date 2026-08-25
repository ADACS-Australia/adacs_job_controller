//! Security tests for Long-Term Secret Keys (LTK) authentication.

mod common;

use std::sync::Arc;

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
        mock_cluster
            .expect_set_connection()
            .returning(|_| Box::pin(async {}));
        mock_cluster.expect_role().returning(|| ClusterRole::Master);
        mock_cluster
            .expect_role_string()
            .returning(|| "master".to_string());
        mock_cluster
            .expect_cluster_details()
            .returning(|| test_cluster_config_with_ltk("ltk-test-cluster", "test-ltk-secret"));
        mock_cluster
            .expect_send_message()
            .returning(|_| Box::pin(async {}));
        Arc::new(mock_cluster)
    };

    let call_count = std::sync::Arc::new(std::sync::Mutex::new(0));
    let call_count_clone = call_count.clone();
    let cluster_for_first = cluster_arc.clone();

    let mut mgr = MockClusterManagerTrait::new();
    mgr.expect_get_file_download_admission().returning(|_| None);
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
    assert_eq!(result1.as_ref().unwrap().name(), "ltk-test-cluster");

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
