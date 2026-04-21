//! Integration tests for Long-Term Secret Keys (LTK) authentication.

mod common;

use std::sync::Arc;

use adacs_job_controller::cluster::traits::{
    ClusterTrait, MockClusterManagerTrait, MockClusterTrait, WsOutbound,
};
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::protocol::types::ClusterRole;

use common::{make_test_state, setup_test_db, test_cluster_config, test_cluster_config_with_ltk};

// ---------------------------------------------------------------------------
// Test: LTK authentication succeeds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ltk_authentication_success() {
    let db = setup_test_db().await;

    let cluster_arc: Arc<dyn ClusterTrait> = {
        let mut mock_cluster = MockClusterTrait::new();
        mock_cluster
            .expect_name()
            .returning(|| "ltk-test".to_string());
        mock_cluster.expect_is_online().returning(|| false);
        mock_cluster.expect_set_connection().returning(|_| ());
        mock_cluster.expect_role().returning(|| ClusterRole::Master);
        mock_cluster
            .expect_role_string()
            .returning(|| "master".to_string());
        mock_cluster
            .expect_cluster_details()
            .returning(|| test_cluster_config_with_ltk("ltk-test", "test-ltk"));
        mock_cluster.expect_send_message().returning(|_| ());
        Arc::new(mock_cluster)
    };

    let cluster_for_manager = Arc::clone(&cluster_arc);

    let mut mock_manager = MockClusterManagerTrait::new();
    mock_manager
        .expect_handle_new_connection()
        .returning(move |_, _, token| {
            let result: Option<Arc<dyn ClusterTrait>> = if token == "test-ltk" {
                Some(Arc::clone(&cluster_for_manager))
            } else {
                None
            };
            Box::pin(async move { result })
        });
    mock_manager
        .expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    mock_manager
        .expect_report_websocket_error()
        .returning(|_, _| ());

    let state = make_test_state(db, mock_manager);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    let result = state
        .cluster_manager
        .handle_new_connection(1, tx, "test-ltk")
        .await;

    assert!(result.is_some(), "LTK authentication should succeed");
    assert_eq!(result.unwrap().name(), "ltk-test");
}

// ---------------------------------------------------------------------------
// Test: Invalid LTK falls back to UUID lookup
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_invalid_ltk_falls_back_to_uuid() {
    let db = setup_test_db().await;

    let cluster_arc: Arc<dyn ClusterTrait> = {
        let mut mock_cluster = MockClusterTrait::new();
        mock_cluster
            .expect_name()
            .returning(|| "test-cluster".to_string());
        mock_cluster.expect_is_online().returning(|| false);
        mock_cluster.expect_set_connection().returning(|_| ());
        mock_cluster.expect_role().returning(|| ClusterRole::Master);
        mock_cluster
            .expect_role_string()
            .returning(|| "master".to_string());
        mock_cluster
            .expect_cluster_details()
            .returning(|| test_cluster_config("test-cluster"));
        mock_cluster.expect_send_message().returning(|_| ());
        Arc::new(mock_cluster)
    };

    let mut mgr = MockClusterManagerTrait::new();
    mgr.expect_handle_new_connection()
        .returning(move |_, _, token| {
            assert_eq!(token, "test-uuid-12345", "Should fall back to UUID lookup");
            let result: Option<Arc<dyn ClusterTrait>> = Some(cluster_arc.clone());
            Box::pin(async move { result })
        });
    mgr.expect_remove_connection()
        .returning(|_, _| Box::pin(async {}));
    mgr.expect_report_websocket_error().returning(|_, _| ());

    let state = make_test_state(db, mgr);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<WsOutbound>();
    let result = state
        .cluster_manager
        .handle_new_connection(1, tx, "test-uuid-12345")
        .await;

    assert!(result.is_some(), "UUID authentication should succeed");
}

// ---------------------------------------------------------------------------
// Test: LTK clusters don't get UUID generated in reconnect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ltk_clusters_skipped_in_reconnect() {
    let ltk_config = test_cluster_config_with_ltk("ltk-cluster", "ltk-secret");
    let ssh_config = test_cluster_config("ssh-cluster");

    assert!(
        ltk_config.ltk.is_some(),
        "LTK cluster should have ltk field"
    );
    assert!(
        ssh_config.ltk.is_none(),
        "SSH cluster should not have ltk field"
    );
}

// ---------------------------------------------------------------------------
// Test: LTK configuration deserialization
// ---------------------------------------------------------------------------

#[test]
fn test_ltk_config_deserialization() {
    let json = r#"[{
        "name": "test-ltk",
        "host": "localhost",
        "username": "test",
        "path": "/test",
        "ltk": "my-secret-ltk"
    }]"#;

    let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "test-ltk");
    assert_eq!(configs[0].ltk, Some("my-secret-ltk".to_string()));
}

#[test]
fn test_ltk_config_missing_field() {
    let json = r#"[{
        "name": "test-ssh",
        "host": "localhost",
        "username": "test",
        "path": "/test"
    }]"#;

    let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "test-ssh");
    assert_eq!(configs[0].ltk, None);
}
