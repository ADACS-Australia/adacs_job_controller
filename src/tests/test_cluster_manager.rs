//! Integration tests for ClusterManager.
//!
//! Each test creates a fresh in-memory SQLite database and a real ClusterManager
//! instance to test connection lifecycle, token management, and cluster configuration.

mod common;

use std::sync::Arc;

use adacs_job_controller::cluster::manager::ClusterManager;
use adacs_job_controller::cluster::traits::ClusterManagerTrait;
use adacs_job_controller::config::clusters::ClusterConfig;
use adacs_job_controller::config::settings::*;

use adacs_job_controller::db::entities::cluster_uuid;
use dashmap::DashMap;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, Database, DatabaseConnection,
    DbBackend, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Schema,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn make_db() -> DatabaseConnection {
    Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection failed")
}

async fn setup_cluster_uuid_table(db: &DatabaseConnection) {
    let builder = DbBackend::Sqlite;
    let schema = Schema::new(builder);
    let stmt = builder.build(&schema.create_table_from_entity(cluster_uuid::Entity));
    db.execute(stmt).await.unwrap();
}

async fn count_uuids(db: &DatabaseConnection) -> u64 {
    cluster_uuid::Entity::find().count(db).await.unwrap()
}

async fn get_uuid_clusters(db: &DatabaseConnection) -> Vec<String> {
    cluster_uuid::Entity::find()
        .order_by_asc(cluster_uuid::Column::Cluster)
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.cluster)
        .collect()
}

fn three_cluster_configs() -> Vec<ClusterConfig> {
    vec![
        ClusterConfig {
            name: "cluster1".to_string(),
            host: "cluster1.com".to_string(),
            username: "user1".to_string(),
            path: "/cluster1/".to_string(),
            key: "cluster1_key".to_string(),
            connection_type: String::new(), // should default to "ssh" behavior
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        },
        ClusterConfig {
            name: "cluster2".to_string(),
            host: "cluster2.com".to_string(),
            username: "user2".to_string(),
            path: "/cluster2/".to_string(),
            key: "cluster2_key".to_string(),
            connection_type: String::new(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        },
        ClusterConfig {
            name: "cluster3".to_string(),
            host: "cluster3.com".to_string(),
            username: "user3".to_string(),
            path: "/cluster3/".to_string(),
            key: "cluster3_key".to_string(),
            connection_type: String::new(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        },
    ]
}

fn make_manager(configs: Vec<ClusterConfig>, db: DatabaseConnection) -> Arc<ClusterManager> {
    let file_list_map = Arc::new(DashMap::new());
    ClusterManager::new(configs, db, file_list_map)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verifies that ClusterManager with empty config creates no clusters.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with an empty config list.
///
/// # Act
/// Call `get_cluster_by_name` with an arbitrary cluster name.
///
/// # Assert
/// Returns `None` because no clusters were configured.
#[tokio::test]
async fn test_constructor_empty_config() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;

    let mgr = make_manager(vec![], db);
    // No clusters should exist
    assert!(mgr.get_cluster_by_name("anything").is_none());
}

/// Verifies that ClusterManager correctly initializes clusters from a provided config list.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with three cluster configs.
///
/// # Act
/// Call `get_cluster_by_name` for each configured cluster name and for a non-existent name.
///
/// # Assert
/// Each configured cluster is found with correct name, host, username, path, and key; the non-existent name returns `None`.
#[tokio::test]
async fn test_constructor_with_clusters() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;

    let configs = three_cluster_configs();
    let mgr = make_manager(configs, db);

    // Each cluster should be findable by name
    for i in 1..=3 {
        let name = format!("cluster{}", i);
        let cluster = mgr.get_cluster_by_name(&name);
        assert!(cluster.is_some(), "cluster {} should exist", name);
        let cluster = cluster.unwrap();
        assert_eq!(cluster.name(), name);
        assert_eq!(cluster.cluster_details().host, format!("cluster{}.com", i));
        assert_eq!(cluster.cluster_details().username, format!("user{}", i));
        assert_eq!(cluster.cluster_details().path, format!("/cluster{}/", i));
        assert_eq!(cluster.cluster_details().key, format!("cluster{}_key", i));
    }

    // Getting a non-existent cluster should return None
    assert!(mgr.get_cluster_by_name("not_a_real_cluster").is_none());
}

/// Verifies that looking up a cluster by an invalid connection ID returns `None`.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with three cluster configs.
///
/// # Act
/// Call `get_cluster_by_connection` with an invalid connection ID (999).
///
/// # Assert
/// Returns `None` because no connection with that ID exists.
#[tokio::test]
async fn test_get_cluster_by_connection() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db);

    // Getting a cluster by invalid connection should return None
    assert!(mgr.get_cluster_by_connection(999).is_none());
}

/// Verifies that all clusters are initially offline after ClusterManager construction.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with three cluster configs.
///
/// # Act
/// Call `is_cluster_online` on each configured cluster.
///
/// # Assert
/// All clusters report offline (`false`) since no connections have been established.
#[tokio::test]
async fn test_is_cluster_online_initially_offline() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db);

    for i in 1..=3 {
        let name = format!("cluster{}", i);
        let cluster = mgr.get_cluster_by_name(&name).unwrap();
        assert!(!mgr.is_cluster_online(cluster.as_ref()));
    }
}

/// Verifies that `reconnect_clusters` inserts one UUID token per offline cluster.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with three cluster configs. Confirm no UUIDs exist initially.
///
/// # Act
/// Call `reconnect_clusters`.
///
/// # Assert
/// Exactly three UUID rows are inserted in the database, one for each offline cluster.
#[tokio::test]
async fn test_reconnect_clusters_inserts_uuids() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Before reconnect, no UUIDs
    assert_eq!(count_uuids(&db).await, 0);

    // Reconnect should insert one UUID per offline cluster
    mgr.reconnect_clusters().await;
    assert_eq!(count_uuids(&db).await, 3);
}

/// Verifies that `reconnect_clusters` does not insert a UUID for an already-connected cluster.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, connect cluster2 via `handle_new_connection`, then clear all UUID rows.
///
/// # Act
/// Call `reconnect_clusters`.
///
/// # Assert
/// Only two new UUID rows are inserted (for the two offline clusters), and cluster2 is not in the UUID list.
#[tokio::test]
async fn test_reconnect_clusters_skips_online() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Insert a UUID for cluster2 and connect it
    mgr.reconnect_clusters().await;

    // Get cluster2's UUID from DB
    let model = cluster_uuid::Entity::find()
        .filter(cluster_uuid::Column::Cluster.eq("cluster2"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let uuid = model.uuid;

    // Connect cluster2 via handle_new_connection
    let conn_id = 100;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(conn_id, tx, &uuid).await;
    assert!(result.is_some());

    // Clear remaining UUIDs
    cluster_uuid::Entity::delete_many().exec(&db).await.unwrap();

    // Reconnect should only insert UUIDs for non-connected clusters
    mgr.reconnect_clusters().await;
    assert_eq!(count_uuids(&db).await, 2);

    // Verify cluster2 is NOT in the UUID list (it's online)
    let clusters = get_uuid_clusters(&db).await;
    assert!(!clusters.contains(&"cluster2".to_string()));
}

/// Verifies that `handle_new_connection` removes expired UUID tokens and preserves recent ones.
///
/// # Setup
/// Insert one expired UUID (older than `MAX_TOKEN_EXPIRY_SECONDS`) and later one recent UUID into the database.
///
/// # Act
/// Call `handle_new_connection` with a non-existent UUID to trigger cleanup logic.
///
/// # Assert
/// Expired UUIDs are deleted from the database, while a UUID younger than the expiry threshold is retained.
#[tokio::test]
async fn test_handle_new_connection_expire_uuids() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Insert an expired UUID (timestamp older than MAX_TOKEN_EXPIRY_SECONDS)
    let expiry = *CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS as i64;
    let expired_ts =
        chrono::Utc::now().naive_utc() - chrono::Duration::try_seconds(expiry).unwrap();
    cluster_uuid::ActiveModel {
        cluster: Set("cluster1".to_string()),
        uuid: Set("expired_uuid".to_string()),
        timestamp: Set(expired_ts),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    assert_eq!(count_uuids(&db).await, 1);

    // Try to connect with a non-existent UUID - should trigger cleanup of expired UUIDs
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(1, tx, "not_a_real_uuid").await;
    assert!(result.is_none());

    // The expired UUID should have been cleaned up
    assert_eq!(count_uuids(&db).await, 0);

    // Now insert a non-expired UUID and verify it ISN'T cleaned up
    let recent_ts =
        chrono::Utc::now().naive_utc() - chrono::Duration::try_seconds(expiry - 1).unwrap();
    cluster_uuid::ActiveModel {
        cluster: Set("cluster1".to_string()),
        uuid: Set("recent_uuid".to_string()),
        timestamp: Set(recent_ts),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(2, tx, "also_not_real").await;
    assert!(result.is_none());

    // The recent UUID should still be in the database
    assert_eq!(count_uuids(&db).await, 1);
}

/// Verifies that `handle_new_connection` connects a cluster when a valid UUID is provided and rejects unknown cluster names.
///
/// # Setup
/// Insert five UUIDs for a non-existent cluster, then five UUIDs for cluster2; use the last UUID in each batch for the connection attempt.
///
/// # Act
/// Call `handle_new_connection` twice: once with a UUID belonging to a non-existent cluster, and once with a UUID belonging to cluster2.
///
/// # Assert
/// The non-existent cluster returns `None` and its UUIDs are deleted; cluster2 returns `Some`, its UUIDs are deleted, it becomes online, and is findable by connection ID while other clusters remain offline.
#[tokio::test]
async fn test_handle_new_connection_valid_uuid() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Insert UUIDs for a fake cluster (should not match any real cluster)
    let mut last_uuid = String::new();
    for i in 0..5 {
        last_uuid = format!("fake-uuid-{}", i);
        cluster_uuid::ActiveModel {
            cluster: Set("not_real_cluster".to_string()),
            uuid: Set(last_uuid.clone()),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // All clusters should be offline
    for i in 1..=3 {
        let cluster = mgr.get_cluster_by_name(&format!("cluster{}", i)).unwrap();
        assert!(!mgr.is_cluster_online(cluster.as_ref()));
    }

    // Try connecting with the last UUID (for a non-existent cluster)
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(1, tx, &last_uuid).await;
    // UUID was found but cluster "not_real_cluster" doesn't exist -> None
    assert!(result.is_none());

    // All UUIDs for that cluster should be deleted
    assert_eq!(count_uuids(&db).await, 0);

    // No clusters should be connected
    for i in 1..=3 {
        let cluster = mgr.get_cluster_by_name(&format!("cluster{}", i)).unwrap();
        assert!(!mgr.is_cluster_online(cluster.as_ref()));
    }

    // Now insert UUIDs for a real cluster (cluster2)
    let mut last_uuid = String::new();
    for i in 0..5 {
        last_uuid = format!("real-uuid-{}", i);
        cluster_uuid::ActiveModel {
            cluster: Set("cluster2".to_string()),
            uuid: Set(last_uuid.clone()),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // Connect with the last UUID
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(10, tx, &last_uuid).await;
    assert!(
        result.is_some(),
        "handle_new_connection should return the cluster"
    );

    // All UUIDs should be deleted
    assert_eq!(count_uuids(&db).await, 0);

    // cluster2 should now be online
    let cluster2 = mgr.get_cluster_by_name("cluster2").unwrap();
    assert!(mgr.is_cluster_online(cluster2.as_ref()));

    // cluster2 should be findable by connection ID
    let found = mgr.get_cluster_by_connection(10);
    assert!(found.is_some());
    assert_eq!(found.unwrap().name(), "cluster2");

    // Other clusters should still be offline
    let cluster1 = mgr.get_cluster_by_name("cluster1").unwrap();
    assert!(!mgr.is_cluster_online(cluster1.as_ref()));
    let cluster3 = mgr.get_cluster_by_name("cluster3").unwrap();
    assert!(!mgr.is_cluster_online(cluster3.as_ref()));
}

/// Verifies that `handle_new_connection` rejects a second connection attempt for an already-connected cluster.
///
/// # Setup
/// Insert a UUID for cluster2, connect it successfully, then insert a second UUID for cluster2.
///
/// # Act
/// Call `handle_new_connection` a second time with the new UUID.
///
/// # Assert
/// The second call returns `None`, both UUIDs are deleted, and the original connection remains active while the new connection ID is not registered.
#[tokio::test]
async fn test_handle_new_connection_already_connected_rejected() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Insert UUID for cluster2 and connect it
    cluster_uuid::ActiveModel {
        cluster: Set("cluster2".to_string()),
        uuid: Set("uuid-first".to_string()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(1, tx, "uuid-first").await;
    assert!(result.is_some());

    // cluster2 is now online
    let cluster2 = mgr.get_cluster_by_name("cluster2").unwrap();
    assert!(mgr.is_cluster_online(cluster2.as_ref()));

    // Insert another UUID for the same cluster and try to connect again
    cluster_uuid::ActiveModel {
        cluster: Set("cluster2".to_string()),
        uuid: Set("uuid-second".to_string()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(2, tx, "uuid-second").await;
    // Should be rejected because cluster2 is already online
    assert!(result.is_none());

    // All UUIDs should still be deleted
    assert_eq!(count_uuids(&db).await, 0);

    // The original connection should still be active
    let found = mgr.get_cluster_by_connection(1);
    assert!(found.is_some());
    assert_eq!(found.unwrap().name(), "cluster2");

    // The new connection should NOT be in the map
    assert!(mgr.get_cluster_by_connection(2).is_none());
}

/// Verifies that `remove_connection` marks a cluster offline and removes it from the connection map.
///
/// # Setup
/// Connect all three clusters via UUID insertion and `handle_new_connection`, confirming each is online.
///
/// # Act
/// Call `remove_connection` for cluster2's connection ID, then call it with invalid IDs (999 and 0).
///
/// # Assert
/// Cluster2 goes offline and is not findable by connection; the other clusters remain online; invalid connection IDs do not panic.
#[tokio::test]
async fn test_remove_connection() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Connect all three clusters
    for i in 1..=3u64 {
        let name = format!("cluster{}", i);
        let uuid = format!("uuid-{}", i);
        cluster_uuid::ActiveModel {
            cluster: Set(name.clone()),
            uuid: Set(uuid.clone()),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = mgr.handle_new_connection(i, tx, &uuid).await;
        assert!(result.is_some());
    }

    // Verify all clusters are online
    for i in 1..=3 {
        let cluster = mgr.get_cluster_by_name(&format!("cluster{}", i)).unwrap();
        assert!(mgr.is_cluster_online(cluster.as_ref()));
    }

    // Remove cluster2's connection
    mgr.remove_connection(2, true).await;

    // cluster2 should be offline
    let cluster2 = mgr.get_cluster_by_name("cluster2").unwrap();
    assert!(!mgr.is_cluster_online(cluster2.as_ref()));

    // cluster2 should not be findable by connection
    assert!(mgr.get_cluster_by_connection(2).is_none());

    // Other clusters should still be online
    assert!(mgr.get_cluster_by_connection(1).is_some());
    assert!(mgr.get_cluster_by_connection(3).is_some());

    // Removing an invalid connection should not panic
    mgr.remove_connection(999, false).await;
    mgr.remove_connection(0, true).await;
}

/// Verifies that ClusterManager correctly parses SSH, Kerberos, and manual connection type configs.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with three configs covering `ssh`, `kerberos`, and `manual` connection types.
///
/// # Act
/// Call `get_cluster_by_name` for each cluster and inspect its `cluster_details`.
///
/// # Assert
/// Each cluster reports the correct `connection_type`, `key`, `keytab`, and `kerberos_principal` fields as specified in its config.
#[tokio::test]
async fn test_constructor_connection_types() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;

    let configs = vec![
        ClusterConfig {
            name: "ssh_cluster".to_string(),
            host: "ssh.example.com".to_string(),
            username: "sshuser".to_string(),
            path: "/ssh/path/".to_string(),
            key: "ssh_rsa_key".to_string(),
            connection_type: "ssh".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        },
        ClusterConfig {
            name: "kerberos_cluster".to_string(),
            host: "krb.example.com".to_string(),
            username: "krbuser".to_string(),
            path: "/krb/path/".to_string(),
            key: String::new(),
            connection_type: "kerberos".to_string(),
            keytab: "/etc/krb5.keytab".to_string(),
            kerberos_principal: "krbuser@EXAMPLE.COM".to_string(),
            ltk: None,
        },
        ClusterConfig {
            name: "manual_cluster".to_string(),
            host: "manual.example.com".to_string(),
            username: "manualuser".to_string(),
            path: "/manual/path/".to_string(),
            key: String::new(),
            connection_type: "manual".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        },
    ];

    let mgr = make_manager(configs, db);

    // SSH cluster
    let ssh = mgr.get_cluster_by_name("ssh_cluster").unwrap();
    assert_eq!(ssh.cluster_details().name, "ssh_cluster");
    assert_eq!(ssh.cluster_details().connection_type, "ssh");
    assert_eq!(ssh.cluster_details().key, "ssh_rsa_key");
    assert_eq!(ssh.cluster_details().keytab, "");
    assert_eq!(ssh.cluster_details().kerberos_principal, "");

    // Kerberos cluster
    let krb = mgr.get_cluster_by_name("kerberos_cluster").unwrap();
    assert_eq!(krb.cluster_details().name, "kerberos_cluster");
    assert_eq!(krb.cluster_details().connection_type, "kerberos");
    assert_eq!(krb.cluster_details().keytab, "/etc/krb5.keytab");
    assert_eq!(
        krb.cluster_details().kerberos_principal,
        "krbuser@EXAMPLE.COM"
    );
    assert_eq!(krb.cluster_details().key, "");

    // Manual cluster
    let manual = mgr.get_cluster_by_name("manual_cluster").unwrap();
    assert_eq!(manual.cluster_details().name, "manual_cluster");
    assert_eq!(manual.cluster_details().connection_type, "manual");
    assert_eq!(manual.cluster_details().host, "manual.example.com");
    assert_eq!(manual.cluster_details().username, "manualuser");
    assert_eq!(manual.cluster_details().path, "/manual/path/");
    assert_eq!(manual.cluster_details().key, "");
    assert_eq!(manual.cluster_details().keytab, "");
    assert_eq!(manual.cluster_details().kerberos_principal, "");
}

/// Verifies that repeated calls to `reconnect_clusters` do not accumulate stale UUID rows.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with three cluster configs.
///
/// # Act
/// Call `reconnect_clusters` three times in succession.
///
/// # Assert
/// After each call, exactly three UUID rows exist in the database, confirming old rows are replaced rather than accumulated.
#[tokio::test]
async fn test_reconnect_clusters_deletes_stale_uuids() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // First call inserts 3 UUIDs (one per cluster)
    mgr.reconnect_clusters().await;
    assert_eq!(count_uuids(&db).await, 3);

    // Second call should delete old UUIDs and insert new ones (still 3)
    mgr.reconnect_clusters().await;
    assert_eq!(count_uuids(&db).await, 3);

    // Third call to confirm the pattern is stable
    mgr.reconnect_clusters().await;
    assert_eq!(count_uuids(&db).await, 3);
}

/// Verifies that `handle_pong` records timestamps without panicking, even for connections that do not exist.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, then instantiate ClusterManager with three cluster configs.
///
/// # Act
/// Call `handle_pong` with connection IDs 1 and 2, then call it again with ID 1.
///
/// # Assert
/// No panic occurs; the method handles both new and duplicate pong entries gracefully.
#[tokio::test]
async fn test_handle_pong_records_timestamp() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // handle_pong should not panic even without a connection
    mgr.handle_pong(1);
    mgr.handle_pong(2);
    mgr.handle_pong(1); // update existing entry
}

/// Verifies that `create_file_download` creates a session that is retrievable by UUID.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, and obtain a reference to cluster1.
///
/// # Act
/// Call `create_file_download` with a UUID, then call `get_file_download` with the same UUID and a non-existent UUID.
///
/// # Assert
/// The created session is found and reports the correct cluster name; the non-existent UUID returns `None`.
#[tokio::test]
async fn test_create_file_download_session() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    let dl_cluster = mgr.create_file_download(&cluster, "dl-uuid-1").await;

    assert_eq!(dl_cluster.name(), "cluster1");

    // Should be findable via get_file_download
    let state = mgr.get_file_download("dl-uuid-1");
    assert!(state.is_some());

    // Non-existent UUID should return None
    assert!(mgr.get_file_download("nonexistent").is_none());
}

/// Verifies that `create_file_upload` creates a session that is retrievable by UUID.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, and obtain a reference to cluster1.
///
/// # Act
/// Call `create_file_upload` with a UUID, then call `get_file_upload` with the same UUID and a non-existent UUID.
///
/// # Assert
/// The created session is found and reports the correct cluster name; the non-existent UUID returns `None`.
#[tokio::test]
async fn test_create_file_upload_session() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    let ul_cluster = mgr.create_file_upload(&cluster, "ul-uuid-1").await;

    assert_eq!(ul_cluster.name(), "cluster1");

    // Should be findable via get_file_upload
    let state = mgr.get_file_upload("ul-uuid-1");
    assert!(state.is_some());

    // Non-existent UUID should return None
    assert!(mgr.get_file_upload("nonexistent").is_none());
}

/// Verifies that `handle_new_connection` accepts a file download session token and registers the connection.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, then create a file download session for cluster1 using a known token.
///
/// # Act
/// Call `handle_new_connection` with the file download token and connection ID 50.
///
/// # Assert
/// Returns `Some`, and the connection is findable via `get_cluster_by_connection(50)`.
#[tokio::test]
async fn test_handle_new_connection_file_download() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Create a file download session
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    let _dl_cluster = mgr.create_file_download(&cluster, "dl-token-1").await;

    // Connect using the file download token
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(50, tx, "dl-token-1").await;
    assert!(result.is_some());

    // Should be trackable by connection
    let found = mgr.get_cluster_by_connection(50);
    assert!(found.is_some());
}

/// Verifies that `handle_new_connection` accepts a file upload session token and registers the connection.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, then create a file upload session for cluster1 using a known token.
///
/// # Act
/// Call `handle_new_connection` with the file upload token and connection ID 60.
///
/// # Assert
/// Returns `Some`, and the connection is findable via `get_cluster_by_connection(60)`.
#[tokio::test]
async fn test_handle_new_connection_file_upload() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Create a file upload session
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    let _ul_cluster = mgr.create_file_upload(&cluster, "ul-token-1").await;

    // Connect using the file upload token
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(60, tx, "ul-token-1").await;
    assert!(result.is_some());

    // Should be trackable by connection
    let found = mgr.get_cluster_by_connection(60);
    assert!(found.is_some());
}

/// Verifies that `remove_connection` removes the connection and cleans up the associated file download session.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, create a file download session, and connect it with ID 70.
///
/// # Act
/// Call `remove_connection(70, true)`.
///
/// # Assert
/// The connection is no longer findable by ID 70, and the file download session for the token is removed.
#[tokio::test]
async fn test_remove_connection_file_download_cleanup() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Create and connect a file download session
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    let _dl_cluster = mgr.create_file_download(&cluster, "dl-cleanup").await;

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(70, tx, "dl-cleanup").await;
    assert!(result.is_some());

    // Remove the connection
    mgr.remove_connection(70, true).await;

    // Connection should be gone
    assert!(mgr.get_cluster_by_connection(70).is_none());

    // File download entry should also be cleaned up
    assert!(mgr.get_file_download("dl-cleanup").is_none());
}

/// Verifies that `remove_connection` removes the connection and cleans up the associated file upload session.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, create a file upload session, and connect it with ID 80.
///
/// # Act
/// Call `remove_connection(80, true)`.
///
/// # Assert
/// The connection is no longer findable by ID 80, and the file upload session for the token is removed.
#[tokio::test]
async fn test_remove_connection_file_upload_cleanup() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    // Create and connect a file upload session
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    let _ul_cluster = mgr.create_file_upload(&cluster, "ul-cleanup").await;

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr.handle_new_connection(80, tx, "ul-cleanup").await;
    assert!(result.is_some());

    // Remove the connection
    mgr.remove_connection(80, true).await;

    // Connection should be gone
    assert!(mgr.get_cluster_by_connection(80).is_none());

    // File upload entry should also be cleaned up
    assert!(mgr.get_file_upload("ul-cleanup").is_none());
}

// ---------------------------------------------------------------------------
// Ping/pong health monitoring tests
// ---------------------------------------------------------------------------

use adacs_job_controller::cluster::traits::WsOutbound;

/// Helper: connect a cluster via UUID insertion + handle_new_connection.
/// Returns (conn_id, rx) where rx is the WS channel receiver.
async fn connect_cluster(
    mgr: &Arc<ClusterManager>,
    db: &DatabaseConnection,
    cluster_name: &str,
    conn_id: u64,
) -> tokio::sync::mpsc::UnboundedReceiver<WsOutbound> {
    // Insert a UUID for the cluster
    cluster_uuid::ActiveModel {
        cluster: Set(cluster_name.to_string()),
        uuid: Set(format!("ping-uuid-{}", conn_id)),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let result = mgr
        .handle_new_connection(conn_id, tx, &format!("ping-uuid-{}", conn_id))
        .await;
    assert!(result.is_some(), "cluster {} should connect", cluster_name);
    rx
}

/// Verifies that pong times are initialized during connection, preventing the first `check_pings` from evicting the cluster.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, and connect cluster1 via `connect_cluster`.
///
/// # Act
/// Call `check_pings` once immediately after connection.
///
/// # Assert
/// Cluster1 remains online because `handle_new_connection` initializes the pong timestamp, protecting it from the first ping check.
#[tokio::test]
async fn test_pong_times_initialized_after_connection() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    let _rx = connect_cluster(&mgr, &db, "cluster1", 1).await;

    // Cluster should be online
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    assert!(cluster.is_online());

    // First check_pings: pong_times exists (from connect) so no eviction.
    // It will send a fresh ping and clear pong_times for next round.
    mgr.check_pings().await;

    // Cluster should still be online — the pong_times entry protected it.
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    assert!(
        cluster.is_online(),
        "cluster should survive the first check_pings after connection"
    );
}

/// Verifies that a cluster remains online when a pong is received between two consecutive `check_pings` calls.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, and connect cluster1 via `connect_cluster`.
///
/// # Act
/// Call `check_pings`, verify a `Ping` frame is sent, call `handle_pong`, then call `check_pings` again.
///
/// # Assert
/// Cluster1 remains online and a second `Ping` frame is sent through the channel, confirming the keep-alive cycle is maintained.
#[tokio::test]
async fn test_check_pings_send_ping_success() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    let mut rx = connect_cluster(&mgr, &db, "cluster1", 1).await;

    // First check_pings: sends a Ping frame
    mgr.check_pings().await;

    // Verify a Ping was sent through the channel
    let outbound = rx.try_recv().expect("should have received a ping");
    assert!(
        matches!(outbound, WsOutbound::Ping),
        "expected WsOutbound::Ping, got {:?}",
        outbound
    );

    // Simulate the cluster responding with a Pong
    mgr.handle_pong(1);

    // Second check_pings: pong was received, so no eviction
    mgr.check_pings().await;

    // Cluster should still be online
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    assert!(
        cluster.is_online(),
        "cluster should survive when pong is received between pings"
    );

    // Another Ping should have been sent
    let outbound = rx.try_recv().expect("should have received second ping");
    assert!(matches!(outbound, WsOutbound::Ping));
}

/// Verifies that a cluster is evicted when no pong is received after a ping is sent.
///
/// # Setup
/// Create an in-memory SQLite database and UUID table, instantiate ClusterManager with three configs, and connect cluster1 via `connect_cluster`.
///
/// # Act
/// Call `check_pings` twice without calling `handle_pong` in between.
///
/// # Assert
/// Cluster1 is marked offline and removed from the connection map after the second `check_pings` call finds no pong response.
#[tokio::test]
async fn test_check_pings_evicts_dead_connection() {
    let db = make_db().await;
    setup_cluster_uuid_table(&db).await;
    let mgr = make_manager(three_cluster_configs(), db.clone());

    let _rx = connect_cluster(&mgr, &db, "cluster1", 1).await;

    // Cluster starts online
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    assert!(cluster.is_online());

    // First check_pings: sends a ping, clears pong_times
    mgr.check_pings().await;

    // Do NOT call handle_pong — simulate a dead/unresponsive cluster

    // Second check_pings: ping_times has entry, pong_times does NOT → evict
    mgr.check_pings().await;

    // Cluster should now be offline
    let cluster = mgr.get_cluster_by_name("cluster1").unwrap();
    assert!(
        !cluster.is_online(),
        "cluster should be evicted when pong is missing after ping"
    );

    // Connection map should be empty
    assert!(mgr.get_cluster_by_connection(1).is_none());
}
