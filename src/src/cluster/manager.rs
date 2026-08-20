#![allow(clippy::pedantic)]
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::cluster::cluster::{AppContext, Cluster};
use crate::cluster::file_download::{
    DownloadCleanupRequest, DownloadCleanupTrigger, DownloadSession, DownloadSessionState,
    DownloadShutdownReason, FileDownloadState,
};
use crate::cluster::file_upload::FileUploadState;
use crate::cluster::ssh;
use crate::cluster::traits::{ClusterManagerTrait, ClusterTrait, ConnectionId, WsConnectionSender};
use crate::config::clusters::ClusterConfig;
use crate::config::settings::{
    CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS, CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS,
    CLUSTER_MANAGER_PING_INTERVAL_SECONDS,
};
use crate::protocol::types::ClusterRole;

type FileDownloadAdmissionContext = (
    Arc<dyn crate::cluster::traits::ClusterTrait>,
    Arc<DownloadSession>,
    ConnectionId,
    Arc<Cluster>,
);

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

    /// File download sessions by UUID. Allocation identity is authoritative.
    file_download_map: DashMap<String, Arc<DownloadSession>>,

    /// Dedicated cluster retained by the exact download session allocation.
    file_download_clusters: DashMap<usize, Arc<Cluster>>,

    /// Cleanup endpoint registered independently of HTTP response bodies.
    download_cleanup_sender: tokio::sync::mpsc::UnboundedSender<DownloadCleanupRequest>,

    /// File upload sessions: UUID → (`upload_state`, cluster)
    file_upload_map: DashMap<String, (Arc<FileUploadState>, Arc<Cluster>)>,

    /// Database connection for token lookups, UUID generation, etc.
    db: sea_orm::DatabaseConnection,

    /// Application context shared with clusters
    app_context: Arc<AppContext>,

    /// Whether the manager is running
    running: AtomicBool,

    /// Whether application shutdown has begun. Once true, new dedicated
    /// file-download admission is rejected so existing handlers can drain
    /// without new traffic arriving. Set by [`Self::begin_application_shutdown_inherent`].
    shutdown_initiated: AtomicBool,

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

        let (download_cleanup_sender, download_cleanup_receiver) =
            tokio::sync::mpsc::unbounded_channel();
        let manager = Arc::new(Self {
            clusters: RwLock::new(clusters),
            connection_map: DashMap::new(),
            file_download_map: DashMap::new(),
            file_download_clusters: DashMap::new(),
            download_cleanup_sender,
            file_upload_map: DashMap::new(),
            db,
            app_context,
            running: AtomicBool::new(true),
            shutdown_initiated: AtomicBool::new(false),
            pong_times: DashMap::new(),
            ping_times: DashMap::new(),
            missed_pongs: DashMap::new(),
            pause_resume_locks,
            reconnect_attempts: DashMap::new(),
            last_reconnect_attempt: DashMap::new(),
        });
        Self::start_download_cleanup_worker(&manager, download_cleanup_receiver);
        manager
    }

    fn session_key(session: &Arc<DownloadSession>) -> usize {
        Arc::as_ptr(session) as usize
    }

    fn start_download_cleanup_worker(
        manager: &Arc<Self>,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<DownloadCleanupRequest>,
    ) {
        let manager = Arc::downgrade(manager);
        tokio::spawn(async move {
            while let Some(request) = receiver.recv().await {
                let Some(manager) = manager.upgrade() else {
                    break;
                };
                manager.cleanup_file_download(request).await;
            }
        });
    }

    fn remove_exact_download_session(
        &self,
        download_id: &str,
        session: &Arc<DownloadSession>,
    ) -> bool {
        use dashmap::mapref::entry::Entry;

        match self.file_download_map.entry(download_id.to_owned()) {
            Entry::Occupied(entry) if Arc::ptr_eq(entry.get(), session) => {
                entry.remove();
                true
            }
            Entry::Occupied(_) | Entry::Vacant(_) => false,
        }
    }

    fn remove_exact_connection(&self, connection_id: ConnectionId, cluster: &Arc<Cluster>) -> bool {
        use dashmap::mapref::entry::Entry;

        match self.connection_map.entry(connection_id) {
            Entry::Occupied(entry) if Arc::ptr_eq(entry.get(), cluster) => {
                entry.remove();
                true
            }
            Entry::Occupied(_) | Entry::Vacant(_) => false,
        }
    }

    async fn cleanup_file_download(&self, request: DownloadCleanupRequest) {
        let Some(session) = request.session.upgrade() else {
            return;
        };
        if session.download_id() != request.download_id {
            return;
        }

        let session_key = Self::session_key(&session);
        let Some(cluster) = self
            .file_download_clusters
            .get(&session_key)
            .map(|entry| Arc::clone(entry.value()))
        else {
            return;
        };

        if !self.remove_exact_download_session(&request.download_id, &session) {
            return;
        }
        self.file_download_clusters.remove(&session_key);

        tracing::info!(
            download_id = %request.download_id,
            connection_id = ?request.connection_id,
            reason = ?request.reason,
            "File download shutdown started"
        );

        if let Some(connection_id) = request.connection_id {
            self.remove_exact_connection(connection_id, &cluster);
            self.pong_times.remove(&connection_id);
            self.ping_times.remove(&connection_id);
            self.missed_pongs.remove(&connection_id);
            cluster.close(false).await;
            cluster.terminate_download_tasks().await;
        } else {
            cluster.set_connection(None).await;
            cluster.terminate_download_tasks().await;
            session.complete(None);
        }
    }

    /// Returns `true` when application shutdown has begun. New dedicated
    /// admission paths consult this flag so they can reject without
    /// publishing any new state.
    #[must_use]
    pub fn is_application_shutting_down(&self) -> bool {
        self.shutdown_initiated.load(Ordering::SeqCst)
    }

    /// Begin bounded application shutdown. Sets the dedicated-admission
    /// flag so new file-download admission is rejected for the remainder of
    /// the process lifetime, then synchronously triggers every currently
    /// registered dedicated download session with reason
    /// [`DownloadShutdownReason::ApplicationShutdown`].
    ///
    /// The call is synchronous, non-blocking, and does not spawn a new task.
    /// It reuses the existing one-shot cleanup trigger from task-1 so the
    /// first notification wins and later duplicates are idempotent no-ops.
    /// Existing session-side cleanup (manager worker + handler-exit guard)
    /// routes the trigger through the same `cleanup_file_download` path as
    /// every other reason, so the structured
    /// `File download shutdown started` event still fires exactly once per
    /// accepted connection.
    ///
    /// This method does not await graceful transport closure or task
    /// termination — the caller composes that with the existing
    /// `WS_CLOSE_HANDSHAKE_GRACE_SECONDS` bound.
    ///
    /// Returns the number of sessions that received the shutdown trigger.
    /// Repeated calls are safe and idempotent: the flag is set on the
    /// first call, and later `trigger(...)` calls on already-`Closing`
    /// sessions are no-ops.
    fn begin_application_shutdown_inherent(&self) -> usize {
        if self.shutdown_initiated.swap(true, Ordering::SeqCst) {
            return 0;
        }

        let mut triggered = 0usize;
        for entry in self.file_download_map.iter() {
            let session = entry.value();
            if session
                .cleanup_trigger()
                .trigger(DownloadShutdownReason::ApplicationShutdown)
            {
                triggered += 1;
            }
        }
        triggered
    }

    /// Exact context for a successfully admitted dedicated download handler.
    /// Convenience inherent mirror of the trait implementation that returns
    /// the concrete `Arc<Cluster>` instead of the trait object.
    #[allow(dead_code)]
    pub fn get_file_download_admission(
        &self,
        connection_id: ConnectionId,
    ) -> Option<(Arc<DownloadSession>, ConnectionId, Arc<Cluster>)> {
        self.admit_file_download(connection_id)
            .map(|(_, session, conn_id, cluster)| (session, conn_id, cluster))
    }

    /// Shared lookup used by both the inherent helper and the trait
    /// implementation. Returns the trait-object cluster pointer alongside
    /// the exact `Arc<DownloadSession>` and accepted `ConnectionId` so the
    /// WebSocket handler can retain all three without holding a map guard.
    fn admit_file_download(
        &self,
        connection_id: ConnectionId,
    ) -> Option<FileDownloadAdmissionContext> {
        // During application shutdown we keep serving already-admitted
        // sessions so their handler-exit guard can finalise
        // `Closing -> Closed`. New admission is rejected upstream in
        // `handle_new_connection`, so reaching `get_file_download_admission`
        // during shutdown is permitted; the guard still emits exactly one
        // closed event at handler exit.
        let cluster = self
            .connection_map
            .get(&connection_id)
            .map(|entry| Arc::clone(entry.value()))?;
        if cluster.role() != ClusterRole::FileDownload {
            return None;
        }

        let download_id = cluster.uuid()?.to_owned();
        let session = self
            .file_download_map
            .get(&download_id)
            .map(|entry| Arc::clone(entry.value()))?;
        if session.state() != DownloadSessionState::Connected(connection_id) {
            return None;
        }

        let trait_cluster: Arc<dyn crate::cluster::traits::ClusterTrait> =
            Arc::clone(&cluster) as Arc<dyn crate::cluster::traits::ClusterTrait>;
        Some((trait_cluster, session, connection_id, cluster))
    }

    /// Start background tasks (reconnection, ping).
    pub fn start_tasks(self: &Arc<Self>) {
        tracing::debug!("ClusterManager: Starting background tasks");

        // Start scheduler tasks for each cluster
        {
            match self.clusters.try_read() {
                Ok(clusters) => {
                    tracing::trace!(
                        "ClusterManager: Acquired read lock, starting {} cluster schedulers",
                        clusters.len()
                    );
                    for (name, cluster) in clusters.iter() {
                        tracing::debug!("ClusterManager: Starting tasks for cluster '{}'", name);
                        cluster.start_tasks();
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "ClusterManager: Failed to acquire read lock on clusters: {}",
                        e
                    );
                }
            }
        }

        // Immediate reconnect attempt on startup
        tracing::debug!("ClusterManager: Spawning immediate reconnect task");
        let this = Arc::clone(self);
        tokio::spawn(async move {
            tracing::trace!("ClusterManager: Immediate reconnect task running");
            this.reconnect_clusters().await;
        });

        // Periodic reconnection task
        tracing::debug!(
            "ClusterManager: Spawning periodic reconnect task (interval: {}s)",
            *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS
        );
        let this = Arc::clone(self);
        tokio::spawn(async move { this.run_reconnect().await });

        // Ping task
        tracing::debug!(
            "ClusterManager: Spawning ping task (interval: {}s)",
            *CLUSTER_MANAGER_PING_INTERVAL_SECONDS
        );
        let this = Arc::clone(self);
        tokio::spawn(async move { this.run_ping().await });

        tracing::info!("ClusterManager: All background tasks started");
    }

    /// Background task: periodically reconnect offline clusters.
    async fn run_reconnect(self: Arc<Self>) {
        tracing::debug!("ClusterManager: Reconnect task loop started");
        let mut cycle = 0u64;
        while self.running.load(Ordering::Relaxed) {
            cycle += 1;
            tracing::trace!(
                "ClusterManager: Reconnect cycle {} - sleeping for {}s",
                cycle,
                *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS
            );
            tokio::time::sleep(std::time::Duration::from_secs(
                *CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS,
            ))
            .await;
            if !self.running.load(Ordering::Relaxed) {
                tracing::debug!("ClusterManager: Reconnect task shutting down (running=false)");
                break;
            }
            tracing::trace!("ClusterManager: Starting reconnect cycle {}", cycle);
            self.reconnect_clusters().await;
            tracing::trace!("ClusterManager: Reconnect cycle {} complete", cycle);
        }
        tracing::debug!("ClusterManager: Reconnect task loop exited");
    }

    /// Try to reconnect all offline master clusters.
    pub async fn reconnect_clusters(&self) {
        use crate::db::entities::cluster_uuid;
        use sea_orm::{
            ActiveModelTrait,
            ActiveValue::{NotSet, Set},
            ColumnTrait, EntityTrait, QueryFilter,
        };

        tracing::trace!("ClusterManager: Scanning clusters for reconnection");
        let clusters = self.clusters.read().await;
        let mut reconnected_count = 0;
        let mut skipped_count = 0;

        for (name, cluster) in clusters.iter() {
            if cluster.is_online() {
                tracing::trace!("ClusterManager: Cluster '{}' is online, skipping", name);
                continue;
            }

            tracing::debug!(
                "ClusterManager: Cluster '{}' is offline, checking reconnection",
                name
            );
            let details = cluster.cluster_details();

            // Skip LTK clusters - they connect autonomously
            if details.ltk.is_some() {
                tracing::debug!(
                    "ClusterManager: Skipping LTK cluster '{}' - waits for autonomous connection",
                    name
                );
                skipped_count += 1;
                continue;
            }

            // Exponential backoff: wait base_interval * 2^(attempt-1) before retrying
            // First attempt (attempt == 0) is always allowed; retries back off exponentially
            let attempt = self.reconnect_attempts.get(name).map_or(0, |r| *r);
            if attempt > 0 {
                let backoff_secs = (*CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS)
                    .saturating_mul(2u64.saturating_pow(attempt - 1));
                if let Some(last_attempt) = self.last_reconnect_attempt.get(name) {
                    let elapsed = last_attempt.elapsed().as_secs();
                    if elapsed < backoff_secs {
                        tracing::debug!(
                            "ClusterManager: Skipping reconnect for '{}' (attempt {}, backoff {}s, {}s elapsed)",
                            name,
                            attempt,
                            backoff_secs,
                            elapsed
                        );
                        skipped_count += 1;
                        continue;
                    }
                    tracing::trace!(
                        "ClusterManager: Backoff period expired for '{}' ({}s >= {}s)",
                        name,
                        elapsed,
                        backoff_secs
                    );
                }
            }

            self.reconnect_attempts.insert(name.clone(), attempt + 1);
            self.last_reconnect_attempt
                .insert(name.clone(), std::time::Instant::now());
            tracing::info!(
                "ClusterManager: Reconnecting cluster '{}' (attempt {})",
                name,
                attempt + 1
            );

            let uuid = uuid::Uuid::new_v4().to_string();
            tracing::trace!(
                "ClusterManager: Generated new UUID '{}' for cluster '{}'",
                uuid,
                name
            );

            // Delete any existing UUIDs for this cluster before inserting a new one
            tracing::trace!("ClusterManager: Deleting old UUIDs for cluster '{}'", name);
            let _ = cluster_uuid::Entity::delete_many()
                .filter(cluster_uuid::Column::Cluster.eq(name.as_str()))
                .exec(&self.db)
                .await;

            tracing::trace!(
                "ClusterManager: Inserting new UUID record for cluster '{}'",
                name
            );
            let record = cluster_uuid::ActiveModel {
                id: NotSet,
                cluster: Set(name.clone()),
                uuid: Set(uuid.clone()),
                timestamp: Set(chrono::Utc::now().naive_utc()),
            };
            if let Err(e) = record.insert(&self.db).await {
                tracing::warn!(
                    "ClusterManager: Failed to insert cluster UUID for '{}': {}",
                    name,
                    e
                );
                continue;
            }
            tracing::debug!(
                "ClusterManager: UUID '{}' inserted for cluster '{}'",
                uuid,
                name
            );

            match details.connection_type.as_str() {
                "manual" => {
                    tracing::info!(
                        "ClusterManager: Cluster '{}' requires manual connection. Token: {}",
                        name,
                        uuid
                    );
                }
                "ssh" => {
                    tracing::debug!(
                        "ClusterManager: Initiating SSH connection for cluster '{}'",
                        name
                    );
                    Self::launch_ssh_connection(&details, &uuid);
                }
                "kerberos" => {
                    tracing::debug!(
                        "ClusterManager: Initiating Kerberos connection for cluster '{}'",
                        name
                    );
                    Self::launch_ssh_connection(&details, &uuid);
                }
                other => {
                    tracing::warn!(
                        "ClusterManager: Unknown connection type '{}' for cluster '{}', defaulting to SSH",
                        other,
                        name
                    );
                    Self::launch_ssh_connection(&details, &uuid);
                }
            }
            reconnected_count += 1;
        }

        tracing::debug!(
            "ClusterManager: Reconnect scan complete - {} reconnected, {} skipped",
            reconnected_count,
            skipped_count
        );
    }

    fn launch_ssh_connection(details: &ClusterConfig, token: &str) {
        let config = details.clone();
        let token = token.to_string();
        let cluster_name = config.name.clone();
        tracing::debug!(
            "ClusterManager: Launching SSH connection for cluster '{}' to {}@{}",
            cluster_name,
            config.username,
            config.host
        );

        tokio::spawn(async move {
            tracing::trace!(
                "ClusterManager: SSH task spawned for cluster '{}'",
                cluster_name
            );
            match ssh::run_remote_client(&config, &token).await {
                Ok(()) => {
                    tracing::info!(
                        "ClusterManager: SSH connection completed for cluster '{}'",
                        cluster_name
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "ClusterManager: SSH connection failed for cluster '{}': {}",
                        cluster_name,
                        e
                    );
                }
            }
        });
    }

    /// Background task: periodically ping connected clusters.
    async fn run_ping(self: Arc<Self>) {
        tracing::debug!(
            "ClusterManager: Ping task started (interval: {}s)",
            *CLUSTER_MANAGER_PING_INTERVAL_SECONDS
        );
        let mut ping_cycle = 0u64;

        while self.running.load(Ordering::Relaxed) {
            ping_cycle += 1;
            tracing::trace!("ClusterManager: Ping cycle {} - sleeping", ping_cycle);
            tokio::time::sleep(std::time::Duration::from_secs(
                *CLUSTER_MANAGER_PING_INTERVAL_SECONDS,
            ))
            .await;
            if !self.running.load(Ordering::Relaxed) {
                tracing::debug!("ClusterManager: Ping task shutting down (running=false)");
                break;
            }

            tracing::trace!("ClusterManager: Starting ping cycle {}", ping_cycle);
            self.check_pings().await;
            tracing::trace!("ClusterManager: Ping cycle {} complete", ping_cycle);
        }
        tracing::debug!(
            "ClusterManager: Ping task exited after {} cycles",
            ping_cycle
        );
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

    fn get_file_download_admission(
        &self,
        conn_id: ConnectionId,
    ) -> Option<(
        Arc<crate::cluster::file_download::DownloadSession>,
        ConnectionId,
        Arc<dyn ClusterTrait>,
    )> {
        self.admit_file_download(conn_id)
            .map(|(_, session, conn_id, cluster)| {
                (
                    session,
                    conn_id,
                    Arc::clone(&cluster) as Arc<dyn ClusterTrait>,
                )
            })
    }

    async fn handle_new_connection(
        &self,
        conn_id: ConnectionId,
        ws_sender: WsConnectionSender,
        token: &str,
    ) -> Option<Arc<dyn ClusterTrait>> {
        use crate::db::entities::cluster_uuid;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

        tracing::debug!(
            "ClusterManager: New connection attempt (conn_id={}, token_len={})",
            conn_id,
            token.len()
        );

        // Reject only the dedicated file-download admission branch during
        // application shutdown. Master, LTK, and upload paths are unchanged.
        // The HTTP download endpoint observes the same flag and routes its
        // post-shutdown admission into the existing `SERVICE_UNAVAILABLE`
        // path (see [`http::file::download_file`]) so no session is created
        // and no transport owner is leaked.
        if self.is_application_shutting_down() && self.file_download_map.contains_key(token) {
            tracing::debug!(
                "ClusterManager: Rejecting file-download connection during application shutdown"
            );
            return None;
        }

        // Clone exact identity so no map guard survives sender installation.
        if let Some(session) = self
            .file_download_map
            .get(token)
            .map(|entry| Arc::clone(entry.value()))
        {
            tracing::trace!("ClusterManager: Token matches file download session");

            if session.bind_connection(conn_id).is_err() {
                tracing::warn!(
                    download_id = %token,
                    connection_id = conn_id,
                    "Duplicate or closing file download admission rejected"
                );
                return None;
            }

            let session_key = Self::session_key(&session);
            let Some(cluster) = self
                .file_download_clusters
                .get(&session_key)
                .map(|entry| Arc::clone(entry.value()))
            else {
                let _ = session
                    .cleanup_trigger()
                    .trigger(DownloadShutdownReason::ClusterOffline);
                return None;
            };

            cluster.set_connection(Some(ws_sender)).await;
            if session.state() != DownloadSessionState::Connected(conn_id) {
                cluster.set_connection(None).await;
                return None;
            }

            self.connection_map.insert(conn_id, Arc::clone(&cluster));
            if session.state() != DownloadSessionState::Connected(conn_id) {
                self.remove_exact_connection(conn_id, &cluster);
                cluster.set_connection(None).await;
                return None;
            }

            tracing::debug!(
                "ClusterManager: File download cluster connected (conn_id={})",
                conn_id
            );
            return Some(cluster as Arc<dyn ClusterTrait>);
        }

        // Check file upload map
        if let Some(entry) = self.file_upload_map.get(token) {
            tracing::trace!("ClusterManager: Token matches file upload session");
            let (_, cluster) = entry.value();
            let cluster = Arc::clone(cluster);
            cluster.set_connection(Some(ws_sender)).await;
            self.connection_map.insert(conn_id, cluster.clone());
            tracing::debug!(
                "ClusterManager: File upload cluster connected (conn_id={})",
                conn_id
            );
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
                let timeout = crate::config::settings::ltk_connection_timeout_ms();
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
            - chrono::Duration::try_seconds(
                (*CLUSTER_MANAGER_MAX_TOKEN_EXPIRY_SECONDS).cast_signed(),
            )
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
        let cluster = self
            .connection_map
            .get(&conn_id)
            .map(|entry| Arc::clone(entry.value()));
        if let Some(cluster) = cluster {
            if cluster.role() == ClusterRole::FileDownload {
                if let Some(uuid) = cluster.uuid()
                    && let Some(session) = self
                        .file_download_map
                        .get(uuid)
                        .map(|entry| Arc::clone(entry.value()))
                    && session.state() == DownloadSessionState::Connected(conn_id)
                {
                    let reason = if close {
                        DownloadShutdownReason::WebSocketError
                    } else {
                        DownloadShutdownReason::WebSocketClosed
                    };
                    if session.cleanup_trigger().trigger(reason) {
                        self.cleanup_file_download(DownloadCleanupRequest {
                            download_id: uuid.to_owned(),
                            connection_id: Some(conn_id),
                            reason,
                            session: Arc::downgrade(&session),
                        })
                        .await;
                    }
                }
                return;
            }

            if !self.remove_exact_connection(conn_id, &cluster) {
                return;
            }
            if close {
                cluster.close(false).await;
            }
            cluster.set_connection(None).await;
            self.pong_times.remove(&conn_id);
            self.ping_times.remove(&conn_id);
            self.missed_pongs.remove(&conn_id);

            let role = cluster.role();
            let name = cluster.name();
            tracing::debug!("Connection removed for {} (role={:?})", name, role);

            // Dedicated downloads return above through exact lifecycle cleanup.
            if role == ClusterRole::FileUpload
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
            tracing::trace!(
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
        // Reject new dedicated admissions once application shutdown has
        // begun. The HTTP layer treats the absent session as the existing
        // `ResponseError` typed terminal reason and never reaches the
        // session-creation branch, so no state is published. The original
        // cluster is returned so the HTTP layer does not panic on its
        // `Arc<dyn ClusterTrait>` argument; the subsequent
        // `get_file_download(...)` lookup returns `None` and the existing
        // `SERVICE_UNAVAILABLE` path is taken when the layer also checks
        // `is_application_shutting_down`.
        if self.is_application_shutting_down() {
            tracing::debug!(
                "ClusterManager: Rejecting create_file_download during application shutdown"
            );
            return Arc::clone(cluster);
        }
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

        let session = DownloadSession::new(
            uuid.to_string(),
            download_state,
            self.download_cleanup_sender.clone(),
        );
        self.file_download_clusters
            .insert(Self::session_key(&session), Arc::clone(&dl_cluster));
        self.file_download_map.insert(uuid.to_string(), session);

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
            tracing::warn!("WebSocket error for cluster {}: {}", name, error);
        } else {
            tracing::warn!("WebSocket error (unknown cluster): {}", error);
        }
    }

    fn get_file_download(&self, uuid: &str) -> Option<Arc<FileDownloadState>> {
        if self.is_application_shutting_down() {
            return None;
        }
        self.file_download_map
            .get(uuid)
            .map(|entry| Arc::clone(entry.value().transfer()))
    }

    fn get_file_download_cleanup_trigger(&self, uuid: &str) -> Option<DownloadCleanupTrigger> {
        if self.shutdown_initiated.load(Ordering::SeqCst) {
            return None;
        }
        self.file_download_map
            .get(uuid)
            .map(|entry| entry.value().cleanup_trigger())
    }

    fn is_application_shutting_down(&self) -> bool {
        self.shutdown_initiated.load(Ordering::SeqCst)
    }

    fn begin_application_shutdown(&self) -> usize {
        self.begin_application_shutdown_inherent()
    }

    fn dedicated_download_clusters(
        &self,
    ) -> Vec<std::sync::Weak<dyn crate::cluster::traits::ClusterTrait>> {
        let mut out: Vec<std::sync::Weak<dyn crate::cluster::traits::ClusterTrait>> = Vec::new();
        for entry in self.file_download_clusters.iter() {
            let cluster = Arc::clone(entry.value());
            out.push(Arc::downgrade(&cluster)
                as std::sync::Weak<dyn crate::cluster::traits::ClusterTrait>);
        }
        out
    }

    fn get_file_upload(&self, uuid: &str) -> Option<Arc<FileUploadState>> {
        self.file_upload_map
            .get(uuid)
            .map(|entry| Arc::clone(&entry.value().0))
    }
}
impl ClusterManager {
    /// Test-only: clone every concrete `Arc<Cluster>` retained for
    /// dedicated file-download shutdown.
    #[allow(dead_code)]
    pub fn dedicated_download_clusters_concrete(
        &self,
    ) -> Vec<Arc<crate::cluster::cluster::Cluster>> {
        self.file_download_clusters
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect()
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
    async fn test_start_tasks_triggers_immediate_reconnect() {
        let db = make_db().await;
        let manager = ClusterManager::new(test_configs(), db.clone(), Arc::new(DashMap::new()));

        let manager_arc = Arc::clone(&manager);
        manager_arc.start_tasks();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while get_uuid_for_cluster(&db, "cluster_b").await.is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "start_tasks should trigger immediate reconnect attempt"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        assert_eq!(
            manager.reconnect_attempts.get("cluster_b").map(|v| *v),
            Some(1),
            "start_tasks should trigger immediate reconnect attempt"
        );
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
