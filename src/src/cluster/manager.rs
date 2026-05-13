use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::cluster::cluster::{AppContext, Cluster};
use crate::cluster::file_download::FileDownloadState;
use crate::cluster::file_upload::FileUploadState;
use crate::cluster::traits::{ClusterManagerTrait, ClusterTrait, ConnectionId, WsConnectionSender};
use crate::config::clusters::ClusterConfig;
use crate::config::settings::{
    CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS, CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS,
    CLUSTER_MANAGER_PING_INTERVAL_SECONDS,
};
use crate::protocol::types::ClusterRole;

/// `ClusterManager` manages the lifecycle of all cluster connections.
///
/// It handles:
/// - Cluster registration from clusters.json
/// - WebSocket connection authentication via DB-stored UUID tokens
/// - Reconnection (SSH/Kerberos) for offline clusters
/// - Ping/pong health monitoring
/// - File download/upload session creation and tracking
pub struct ClusterManager {
    /// Master cluster instances by name
    clusters: RwLock<HashMap<String, Arc<Cluster>>>,

    /// WebSocket connection ID → cluster mapping
    connection_map: DashMap<ConnectionId, Arc<Cluster>>,

    /// File download sessions: UUID → (`download_state`, cluster)
    file_download_map: DashMap<String, (Arc<FileDownloadState>, Arc<Cluster>)>,

    /// File upload sessions: UUID → (`upload_state`, cluster)
    file_upload_map: DashMap<String, (Arc<FileUploadState>, Arc<Cluster>)>,

    /// Database connection for token lookups, UUID generation, etc.
    db: sea_orm::DatabaseConnection,

    /// Application context shared with clusters
    app_context: Arc<AppContext>,

    /// Whether the manager is running
    running: AtomicBool,

    /// Pong timestamps for latency tracking (`connection_id` → `last_pong_time`)
    pong_times: DashMap<ConnectionId, std::time::Instant>,

    /// Ping timestamps for dead connection detection (`connection_id` → `last_ping_sent_time`)
    ping_times: DashMap<ConnectionId, std::time::Instant>,

    /// Consecutive missed pongs per connection (threshold = 2 before eviction)
    missed_pongs: DashMap<ConnectionId, u32>,

    /// Pause/resume locks per cluster name (shared with file download clusters)
    pause_resume_locks: DashMap<String, Arc<tokio::sync::Mutex<()>>>,

    /// Reconnect attempt counters per cluster for exponential backoff
    reconnect_attempts: DashMap<String, u32>,
    /// Timestamp of last reconnect attempt per cluster
    last_reconnect_attempt: DashMap<String, std::time::Instant>,
}

impl ClusterManager {
    /// Create a new `ClusterManager` and register clusters from config.
    #[must_use]
    pub fn new(
        cluster_configs: Vec<ClusterConfig>,
        db: sea_orm::DatabaseConnection,
        file_list_map: Arc<
            DashMap<String, Arc<tokio::sync::Mutex<crate::protocol::types::FileListState>>>,
        >,
    ) -> Arc<Self> {
        let app_context = Arc::new(AppContext {
            db: db.clone(),
            file_list_map,
        });

        let mut clusters = HashMap::new();
        let pause_resume_locks = DashMap::new();
        for config in cluster_configs {
            let name = config.name.clone();
            let cluster = Cluster::new(config, Some(Arc::clone(&app_context)));
            pause_resume_locks.insert(name.clone(), Arc::new(tokio::sync::Mutex::new(())));
            clusters.insert(name, cluster);
        }

        Arc::new(Self {
            clusters: RwLock::new(clusters),
            connection_map: DashMap::new(),
            file_download_map: DashMap::new(),
            file_upload_map: DashMap::new(),
            db,
            app_context,
            running: AtomicBool::new(true),
            pong_times: DashMap::new(),
            ping_times: DashMap::new(),
            missed_pongs: DashMap::new(),
            pause_resume_locks,
            reconnect_attempts: DashMap::new(),
            last_reconnect_attempt: DashMap::new(),
        })
    }

    /// Start background tasks (reconnection, ping).
    ///
    /// # Panics
    ///
    /// Panics if `RwLock` on clusters cannot be acquired for reading.
    pub fn start_tasks(self: &Arc<Self>) {
        // Start scheduler tasks for each cluster
        {
            match self.clusters.try_read() {
                Ok(clusters) => {
                    for cluster in clusters.values() {
                        cluster.start_tasks();
                    }
                }
                Err(e) => {
                    eprintln!("WARNING: Failed to acquire read lock on clusters: {e}");
                }
            }
        }

        // Reconnection task
        let this = Arc::clone(self);
        tokio::spawn(async move { this.run_reconnect().await });

        // Ping task
        let this = Arc::clone(self);
        tokio::spawn(async move { this.run_ping().await });
    }

    /// Background task: periodically reconnect offline clusters.
    async fn run_reconnect(self: Arc<Self>) {
        while self.running.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_secs(
                *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS,
            ))
            .await;
            if !self.running.load(Ordering::Relaxed) {
                break;
            }
            self.reconnect_clusters().await;
        }
    }

    /// Try to reconnect all offline master clusters.
    pub async fn reconnect_clusters(&self) {
        use crate::db::entities::cluster_uuid;
        use sea_orm::{
            ActiveModelTrait,
            ActiveValue::{NotSet, Set},
            ColumnTrait, EntityTrait, QueryFilter,
        };

        let clusters = self.clusters.read().await;
        for (name, cluster) in clusters.iter() {
            if cluster.is_online() {
                continue;
            }

            let details = cluster.cluster_details();

            // Skip LTK clusters - they connect autonomously
            if details.ltk.is_some() {
                tracing::info!(
                    "Skipping LTK cluster {} - waits for autonomous connection",
                    name
                );
                continue;
            }

            // Exponential backoff: wait base_interval * 2^(attempt-1) before retrying
            // First attempt (attempt == 0) is always allowed; retries back off exponentially
            let attempt = self.reconnect_attempts.get(name).map_or(0, |r| *r);
            if attempt > 0 {
                let backoff_secs =
                    *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS * 2u64.saturating_pow(attempt - 1);
                if let Some(last_attempt) = self.last_reconnect_attempt.get(name) {
                    let elapsed = last_attempt.elapsed().as_secs();
                    if elapsed < backoff_secs {
                        tracing::info!(
                            "Skipping reconnect for {} (attempt {}, backoff {}s, {}s elapsed)",
                            name,
                            attempt,
                            backoff_secs,
                            elapsed
                        );
                        continue;
                    }
                }
            }

            self.reconnect_attempts.insert(name.clone(), attempt + 1);
            self.last_reconnect_attempt
                .insert(name.clone(), std::time::Instant::now());
            tracing::info!("Reconnecting cluster {} (attempt {})", name, attempt);

            let uuid = uuid::Uuid::new_v4().to_string();

            // Delete any existing UUIDs for this cluster before inserting a new one
            let _ = cluster_uuid::Entity::delete_many()
                .filter(cluster_uuid::Column::Cluster.eq(name.as_str()))
                .exec(&self.db)
                .await;
            let record = cluster_uuid::ActiveModel {
                id: NotSet,
                cluster: Set(name.clone()),
                uuid: Set(uuid.clone()),
                timestamp: Set(chrono::Utc::now().naive_utc()),
            };
            if let Err(e) = record.insert(&self.db).await {
                tracing::warn!("Failed to insert cluster UUID for {}: {}", name, e);
                continue;
            }

            match details.connection_type.as_str() {
                "manual" => {
                    tracing::info!(
                        "Cluster {} requires manual connection. Token: {}",
                        name,
                        uuid
                    );
                }
                _ => {
                    // SSH or Kerberos: launch Python keyserver
                    self.launch_ssh_connection(&details, &uuid);
                }
            }
        }
    }

    /// Launch SSH connection via Python keyserver.
    #[allow(clippy::unused_self)]
    fn launch_ssh_connection(&self, details: &ClusterConfig, token: &str) {
        let python_path = std::env::var("KEYCLIENT_PYTHON")
            .unwrap_or_else(|_| "./utils/keyserver/venv/bin/python".to_string());
        let script_path = std::env::var("KEYCLIENT_SCRIPT")
            .unwrap_or_else(|_| "./utils/keyserver/keyserver.py".to_string());
        self.launch_ssh_connection_with_paths(details, token, &python_path, &script_path);
    }

    #[allow(clippy::unused_self)]
    fn launch_ssh_connection_with_paths(
        &self,
        details: &ClusterConfig,
        token: &str,
        python_path: &str,
        script_path: &str,
    ) {
        let mut cmd = tokio::process::Command::new(&python_path);
        cmd.arg(&script_path);
        cmd.stdin(std::process::Stdio::piped());
        cmd.env("SSH_HOST", &details.host);
        cmd.env("SSH_USERNAME", &details.username);
        cmd.env("SSH_KEY", &details.key);
        cmd.env("SSH_PATH", &details.path);

        if !details.keytab.is_empty() {
            cmd.env("SSH_KEYTAB", &details.keytab);
        }
        if !details.kerberos_principal.is_empty() {
            cmd.env("SSH_PRINCIPAL", &details.kerberos_principal);
        }

        let token_owned = token.to_string();
        match cmd.spawn() {
            Ok(mut child) => {
                tokio::spawn(async move {
                    // Write token to stdin (avoids /proc/<pid>/environ exposure)
                    if let Some(stdin) = child.stdin.as_mut() {
                        use tokio::io::AsyncWriteExt;
                        let _ = stdin.write_all(token_owned.as_bytes()).await;
                        let _ = stdin.shutdown().await;
                    }
                    match child.wait().await {
                        Ok(status) => {
                            if !status.success() {
                                tracing::warn!("SSH keyserver exited with status: {}", status);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to wait for SSH keyserver: {}", e);
                        }
                    }
                });
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to launch SSH keyserver (python={python_path}, script={script_path}): {e}"
                );
            }
        }
    }

    /// Background task: periodically ping connected clusters.
    async fn run_ping(self: Arc<Self>) {
        while self.running.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_secs(
                *CLUSTER_MANAGER_PING_INTERVAL_SECONDS,
            ))
            .await;
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            self.check_pings().await;
        }
    }

    /// Check for dead connections and send fresh pings.
    ///
    /// A connection is considered dead if a ping was previously sent
    /// (`ping_times` has an entry) but no pong was received since
    /// (`pong_times` entry was cleared when the ping was sent and
    /// hasn't been re-inserted by `handle_pong`).
    ///
    /// After evicting dead connections, a fresh ping is sent to all
    /// remaining master–role connections.
    pub async fn check_pings(&self) {
        // 1. Find connections that missed a pong: ping was sent but no pong received
        let no_pong: Vec<ConnectionId> = self
            .ping_times
            .iter()
            .filter(|entry| !self.pong_times.contains_key(entry.key()))
            .map(|entry| *entry.key())
            .collect();

        // 1b. Increment missed counter; evict only after 2 consecutive misses
        let dead_conn_ids: Vec<ConnectionId> = no_pong
            .into_iter()
            .filter(|conn_id| {
                let mut missed = self.missed_pongs.entry(*conn_id).or_insert(0);
                *missed += 1;
                *missed >= 2
            })
            .collect();

        // 2. Evict dead connections
        for conn_id in dead_conn_ids {
            let cluster_name = self
                .connection_map
                .get(&conn_id)
                .map_or_else(|| "unknown".to_string(), |c| c.name());
            tracing::warn!(
                "WS: Cluster {} timed out waiting for pong (conn_id={}). Disconnecting.",
                cluster_name,
                conn_id
            );
            self.ping_times.remove(&conn_id);
            self.remove_connection(conn_id, true).await;
        }

        // 3. Send fresh ping to all online master connections
        for entry in &self.connection_map {
            let conn_id = *entry.key();
            let cluster = entry.value();

            // Only ping master connections (not file download/upload)
            if cluster.role() != ClusterRole::Master || !cluster.is_online() {
                continue;
            }

            // Send WS ping frame first, then update tracking maps
            cluster.send_ping();
            self.ping_times.insert(conn_id, std::time::Instant::now());
            self.pong_times.remove(&conn_id);
        }
    }
}

#[async_trait]
impl ClusterManagerTrait for ClusterManager {
    fn get_cluster_by_name(&self, name: &str) -> Option<Arc<dyn ClusterTrait>> {
        let clusters = self.clusters.try_read().ok()?;
        clusters
            .get(name)
            .map(|c| Arc::clone(c) as Arc<dyn ClusterTrait>)
    }

    fn get_cluster_by_connection(&self, conn_id: ConnectionId) -> Option<Arc<dyn ClusterTrait>> {
        self.connection_map
            .get(&conn_id)
            .map(|c| Arc::clone(c.value()) as Arc<dyn ClusterTrait>)
    }

    async fn handle_new_connection(
        &self,
        conn_id: ConnectionId,
        ws_sender: WsConnectionSender,
        token: &str,
    ) -> Option<Arc<dyn ClusterTrait>> {
        use crate::db::entities::cluster_uuid;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

        // Check file download map first
        if let Some(entry) = self.file_download_map.get(token) {
            let (_, cluster) = entry.value();
            let cluster = Arc::clone(cluster);
            cluster.set_connection(Some(ws_sender)).await;
            self.connection_map.insert(conn_id, cluster.clone());
            return Some(cluster as Arc<dyn ClusterTrait>);
        }

        // Check file upload map
        if let Some(entry) = self.file_upload_map.get(token) {
            let (_, cluster) = entry.value();
            let cluster = Arc::clone(cluster);
            cluster.set_connection(Some(ws_sender)).await;
            self.connection_map.insert(conn_id, cluster.clone());
            return Some(cluster as Arc<dyn ClusterTrait>);
        }

        // Check LTK authentication first (before UUID DB lookup)
        let clusters = self.clusters.read().await;
        for (cluster_name, cluster) in clusters.iter() {
            if let Some(configured_ltk) = &cluster.cluster_details().ltk
                && configured_ltk == token
            {
                // LTK match found - check for duplicate connection (security)
                if cluster.is_online() {
                    tracing::warn!(
                        "Security: Duplicate LTK connection attempt for cluster {} (conn_id={})",
                        cluster_name,
                        conn_id
                    );
                    return None;
                }

                // Apply rate limiting timeout
                let timeout = *crate::config::settings::LTK_CONNECTION_TIMEOUT_MS;
                if timeout > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(u64::from(timeout))).await;
                }

                // Authenticate LTK cluster
                let cluster = Arc::clone(cluster);
                cluster.set_connection(Some(ws_sender)).await;
                self.connection_map.insert(conn_id, cluster.clone());
                self.pong_times.insert(conn_id, std::time::Instant::now());
                self.reconnect_attempts.remove(cluster_name);
                self.last_reconnect_attempt.remove(cluster_name);
                tracing::info!(
                    "LTK cluster {} connected (conn_id={})",
                    cluster_name,
                    conn_id
                );
                return Some(cluster as Arc<dyn ClusterTrait>);
            }
        }

        // No LTK match - fall back to UUID DB lookup (existing logic)

        // First clean up any expired UUIDs
        let cutoff = chrono::Utc::now().naive_utc()
            - chrono::Duration::try_seconds(*CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS as i64)
                .unwrap_or_default();
        let _ = cluster_uuid::Entity::delete_many()
            .filter(cluster_uuid::Column::Timestamp.lte(cutoff))
            .exec(&self.db)
            .await;

        // Now look up the token (only non-expired ones remain)
        let row = cluster_uuid::Entity::find()
            .filter(cluster_uuid::Column::Uuid.eq(token))
            .one(&self.db)
            .await
            .ok()
            .flatten();

        if let Some(r) = row {
            let cluster_name = r.cluster.clone();

            // Delete ALL UUID records for this cluster
            let _ = cluster_uuid::Entity::delete_many()
                .filter(cluster_uuid::Column::Cluster.eq(cluster_name.as_str()))
                .exec(&self.db)
                .await;

            if let Some(cluster) = clusters.get(&cluster_name) {
                // If this cluster is already connected, reject the new connection
                if cluster.is_online() {
                    return None;
                }

                let cluster = Arc::clone(cluster);
                cluster.set_connection(Some(ws_sender)).await;
                self.connection_map.insert(conn_id, cluster.clone());
                self.pong_times.insert(conn_id, std::time::Instant::now());
                self.reconnect_attempts.remove(&cluster_name);
                self.last_reconnect_attempt.remove(&cluster_name);
                tracing::info!("Cluster {} connected (conn_id={})", cluster_name, conn_id);
                return Some(cluster as Arc<dyn ClusterTrait>);
            }
        }

        tracing::warn!("Invalid token for connection {}", conn_id);
        None
    }

    async fn remove_connection(&self, conn_id: ConnectionId, close: bool) {
        if let Some((_, cluster)) = self.connection_map.remove(&conn_id) {
            if close {
                cluster.close(false).await;
            }
            cluster.set_connection(None).await;
            self.pong_times.remove(&conn_id);
            self.ping_times.remove(&conn_id);
            self.missed_pongs.remove(&conn_id);

            let role = cluster.role();
            let name = cluster.name();
            tracing::info!("Connection removed for {} (role={:?})", name, role);

            // Clean up file download/upload entries if applicable
            if role == ClusterRole::FileDownload {
                if let Some(uuid) = cluster.uuid() {
                    self.file_download_map.remove(uuid);
                }
            } else if role == ClusterRole::FileUpload
                && let Some(uuid) = cluster.uuid()
            {
                self.file_upload_map.remove(uuid);
            }
        }
    }

    fn handle_pong(&self, conn_id: ConnectionId) {
        let now = std::time::Instant::now();
        self.pong_times.insert(conn_id, now);
        self.missed_pongs.insert(conn_id, 0);

        // Report latency
        if let Some(ping_time) = self.ping_times.get(&conn_id) {
            let latency = now.duration_since(*ping_time);
            let cluster_name = self
                .connection_map
                .get(&conn_id)
                .map_or_else(|| "unknown".to_string(), |c| c.name());
            tracing::info!(
                "WS: Cluster {} had {}ms latency.",
                cluster_name,
                latency.as_millis()
            );
        }
    }

    async fn create_file_download(
        &self,
        cluster: &Arc<dyn ClusterTrait>,
        uuid: &str,
    ) -> Arc<dyn ClusterTrait> {
        let details = cluster.cluster_details();
        let download_state = Arc::new(FileDownloadState::new());

        // Get or create the pause/resume lock for this cluster
        let lock = self
            .pause_resume_locks
            .entry(details.name.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();

        let dl_cluster = Cluster::new_file_download(
            details,
            uuid.to_string(),
            Arc::clone(&download_state),
            Some(Arc::clone(&self.app_context)),
            lock,
        );
        dl_cluster.start_tasks();

        self.file_download_map
            .insert(uuid.to_string(), (download_state, Arc::clone(&dl_cluster)));

        dl_cluster as Arc<dyn ClusterTrait>
    }

    async fn create_file_upload(
        &self,
        cluster: &Arc<dyn ClusterTrait>,
        uuid: &str,
    ) -> Arc<dyn ClusterTrait> {
        let details = cluster.cluster_details();
        let upload_state = Arc::new(FileUploadState::new());
        let ul_cluster = Cluster::new_file_upload(
            details,
            uuid.to_string(),
            Arc::clone(&upload_state),
            Some(Arc::clone(&self.app_context)),
        );
        ul_cluster.start_tasks();

        self.file_upload_map
            .insert(uuid.to_string(), (upload_state, Arc::clone(&ul_cluster)));

        ul_cluster as Arc<dyn ClusterTrait>
    }

    fn is_cluster_online(&self, cluster: &dyn ClusterTrait) -> bool {
        cluster.is_online()
    }

    fn report_websocket_error(&self, cluster_name: Option<String>, error: String) {
        if let Some(name) = cluster_name {
            tracing::error!("WebSocket error for cluster {}: {}", name, error);
        } else {
            tracing::error!("WebSocket error (unknown cluster): {}", error);
        }
    }

    fn get_file_download(&self, uuid: &str) -> Option<Arc<FileDownloadState>> {
        self.file_download_map
            .get(uuid)
            .map(|entry| Arc::clone(&entry.value().0))
    }

    fn get_file_upload(&self, uuid: &str) -> Option<Arc<FileUploadState>> {
        self.file_upload_map
            .get(uuid)
            .map(|entry| Arc::clone(&entry.value().0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::traits::MockClusterManagerTrait;
    use crate::cluster::traits::MockClusterTrait;
    use crate::db::entities::cluster_uuid;
    use sea_orm::{
        ColumnTrait, ConnectionTrait, Database, DbBackend, EntityTrait, QueryFilter, Schema,
    };

    fn test_configs() -> Vec<ClusterConfig> {
        vec![
            ClusterConfig {
                name: "cluster_a".to_string(),
                host: "host-a.example.com".to_string(),
                username: "user_a".to_string(),
                path: "/path/a".to_string(),
                key: "key_a".to_string(),
                connection_type: "ssh".to_string(),
                keytab: String::new(),
                kerberos_principal: String::new(),
                ltk: None,
            },
            ClusterConfig {
                name: "cluster_b".to_string(),
                host: "host-b.example.com".to_string(),
                username: "user_b".to_string(),
                path: "/path/b".to_string(),
                key: String::new(),
                connection_type: "manual".to_string(),
                keytab: String::new(),
                kerberos_principal: String::new(),
                ltk: None,
            },
        ]
    }

    // Test basic creation (without DB pool — requires mock or real pool)
    // For unit tests we'll test the non-DB parts using the mock trait

    #[test]
    fn test_cluster_config_struct() {
        let configs = test_configs();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "cluster_a");
        assert_eq!(configs[0].connection_type, "ssh");
        assert_eq!(configs[1].name, "cluster_b");
        assert_eq!(configs[1].connection_type, "manual");
    }

    async fn make_db() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("sqlite in-memory connection failed");
        let builder = DbBackend::Sqlite;
        let schema = Schema::new(builder);
        let stmt = builder.build(&schema.create_table_from_entity(cluster_uuid::Entity));
        db.execute(stmt).await.unwrap();
        db
    }

    async fn get_uuid_for_cluster(
        db: &sea_orm::DatabaseConnection,
        cluster: &str,
    ) -> Option<String> {
        cluster_uuid::Entity::find()
            .filter(cluster_uuid::Column::Cluster.eq(cluster))
            .one(db)
            .await
            .unwrap()
            .map(|model| model.uuid)
    }

    #[tokio::test]
    async fn test_reconnect_backoff_doubles_between_attempts() {
        let db = make_db().await;
        let manager = ClusterManager::new(test_configs(), db.clone(), Arc::new(DashMap::new()));

        manager.reconnect_clusters().await;
        let first_uuid = get_uuid_for_cluster(&db, "cluster_b").await.unwrap();
        assert_eq!(
            manager.reconnect_attempts.get("cluster_b").map(|v| *v),
            Some(1)
        );

        let first_attempt_time = *manager.last_reconnect_attempt.get("cluster_b").unwrap();
        manager.reconnect_clusters().await;
        let second_uuid = get_uuid_for_cluster(&db, "cluster_b").await.unwrap();
        assert_eq!(
            second_uuid, first_uuid,
            "backoff should skip immediate retry"
        );
        assert_eq!(
            manager.reconnect_attempts.get("cluster_b").map(|v| *v),
            Some(1)
        );
        assert_eq!(
            *manager.last_reconnect_attempt.get("cluster_b").unwrap(),
            first_attempt_time
        );

        manager.last_reconnect_attempt.insert(
            "cluster_b".to_string(),
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(
                    *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS,
                ))
                .unwrap(),
        );
        manager.reconnect_clusters().await;
        let third_uuid = get_uuid_for_cluster(&db, "cluster_b").await.unwrap();
        assert_ne!(
            third_uuid, second_uuid,
            "retry should proceed after first backoff window"
        );
        assert_eq!(
            manager.reconnect_attempts.get("cluster_b").map(|v| *v),
            Some(2)
        );

        manager.last_reconnect_attempt.insert(
            "cluster_b".to_string(),
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(
                    *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS,
                ))
                .unwrap(),
        );
        manager.reconnect_clusters().await;
        let fourth_uuid = get_uuid_for_cluster(&db, "cluster_b").await.unwrap();
        assert_eq!(
            fourth_uuid, third_uuid,
            "second retry should require doubled backoff"
        );
        assert_eq!(
            manager.reconnect_attempts.get("cluster_b").map(|v| *v),
            Some(2)
        );

        manager.last_reconnect_attempt.insert(
            "cluster_b".to_string(),
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(
                    *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS * 2,
                ))
                .unwrap(),
        );
        manager.reconnect_clusters().await;
        let fifth_uuid = get_uuid_for_cluster(&db, "cluster_b").await.unwrap();
        assert_ne!(
            fifth_uuid, fourth_uuid,
            "retry should proceed after doubled backoff window"
        );
        assert_eq!(
            manager.reconnect_attempts.get("cluster_b").map(|v| *v),
            Some(3)
        );
    }

    #[tokio::test]
    async fn test_launch_ssh_connection_passes_token_via_stdin_not_env() {
        let db = make_db().await;
        let manager = ClusterManager::new(vec![], db, Arc::new(DashMap::new()));

        let temp_dir =
            std::env::temp_dir().join(format!("adacs-keyclient-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let script_path = temp_dir.join("capture.sh");
        let env_out = temp_dir.join("env.txt");
        let stdin_out = temp_dir.join("stdin.txt");

        std::fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nprintf '%s' \"${{SSH_TOKEN:-}}\" > \"{}\"\ncat > \"{}\"\n",
                env_out.display(),
                stdin_out.display()
            ),
        )
        .unwrap();

        let details = ClusterConfig {
            name: "ssh-test".to_string(),
            host: "example.com".to_string(),
            username: "user".to_string(),
            path: "/remote/path".to_string(),
            key: "private-key".to_string(),
            connection_type: "ssh".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        };

        let token = "super-secret-token";
        manager.launch_ssh_connection_with_paths(&details, token, "/bin/sh", &script_path.to_string_lossy());

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while (!env_out.exists() || !stdin_out.exists()) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }

        let exposed_env = std::fs::read_to_string(&env_out).unwrap();
        let stdin_token = std::fs::read_to_string(&stdin_out).unwrap();

        let _ = std::fs::remove_dir_all(&temp_dir);

        assert!(
            exposed_env.is_empty(),
            "SSH_TOKEN should not be set in child environment"
        );
        assert_eq!(stdin_token, token, "token should be delivered via stdin");
    }

    // Integration tests would require a real or mock DB pool
    // The ClusterManagerTrait mock (from mockall) allows testing
    // consumers of ClusterManager without a real instance
    #[test]
    fn test_mock_cluster_manager_get_cluster() {
        let mut mock = MockClusterManagerTrait::new();
        mock.expect_get_cluster_by_name()
            .with(mockall::predicate::eq("test"))
            .returning(|_| None);

        assert!(mock.get_cluster_by_name("test").is_none());
    }

    #[test]
    fn test_mock_cluster_manager_is_online() {
        let mut mock = MockClusterManagerTrait::new();
        mock.expect_is_cluster_online().returning(|_| false);

        let cluster_mock = MockClusterTrait::new();
        assert!(!mock.is_cluster_online(&cluster_mock));
    }

    #[test]
    fn test_mock_cluster_manager_report_error() {
        let mut mock = MockClusterManagerTrait::new();
        mock.expect_report_websocket_error().returning(|_, _| ());

        mock.report_websocket_error(Some("cluster_a".into()), "test error".into());
    }

    #[test]
    fn test_mock_cluster_manager_file_download() {
        let mut mock = MockClusterManagerTrait::new();
        mock.expect_get_file_download().returning(|_| None);

        assert!(mock.get_file_download("nonexistent-uuid").is_none());
    }

    #[test]
    fn test_mock_cluster_manager_file_upload() {
        let mut mock = MockClusterManagerTrait::new();
        mock.expect_get_file_upload().returning(|_| None);

        assert!(mock.get_file_upload("nonexistent-uuid").is_none());
    }

    #[tokio::test]
    async fn test_mock_cluster_manager_remove_connection() {
        let mut mock = MockClusterManagerTrait::new();
        mock.expect_remove_connection()
            .returning(|_, _| Box::pin(async {}));

        // Should not panic when removing a connection
        mock.remove_connection(42, true).await;
        mock.remove_connection(42, false).await;
    }

    #[test]
    fn test_mock_cluster_manager_handle_pong() {
        let mut mock = MockClusterManagerTrait::new();
        mock.expect_handle_pong().returning(|_| ());

        // Should not panic
        mock.handle_pong(1);
        mock.handle_pong(2);
    }
}
