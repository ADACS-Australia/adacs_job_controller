#![allow(clippy::pedantic)]
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::{Notify, RwLock};

use crate::cluster::file_download::FileDownloadState;
use crate::cluster::file_upload::FileUploadState;
use crate::cluster::traits::{ClusterTrait, WsConnectionSender, WsOutbound};
use crate::config::clusters::ClusterConfig;
use crate::config::settings::{
    CLIENT_TIMEOUT_SECONDS, CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS,
    CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS, MAX_FILE_BUFFER_SIZE, MIN_FILE_BUFFER_SIZE,
    QUEUE_SOURCE_PRUNE_MILLISECONDS,
};
use crate::protocol::constants::{
    CANCEL_JOB, DELETE_JOB, FILE_CHUNK, FILE_DETAILS, FILE_ERROR, FILE_LIST, FILE_LIST_ERROR,
    FILE_UPLOAD_COMPLETE, FILE_UPLOAD_ERROR, JOB_COMPLETION_SOURCE, PAUSE_FILE_CHUNK_STREAM,
    SERVER_READY, SUBMIT_JOB, UPDATE_JOB,
};
use crate::protocol::message::Message;
use crate::protocol::types::{ClusterRole, FileInfo, FileListState, JobStatus, Priority};
use crate::utils::uuid::generate_uuid;

/// Shared application context needed by Cluster for DB and file-list coordination.
pub struct AppContext {
    pub db: sea_orm::DatabaseConnection,
    pub file_list_map: Arc<DashMap<String, Arc<tokio::sync::Mutex<FileListState>>>>,
}

/// The core Cluster type that implements message queuing, scheduling, and dispatch.
///
/// Each Cluster holds a priority-based message queue and a WS connection sender.
/// Background tasks handle scheduling (dequeue + send), source pruning, and
/// message resend for stale jobs.
pub struct Cluster {
    details: ClusterConfig,
    role: ClusterRole,
    role_string: String,

    // WebSocket connection state (None = offline)
    connection_tx: tokio::sync::watch::Sender<Option<WsConnectionSender>>,
    connection_rx: tokio::sync::watch::Receiver<Option<WsConnectionSender>>,

    // Priority queue: BTreeMap ensures iteration in priority order (lowest enum value = highest priority)
    // Each priority maps source-strings to their per-source queues.
    #[allow(clippy::type_complexity)]
    queue: BTreeMap<u8, RwLock<HashMap<String, VecDeque<Vec<u8>>>>>,

    queued_message_size: AtomicUsize,

    // Notifications
    data_notify: Notify,
    queue_size_notify: Notify,

    running: AtomicBool,
    app_context: Option<Arc<AppContext>>,

    // File transfer state (only populated for file download/upload roles)
    file_download_state: Option<Arc<FileDownloadState>>,
    file_upload_state: Option<Arc<FileUploadState>>,
    uuid: Option<String>,

    // Backpressure mutex shared with HTTP download handlers
    file_download_pause_resume_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Cluster {
    /// Create a new master cluster.
    #[must_use]
    pub fn new(details: ClusterConfig, app_context: Option<Arc<AppContext>>) -> Arc<Self> {
        let mut queue = BTreeMap::new();
        queue.insert(Priority::Highest as u8, RwLock::new(HashMap::new()));
        queue.insert(Priority::Medium as u8, RwLock::new(HashMap::new()));
        queue.insert(Priority::Lowest as u8, RwLock::new(HashMap::new()));
        let (connection_tx, connection_rx) = tokio::sync::watch::channel(None);

        Arc::new(Self {
            role_string: format!("master {}", details.name),
            details,
            role: ClusterRole::Master,
            connection_tx,
            connection_rx,
            queue,
            queued_message_size: AtomicUsize::new(0),
            data_notify: Notify::new(),
            queue_size_notify: Notify::new(),
            running: AtomicBool::new(true),
            app_context,
            file_download_state: None,
            file_upload_state: None,
            uuid: None,
            file_download_pause_resume_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// Create a file-download cluster.
    pub fn new_file_download(
        details: ClusterConfig,
        uuid: String,
        download_state: Arc<FileDownloadState>,
        app_context: Option<Arc<AppContext>>,
        pause_resume_lock: Arc<tokio::sync::Mutex<()>>,
    ) -> Arc<Self> {
        let mut queue = BTreeMap::new();
        queue.insert(Priority::Highest as u8, RwLock::new(HashMap::new()));
        queue.insert(Priority::Medium as u8, RwLock::new(HashMap::new()));
        queue.insert(Priority::Lowest as u8, RwLock::new(HashMap::new()));
        let (connection_tx, connection_rx) = tokio::sync::watch::channel(None);

        Arc::new(Self {
            role_string: format!("file download {uuid}"),
            details,
            role: ClusterRole::FileDownload,
            connection_tx,
            connection_rx,
            queue,
            queued_message_size: AtomicUsize::new(0),
            data_notify: Notify::new(),
            queue_size_notify: Notify::new(),
            running: AtomicBool::new(true),
            app_context,
            file_download_state: Some(download_state),
            file_upload_state: None,
            uuid: Some(uuid),
            file_download_pause_resume_lock: pause_resume_lock,
        })
    }

    /// Create a file-upload cluster.
    pub fn new_file_upload(
        details: ClusterConfig,
        uuid: String,
        upload_state: Arc<FileUploadState>,
        app_context: Option<Arc<AppContext>>,
    ) -> Arc<Self> {
        let mut queue = BTreeMap::new();
        queue.insert(Priority::Highest as u8, RwLock::new(HashMap::new()));
        queue.insert(Priority::Medium as u8, RwLock::new(HashMap::new()));
        queue.insert(Priority::Lowest as u8, RwLock::new(HashMap::new()));
        let (connection_tx, connection_rx) = tokio::sync::watch::channel(None);

        Arc::new(Self {
            role_string: format!("file upload {uuid}"),
            details,
            role: ClusterRole::FileUpload,
            connection_tx,
            connection_rx,
            queue,
            queued_message_size: AtomicUsize::new(0),
            data_notify: Notify::new(),
            queue_size_notify: Notify::new(),
            running: AtomicBool::new(true),
            app_context,
            file_download_state: None,
            file_upload_state: Some(upload_state),
            uuid: Some(uuid),
            file_download_pause_resume_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    pub fn uuid(&self) -> Option<&str> {
        self.uuid.as_deref()
    }

    /// Start background tasks (scheduler, prune, resend).
    /// Must be called after construction; pass the Arc to self.
    pub fn start_tasks(self: &Arc<Self>) {
        let cluster_name = self.name();
        tracing::debug!(
            "Cluster[{}]: Starting background tasks (role: {:?})",
            cluster_name,
            self.role
        );

        // Each spawned task gets its own watch receiver
        let scheduler_rx = self.connection_rx.clone();
        let this = Arc::clone(self);
        tracing::trace!("Cluster[{}]: Spawning scheduler task", cluster_name);
        tokio::spawn(async move { this.run_scheduler(scheduler_rx).await });

        let this = Arc::clone(self);
        tracing::trace!("Cluster[{}]: Spawning prune task", cluster_name);
        tokio::spawn(async move { this.run_prune().await });

        if self.role == ClusterRole::Master {
            let this = Arc::clone(self);
            tracing::trace!(
                "Cluster[{}]: Spawning resend task (master role)",
                cluster_name
            );
            tokio::spawn(async move { this.run_resend().await });
        } else {
            tracing::debug!(
                "Cluster[{}]: Skipping resend task (not master role)",
                cluster_name
            );
        }

        tracing::info!("Cluster[{}]: All background tasks started", cluster_name);
    }

    // ---- Scheduler ----

    async fn run_scheduler(
        self: Arc<Self>,
        connection_rx: tokio::sync::watch::Receiver<Option<WsConnectionSender>>,
    ) {
        tracing::debug!("Cluster[{}]: Scheduler task started", self.name());
        let mut cycle = 0u64;
        let mut total_sent = 0u64;

        while self.running.load(Ordering::Relaxed) {
            cycle += 1;
            // Wait for data
            tracing::trace!(
                "Cluster[{}]: Scheduler cycle {} - waiting for data notification",
                self.name(),
                cycle
            );
            self.data_notify.notified().await;
            if !self.running.load(Ordering::Relaxed) {
                tracing::debug!(
                    "Cluster[{}]: Scheduler shutting down (running=false)",
                    self.name()
                );
                break;
            }

            // Process queues with priority preemption
            tracing::trace!(
                "Cluster[{}]: Processing queues (cycle {})",
                self.name(),
                cycle
            );
            'reset: loop {
                let mut sent_anything = false;
                for (&priority_val, rw_map) in &self.queue {
                    let mut had_data_this_round;
                    loop {
                        had_data_this_round = false;
                        let mut map = rw_map.write().await;
                        let sources: Vec<String> = map.keys().cloned().collect();
                        for source in &sources {
                            if let Some(queue) = map.get_mut(source)
                                && let Some(data) = queue.pop_front()
                            {
                                let data_len = data.len();
                                self.queued_message_size
                                    .fetch_sub(data_len, Ordering::Relaxed);
                                self.queue_size_notify.notify_waiters();
                                drop(map);

                                // Send via WS connection (non-blocking read of current state)
                                if let Some(sender) = connection_rx.borrow().as_ref() {
                                    tracing::trace!(
                                        "Cluster[{}]: Sending message ({} bytes, priority {}, source {})",
                                        self.name(),
                                        data_len,
                                        priority_val,
                                        source
                                    );
                                    let _ = sender.send(WsOutbound::Binary(data));
                                    total_sent += 1;
                                } else {
                                    tracing::warn!(
                                        "Cluster[{}]: Discarding packet (source {}, priority {}) - connection closed",
                                        self.name(),
                                        source,
                                        priority_val
                                    );
                                }

                                had_data_this_round = true;
                                sent_anything = true;

                                // Check for higher priority data
                                if self.has_higher_priority_data(priority_val).await {
                                    tracing::trace!(
                                        "Cluster[{}]: Higher priority data detected - resetting loop",
                                        self.name()
                                    );
                                    continue 'reset;
                                }

                                // Re-acquire map for next source
                                map = rw_map.write().await;
                            }
                        }
                        if !had_data_this_round {
                            break;
                        }
                    }
                }
                if !sent_anything {
                    break;
                }
                break; // All priorities processed without preemption
            }
            tracing::trace!(
                "Cluster[{}]: Scheduler cycle {} complete (sent total: {})",
                self.name(),
                cycle,
                total_sent
            );
        }
        tracing::debug!(
            "Cluster[{}]: Scheduler task exited (total messages sent: {})",
            self.name(),
            total_sent
        );
    }

    async fn has_higher_priority_data(&self, max_priority: u8) -> bool {
        for (&priority_val, rw_map) in &self.queue {
            if priority_val >= max_priority {
                return false;
            }
            let map = rw_map.read().await;
            for queue in map.values() {
                if !queue.is_empty() {
                    return true;
                }
            }
        }
        false
    }

    // ---- Prune ----

    async fn run_prune(self: Arc<Self>) {
        while self.running.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(
                *QUEUE_SOURCE_PRUNE_MILLISECONDS,
            ))
            .await;
            if !self.running.load(Ordering::Relaxed) {
                break;
            }
            self.prune_once().await;
        }
    }

    /// Execute one prune cycle: remove all empty source queues.
    /// Public for testing.
    pub async fn prune_once(&self) {
        for rw_map in self.queue.values() {
            let mut map = rw_map.write().await;
            map.retain(|_, queue| !queue.is_empty());
        }
    }

    // ---- Resend ----

    async fn run_resend(self: Arc<Self>) {
        while self.running.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(
                *CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS,
            ))
            .await;
            if !self.running.load(Ordering::Relaxed) {
                break;
            }
            self.check_unsubmitted_jobs().await;
            self.check_cancelling_jobs().await;
            self.check_deleting_jobs().await;
        }
    }

    // ---- Message handling ----

    /// Processes an `UPDATE_JOB` message by recording job history and caching the file list on completion.
    async fn handle_update_job(&self, message: &mut Message) {
        use crate::db::entities::job_history;
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};

        let job_id = message.pop_uint();
        let what = message.pop_string();
        let status = message.pop_uint();
        let details = message.pop_string();

        tracing::trace!(
            "Cluster[{}]: Received UPDATE_JOB - job_id={}, what={}, status={}, details={}",
            self.name(),
            job_id,
            what,
            status,
            details
        );

        let Some(ctx) = &self.app_context else {
            tracing::warn!(
                "Cluster[{}]: No app context available - cannot process UPDATE_JOB",
                self.name()
            );
            return;
        };

        tracing::trace!(
            "Cluster[{}]: Inserting job history for job {}",
            self.name(),
            job_id
        );
        let record = job_history::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            job_id: Set(i64::from(job_id)),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            what: Set(what.clone()),
            state: Set(status.cast_signed()),
            details: Set(details.clone()),
        };
        if let Err(e) = record.insert(&ctx.db).await {
            tracing::warn!(
                "Cluster[{}]: Failed to insert job history for job {} status {}: {}",
                self.name(),
                job_id,
                status,
                e
            );
        } else {
            tracing::debug!(
                "Cluster[{}]: Job history inserted for job {} (status {})",
                self.name(),
                job_id,
                status
            );
        }

        // On job completion, proactively cache the file list in the background.
        if what == JOB_COMPLETION_SOURCE {
            self.cache_file_list_on_completion(ctx, job_id).await;
        }
    }

    /// Cache the file list for a completed job in the background.
    async fn cache_file_list_on_completion(&self, ctx: &AppContext, job_id: u32) {
        use crate::db::entities::{file_list_cache, job};
        use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

        let model = job::Entity::find_by_id(i64::from(job_id))
            .one(&ctx.db)
            .await
            .ok()
            .flatten();

        if let Some(m) = model {
            let bundle = m.bundle;
            let uuid = generate_uuid();
            let fl_state = Arc::new(tokio::sync::Mutex::new(FileListState::new()));
            ctx.file_list_map
                .insert(uuid.clone(), Arc::clone(&fl_state));

            // Send FILE_LIST request through this cluster's WS connection
            let mut msg = Message::new(FILE_LIST, Priority::Highest, &uuid);
            msg.push_uint(job_id);
            msg.push_string(&uuid);
            msg.push_string(&bundle);
            msg.push_string("");
            msg.push_bool(true);
            self.send_message_internal(msg).await;

            // Background: wait for FILE_LIST_RESPONSE and persist to cache
            let db = ctx.db.clone();
            let file_list_map = ctx.file_list_map.clone();
            let uuid_bg = uuid.clone();
            tokio::spawn(async move {
                let timeout = std::time::Duration::from_secs(*CLIENT_TIMEOUT_SECONDS);
                let fl_clone = Arc::clone(&fl_state);
                let _ = tokio::time::timeout(timeout, async {
                    loop {
                        let notify = {
                            let locked = fl_clone.lock().await;
                            if locked.data_ready {
                                return;
                            }
                            Arc::clone(&locked.notify)
                        };
                        notify.notified().await;
                    }
                })
                .await;

                let locked = fl_state.lock().await;
                if !locked.error {
                    let _ = file_list_cache::Entity::delete_many()
                        .filter(file_list_cache::Column::JobId.eq(i64::from(job_id)))
                        .exec(&db)
                        .await;
                    for file in &locked.files {
                        let _ = file_list_cache::ActiveModel {
                            job_id: Set(i64::from(job_id)),
                            path: Set(file.file_name.clone()),
                            is_dir: Set(file.is_directory),
                            file_size: Set(file.file_size.cast_signed()),
                            permissions: Set(file.permissions.cast_signed()),
                            ..Default::default()
                        }
                        .insert(&db)
                        .await;
                    }
                }
                file_list_map.remove(&uuid_bg);
            });
        }
    }

    /// Parses a `FILE_LIST` response payload into file entries and updates the matching [`FileListState`].
    async fn handle_file_list_response(&self, message: &mut Message) {
        let uuid = message.pop_string();
        let num_files = message.pop_uint();

        let mut files = Vec::with_capacity(num_files as usize);
        for _ in 0..num_files {
            let file_name = message.pop_string();
            let is_directory = message.pop_bool();
            let file_size = message.pop_ulong();
            files.push(FileInfo {
                file_name,
                is_directory,
                file_size,
                permissions: 0,
            });
        }

        let Some(ctx) = &self.app_context else {
            tracing::warn!(
                "Cluster[{}]: FILE_LIST received but no app_context (uuid={uuid})",
                self.name()
            );
            return;
        };
        let Some(fl_state) = ctx.file_list_map.get(&uuid) else {
            tracing::warn!(
                "Cluster[{}]: FILE_LIST received for unknown uuid={uuid}",
                self.name()
            );
            return;
        };

        let mut state = fl_state.lock().await;
        state.files = files;
        state.data_ready = true;
        state.notify.notify_waiters();
    }

    /// Handles a `FILE_LIST_ERROR` response by recording the error details and waking waiters.
    async fn handle_file_list_error(&self, message: &mut Message) {
        let uuid = message.pop_string();
        let detail = message.pop_string();

        let Some(ctx) = &self.app_context else {
            tracing::warn!(
                "Cluster[{}]: FILE_LIST_ERROR received but no app_context (uuid={uuid})",
                self.name()
            );
            return;
        };
        let Some(fl_state) = ctx.file_list_map.get(&uuid) else {
            tracing::warn!(
                "Cluster[{}]: FILE_LIST_ERROR received for unknown uuid={uuid}",
                self.name()
            );
            return;
        };

        let mut state = fl_state.lock().await;
        state.error = true;
        state.error_details = detail;
        state.data_ready = true;
        state.notify.notify_waiters();
    }

    // ---- FileDownload message handling ----

    /// Forwards a file chunk to the HTTP download handler and sends `PAUSE_FILE_CHUNK_STREAM` when buffered bytes exceed the limit.
    async fn handle_file_chunk(&self, message: &mut Message) {
        let Some(state) = &self.file_download_state else {
            tracing::warn!(
                "Cluster[{}]: FILE_CHUNK received but no file_download_state",
                self.name()
            );
            return;
        };

        let chunk = message.pop_bytes();
        let chunk_len = chunk.len() as u64;
        tracing::trace!(
            "Cluster[{}]: FILE_CHUNK received - {} bytes",
            self.name(),
            chunk_len
        );

        state.received_bytes.fetch_add(chunk_len, Ordering::Relaxed);
        // If HTTP side has disconnected, the receiver is dropped and send fails
        if state.chunk_sender.send(chunk).is_err() {
            tracing::debug!(
                "Cluster[{}]: FILE_CHUNK send failed - HTTP client disconnected",
                self.name()
            );
            state.error.store(true, Ordering::Release);
            *state.error_details.lock().await =
                "Download aborted: HTTP client disconnected".to_string();
            state.data_ready.store(true, Ordering::Release);
            state.data_notify.notify_waiters();
            return;
        }

        // Backpressure: send PAUSE if buffer too big
        let _lock = self.file_download_pause_resume_lock.lock().await;
        if !state.client_paused.load(Ordering::Relaxed) {
            let received = state.received_bytes.load(Ordering::Relaxed);
            let sent = state.sent_bytes.load(Ordering::Relaxed);
            if received.saturating_sub(sent) > *MAX_FILE_BUFFER_SIZE {
                state.client_paused.store(true, Ordering::Relaxed);
                let uuid = self.uuid.as_deref().unwrap_or("");
                let msg = Message::new(PAUSE_FILE_CHUNK_STREAM, Priority::Highest, uuid);
                self.send_message_internal(msg).await;
                tracing::debug!(
                    "Cluster[{}]: Sent PAUSE_FILE_CHUNK_STREAM (buffer full)",
                    self.name()
                );
            }
        }

        state.data_ready.store(true, Ordering::Release);
        state.data_notify.notify_waiters();
        tracing::trace!(
            "Cluster[{}]: FILE_CHUNK forwarded to HTTP handler",
            self.name()
        );
    }

    fn handle_file_details(&self, message: &mut Message) {
        let Some(state) = &self.file_download_state else {
            tracing::warn!(
                "Cluster[{}]: FILE_DETAILS received but no file_download_state",
                self.name()
            );
            return;
        };

        let file_size = message.pop_ulong();
        tracing::debug!(
            "Cluster[{}]: FILE_DETAILS received - file_size={} bytes",
            self.name(),
            file_size
        );
        state.file_size.store(file_size, Ordering::Relaxed);
        state.received_data.store(true, Ordering::Release);
        state.data_ready.store(true, Ordering::Release);
        state.data_notify.notify_waiters();
        tracing::debug!(
            "Cluster[{}]: FILE_DETAILS processed - data_ready set",
            self.name()
        );
    }

    /// Records a file download error from the cluster and notifies waiting HTTP clients.
    async fn handle_file_error(&self, message: &mut Message) {
        let Some(state) = &self.file_download_state else {
            tracing::warn!(
                "Cluster[{}]: FILE_ERROR received but no file_download_state",
                self.name()
            );
            return;
        };

        let details = message.pop_string();
        tracing::warn!(
            "Cluster[{}]: FILE_ERROR received - {}",
            self.name(),
            details
        );
        *state.error_details.lock().await = details;
        state.error.store(true, Ordering::Release);
        state.data_ready.store(true, Ordering::Release);
        state.data_notify.notify_waiters();
    }

    // ---- FileUpload message handling ----

    /// Signals that the file-upload server is ready and unblocks waiting upload readers.
    #[allow(clippy::unused_async)]
    async fn handle_server_ready(&self) {
        let Some(state) = &self.file_upload_state else {
            tracing::warn!(
                "Cluster[{}]: SERVER_READY received but no file_upload_state",
                self.name()
            );
            return;
        };
        state.data_ready.store(true, Ordering::Release);
        state.data_notify.notify_waiters();
    }

    /// Stores the file upload error details and notifies any waiting readers.
    async fn handle_file_upload_error(&self, message: &mut Message) {
        let Some(state) = &self.file_upload_state else {
            tracing::warn!(
                "Cluster[{}]: FILE_UPLOAD_ERROR received but no file_upload_state",
                self.name()
            );
            return;
        };
        let details = message.pop_string();
        tracing::warn!(
            "Cluster[{}]: FILE_UPLOAD_ERROR received - {}",
            self.name(),
            details
        );
        *state.error_details.lock().await = details;
        state.error.store(true, Ordering::Release);
        state.data_ready.store(true, Ordering::Release);
        state.data_notify.notify_waiters();
    }

    /// Marks the file upload as complete and notifies any waiting readers.
    fn handle_file_upload_complete(&self) {
        let Some(state) = &self.file_upload_state else {
            tracing::warn!(
                "Cluster[{}]: FILE_UPLOAD_COMPLETE received but no file_upload_state",
                self.name()
            );
            return;
        };
        state.complete.store(true, Ordering::Release);
        state.received_data.store(true, Ordering::Release);
        state.data_ready.store(true, Ordering::Release);
        state.data_notify.notify_waiters();
    }

    // ---- Resend helpers ----

    pub async fn check_unsubmitted_jobs(&self) {
        self.check_resend_jobs(
            &[JobStatus::Pending as i32, JobStatus::Submitting as i32],
            SUBMIT_JOB,
            "Resubmitting",
            true,
        )
        .await;
    }

    pub async fn check_cancelling_jobs(&self) {
        self.check_resend_jobs(
            &[JobStatus::Cancelling as i32],
            CANCEL_JOB,
            "Recancelling",
            false,
        )
        .await;
    }

    pub async fn check_deleting_jobs(&self) {
        self.check_resend_jobs(
            &[JobStatus::Deleting as i32],
            DELETE_JOB,
            "Redeleting",
            false,
        )
        .await;
    }

    async fn check_resend_jobs(
        &self,
        states: &[i32],
        message_id: u32,
        log_label: &'static str,
        push_bundle_and_params: bool,
    ) {
        use crate::db::entities::{job, job_history};
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
        use std::collections::HashMap;

        if !self.is_online_internal() {
            return;
        }
        let Some(ctx) = &self.app_context else {
            tracing::warn!(
                "Cluster[{}]: {} skipped - no app context available",
                self.name(),
                log_label
            );
            return;
        };
        let db = &ctx.db;
        let cluster_name = &self.details.name;
        let ignore_secs = (*CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS).cast_signed();
        let cutoff = chrono::Utc::now().naive_utc()
            - chrono::Duration::try_seconds(ignore_secs).unwrap_or_default();

        let jobs = job::Entity::find()
            .filter(job::Column::Cluster.eq(cluster_name.as_str()))
            .limit(500)
            .all(db)
            .await
            .unwrap_or_default();

        if jobs.is_empty() {
            return;
        }

        // Batch-fetch all relevant history entries per job (2 queries instead of N+1).
        // `id DESC` breaks ties when multiple rows are written in the same millisecond
        // (Pending + Submitting can be inserted back-to-back at job creation time).
        let job_ids: Vec<i64> = jobs.iter().map(|j| j.id).collect();
        let all_histories = job_history::Entity::find()
            .filter(job_history::Column::JobId.is_in(job_ids))
            .filter(job_history::Column::Timestamp.lte(cutoff))
            .order_by_desc(job_history::Column::Timestamp)
            .order_by_desc(job_history::Column::Id)
            .all(db)
            .await
            .unwrap_or_default();

        let mut latest_per_job: HashMap<i64, &job_history::Model> = HashMap::new();
        // Track the set of states a job has ever reached; if any state outside
        // `states` has been seen, the job is in-flight or terminal and must not
        // be re-triggered. Prevents failed jobs from being looped back into
        // "Resubmitting" when their Submitting row sorts first under same-ms ties.
        let mut seen_terminal_or_inflight: HashMap<i64, bool> = HashMap::new();
        for h in &all_histories {
            latest_per_job.entry(h.job_id).or_insert(h);
            if !states.contains(&h.state) {
                seen_terminal_or_inflight.insert(h.job_id, true);
            }
        }

        for j in &jobs {
            if seen_terminal_or_inflight.contains_key(&j.id) {
                continue;
            }
            if let Some(h) = latest_per_job.get(&j.id)
                && states.contains(&h.state)
            {
                tracing::debug!("{}: {}", log_label, j.id);
                let mut msg = Message::new(
                    message_id,
                    Priority::Medium,
                    &format!("{}_{}", j.id, cluster_name),
                );
                msg.push_uint(u32::try_from(j.id).unwrap());
                if push_bundle_and_params {
                    msg.push_string(&j.bundle);
                    msg.push_string(&j.parameters);
                }
                self.send_message_internal(msg).await;
            }
        }
    }

    // Internal helpers

    fn is_online_internal(&self) -> bool {
        self.connection_rx.borrow().is_some()
    }

    async fn send_message_internal(&self, message: Message) {
        let priority = message.priority();
        let data = message.into_data();
        let source = String::new();
        self.queue_message(source, data, priority).await;
    }
}

#[async_trait]
impl ClusterTrait for Cluster {
    fn name(&self) -> String {
        self.details.name.clone()
    }

    fn is_online(&self) -> bool {
        self.connection_rx.borrow().is_some()
    }

    fn role(&self) -> ClusterRole {
        self.role
    }

    fn role_string(&self) -> String {
        self.role_string.clone()
    }

    fn cluster_details(&self) -> ClusterConfig {
        self.details.clone()
    }

    /// Dispatches an incoming WebSocket message to the appropriate handler.
    ///
    /// Master clusters first route `DB_*` messages to `ClusterDB`. Remaining messages
    /// are matched by ID to job, file-list, file-download, or file-upload handlers
    /// based on the cluster role.
    async fn handle_message(&self, mut message: Message) {
        // Try ClusterDB first (for master clusters)
        if self.role == ClusterRole::Master
            && let Some(ctx) = &self.app_context
            && crate::db::cluster_db::maybe_handle_cluster_db_message(&mut message, self, &ctx.db)
                .await
        {
            return;
        }

        match message.id() {
            // Master cluster messages
            UPDATE_JOB => self.handle_update_job(&mut message).await,
            FILE_LIST => self.handle_file_list_response(&mut message).await,
            FILE_LIST_ERROR => self.handle_file_list_error(&mut message).await,

            // FileDownload messages
            FILE_CHUNK => self.handle_file_chunk(&mut message).await,
            FILE_DETAILS => self.handle_file_details(&mut message),
            FILE_ERROR => self.handle_file_error(&mut message).await,

            // FileUpload messages
            SERVER_READY if self.role == ClusterRole::FileUpload => {
                self.handle_server_ready().await;
            }
            FILE_UPLOAD_ERROR => self.handle_file_upload_error(&mut message).await,
            FILE_UPLOAD_COMPLETE if self.role == ClusterRole::FileUpload => {
                self.handle_file_upload_complete();
            }

            other => {
                tracing::warn!(
                    "Got invalid message ID {} from {}",
                    other,
                    self.details.name
                );
            }
        }
    }

    async fn send_message(&self, message: Message) {
        let source = message.source().to_string();
        let priority = message.priority();
        let data = message.into_data();
        self.queue_message(source, data, priority).await;
    }

    async fn queue_message(&self, source: String, data: Vec<u8>, priority: Priority) {
        let priority_val = priority as u8;
        tracing::trace!(
            "Cluster[{}]: Queueing message (source={}, priority={:?}, size={} bytes)",
            self.name(),
            source,
            priority,
            data.len()
        );

        if let Some(rw_map) = self.queue.get(&priority_val) {
            let data_len = data.len();
            let mut map = rw_map.write().await;
            map.entry(source).or_default().push_back(data);
            self.queued_message_size
                .fetch_add(data_len, Ordering::Relaxed);

            let new_size = self.queued_message_size.load(Ordering::Relaxed);
            tracing::trace!(
                "Cluster[{}]: Message queued - queue size now {} bytes",
                self.name(),
                new_size
            );
            self.data_notify.notify_one();
        } else {
            tracing::warn!(
                "Cluster[{}]: Priority level {} not found in queue map",
                self.name(),
                priority_val
            );
        }
    }

    async fn wait_for_queue_drain(&self, wait_for_empty: bool) -> bool {
        let current = self.queued_message_size.load(Ordering::Relaxed);

        if wait_for_empty {
            if current == 0 {
                return true;
            }
            let timeout = std::time::Duration::from_secs(*CLIENT_TIMEOUT_SECONDS);
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                tokio::select! {
                    () = self.queue_size_notify.notified() => {
                        if self.queued_message_size.load(Ordering::Relaxed) == 0 {
                            return true;
                        }
                    }
                    () = tokio::time::sleep_until(deadline) => {
                        return self.queued_message_size.load(Ordering::Relaxed) == 0;
                    }
                }
            }
        } else {
            if current <= (*MAX_FILE_BUFFER_SIZE) as usize {
                return true;
            }
            let timeout = std::time::Duration::from_secs(*CLIENT_TIMEOUT_SECONDS);
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                tokio::select! {
                    () = self.queue_size_notify.notified() => {
                        if self.queued_message_size.load(Ordering::Relaxed)
                            <= *MIN_FILE_BUFFER_SIZE as usize
                        {
                            return true;
                        }
                    }
                    () = tokio::time::sleep_until(deadline) => {
                        return self.queued_message_size.load(Ordering::Relaxed)
                            <= (*MIN_FILE_BUFFER_SIZE) as usize;
                    }
                }
            }
        }
    }

    async fn set_connection(&self, conn: Option<WsConnectionSender>) {
        let _ = self.connection_tx.send(conn);
    }

    fn send_ping(&self) {
        if let Some(sender) = self.connection_rx.borrow().as_ref() {
            let _ = sender.send(WsOutbound::Ping);
        }
    }

    async fn close(&self, _force: bool) {
        // Signal the WS forwarder to send a Close frame to the peer
        // BEFORE clearing the watch channel. We must borrow the
        // current sender first; if we cleared the watch first the
        // sender would be dropped and the close signal lost. The
        // peer then sees a real WebSocket Close (and EOF on the
        // TCP read) and can reconnect, instead of being stranded
        // on a half-open socket that still pongs.
        if let Some(sender) = self.connection_rx.borrow().as_ref() {
            let _ = sender.send(WsOutbound::Close);
        }
        let _ = self.connection_tx.send(None);
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.data_notify.notify_waiters();
        self.queue_size_notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ClusterConfig {
        ClusterConfig {
            name: "test_cluster".to_string(),
            host: "localhost".to_string(),
            username: "user".to_string(),
            path: "/tmp".to_string(),
            key: String::new(),
            connection_type: "manual".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        }
    }

    /// Verifies basic `Cluster::new` construction sets the expected name, offline status, and role.
    #[test]
    fn test_cluster_creation() {
        let cluster = Cluster::new(test_config(), None);
        assert_eq!(cluster.name(), "test_cluster");
        assert!(!cluster.is_online());
        assert_eq!(cluster.role(), ClusterRole::Master);
    }

    /// Verifies that `new_file_download` creates a cluster with `FileDownload` role and correct UUID.
    #[test]
    fn test_cluster_file_download_creation() {
        let state = Arc::new(FileDownloadState::new());
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let cluster =
            Cluster::new_file_download(test_config(), "uuid-123".into(), state, None, lock);
        assert_eq!(cluster.role(), ClusterRole::FileDownload);
        assert_eq!(cluster.uuid(), Some("uuid-123"));
        assert!(cluster.role_string().contains("file download"));
    }

    /// Verifies that `new_file_upload` creates a cluster with `FileUpload` role and correct UUID.
    #[test]
    fn test_cluster_file_upload_creation() {
        let state = Arc::new(FileUploadState::new());
        let cluster = Cluster::new_file_upload(test_config(), "uuid-456".into(), state, None);
        assert_eq!(cluster.role(), ClusterRole::FileUpload);
        assert_eq!(cluster.uuid(), Some("uuid-456"));
        assert!(cluster.role_string().contains("file upload"));
    }

    /// Verifies that `queue_message` increments `queued_message_size` by the payload length.
    #[tokio::test]
    async fn test_queue_message_and_size() {
        let cluster = Cluster::new(test_config(), None);
        let data = vec![1u8, 2, 3, 4, 5];
        cluster
            .queue_message("source1".into(), data.clone(), Priority::Medium)
            .await;

        // Give tokio a moment to process
        tokio::task::yield_now().await;

        assert_eq!(cluster.queued_message_size.load(Ordering::Relaxed), 5);
    }

    /// Verifies that `send_message` results in non-zero `queued_message_size`.
    #[tokio::test]
    async fn test_send_message_queues_data() {
        let cluster = Cluster::new(test_config(), None);
        let msg = Message::new(SUBMIT_JOB, Priority::Medium, "test_source");
        cluster.send_message(msg).await;

        assert!(cluster.queued_message_size.load(Ordering::Relaxed) > 0);
    }

    /// Verifies that calling `stop()` sets `running` to false.
    #[tokio::test]
    async fn test_stop_sets_running_false() {
        let cluster = Cluster::new(test_config(), None);
        assert!(cluster.running.load(Ordering::Relaxed));
        cluster.stop();
        assert!(!cluster.running.load(Ordering::Relaxed));
    }

    // -----------------------------------------------------------------------
    // test_queueMessage (comprehensive)
    // -----------------------------------------------------------------------

    /// Verifies comprehensive queue insertion: multiple sources and priorities, correct dequeuing order.
    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn test_queue_message_comprehensive() {
        let cluster = Cluster::new(test_config(), None);
        cluster.stop(); // stop scheduler so messages stay in queue

        // Check that source doesn't exist yet in Highest
        {
            let map = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            assert!(
                !map.contains_key("s1"),
                "s1 should not exist yet in Highest"
            );
        }

        let s1_d1 = vec![1u8, 2, 3];
        cluster
            .queue_message("s1".into(), s1_d1.clone(), Priority::Highest)
            .await;

        let s2_d1 = vec![4u8, 5, 6];
        cluster
            .queue_message("s2".into(), s2_d1.clone(), Priority::Lowest)
            .await;

        let s3_d1 = vec![7u8, 8, 9];
        cluster
            .queue_message("s3".into(), s3_d1.clone(), Priority::Lowest)
            .await;

        // s1 should only exist in Highest
        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            let m = cluster
                .queue
                .get(&(Priority::Medium as u8))
                .unwrap()
                .read()
                .await;
            let l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .read()
                .await;
            assert!(h.contains_key("s1"));
            assert!(!m.contains_key("s1"));
            assert!(!l.contains_key("s1"));
        }

        // s2 should only exist in Lowest
        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            let m = cluster
                .queue
                .get(&(Priority::Medium as u8))
                .unwrap()
                .read()
                .await;
            let l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .read()
                .await;
            assert!(!h.contains_key("s2"));
            assert!(!m.contains_key("s2"));
            assert!(l.contains_key("s2"));
        }

        // s3 should only exist in Lowest
        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            let l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .read()
                .await;
            assert!(!h.contains_key("s3"));
            assert!(l.contains_key("s3"));
        }

        // Check s1 has 1 item
        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            assert_eq!(h.get("s1").unwrap().len(), 1);
            assert_eq!(h.get("s1").unwrap().front().unwrap(), &s1_d1);
        }

        // Check s2 has 1 item
        {
            let l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .read()
                .await;
            assert_eq!(l.get("s2").unwrap().len(), 1);
            assert_eq!(l.get("s2").unwrap().front().unwrap(), &s2_d1);
        }

        // Queue more to s1
        let s1_d2 = vec![10u8, 11, 12];
        cluster
            .queue_message("s1".into(), s1_d2.clone(), Priority::Highest)
            .await;
        let s1_d3 = vec![13u8, 14, 15];
        cluster
            .queue_message("s1".into(), s1_d3.clone(), Priority::Highest)
            .await;

        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            assert_eq!(h.get("s1").unwrap().len(), 3);
        }

        // Dequeue from s1 and verify FIFO order
        {
            let mut h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .write()
                .await;
            let q = h.get_mut("s1").unwrap();
            assert_eq!(q.pop_front().unwrap(), s1_d1);
            assert_eq!(q.pop_front().unwrap(), s1_d2);
            assert_eq!(q.pop_front().unwrap(), s1_d3);
            assert!(q.is_empty());
        }

        // Queue more to s2 and s3, dequeue and verify
        let s2_d2 = vec![16u8, 17];
        cluster
            .queue_message("s2".into(), s2_d2.clone(), Priority::Lowest)
            .await;
        let s2_d3 = vec![18u8, 19];
        cluster
            .queue_message("s2".into(), s2_d3.clone(), Priority::Lowest)
            .await;
        let s3_d2 = vec![20u8, 21];
        cluster
            .queue_message("s3".into(), s3_d2.clone(), Priority::Lowest)
            .await;
        let s3_d3 = vec![22u8, 23];
        cluster
            .queue_message("s3".into(), s3_d3.clone(), Priority::Lowest)
            .await;

        {
            let mut l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .write()
                .await;
            let q2 = l.get_mut("s2").unwrap();
            assert_eq!(q2.len(), 3);
            assert_eq!(q2.pop_front().unwrap(), s2_d1);
            assert_eq!(q2.pop_front().unwrap(), s2_d2);
            assert_eq!(q2.pop_front().unwrap(), s2_d3);
            assert!(q2.is_empty());

            let q3 = l.get_mut("s3").unwrap();
            assert_eq!(q3.len(), 3);
            assert_eq!(q3.pop_front().unwrap(), s3_d1);
            assert_eq!(q3.pop_front().unwrap(), s3_d2);
            assert_eq!(q3.pop_front().unwrap(), s3_d3);
            assert!(q3.is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // test_pruneSources
    // Uses prune_once() directly instead of waiting for background task timer.
    // -----------------------------------------------------------------------

    /// Verifies that `prune_once` removes sources with empty queues while retaining sources with data.
    #[tokio::test]
    async fn test_prune_sources() {
        let cluster = Cluster::new(test_config(), None);
        cluster.stop(); // don't start background tasks

        // Queue messages from 3 sources
        cluster
            .queue_message("s1".into(), vec![1, 2, 3], Priority::Highest)
            .await;
        cluster
            .queue_message("s2".into(), vec![4, 5, 6], Priority::Lowest)
            .await;
        cluster
            .queue_message("s3".into(), vec![7, 8, 9], Priority::Lowest)
            .await;

        // Prune: all sources have data -> nothing should be removed
        cluster.prune_once().await;

        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            assert!(h.contains_key("s1"), "s1 should still exist (has data)");
        }
        {
            let l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .read()
                .await;
            assert!(l.contains_key("s2"), "s2 should still exist (has data)");
            assert!(l.contains_key("s3"), "s3 should still exist (has data)");
        }

        // Drain s2 (remove its data)
        {
            let mut l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .write()
                .await;
            l.get_mut("s2").unwrap().pop_front();
        }

        // Prune: s2 is empty -> should be removed
        cluster.prune_once().await;

        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            assert!(h.contains_key("s1"));
        }
        {
            let l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .read()
                .await;
            assert!(!l.contains_key("s2"), "s2 should be pruned (empty)");
            assert!(l.contains_key("s3"));
        }

        // Drain s1 and s3
        {
            let mut h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .write()
                .await;
            h.get_mut("s1").unwrap().pop_front();
        }
        {
            let mut l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .write()
                .await;
            l.get_mut("s3").unwrap().pop_front();
        }

        // Prune: both are empty -> should be removed
        cluster.prune_once().await;

        {
            let h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .read()
                .await;
            assert!(!h.contains_key("s1"), "s1 should be pruned");
            assert!(h.is_empty());
        }
        {
            let l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .read()
                .await;
            assert!(!l.contains_key("s2"));
            assert!(!l.contains_key("s3"), "s3 should be pruned");
            assert!(l.is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // test_doesHigherPriorityDataExist
    // -----------------------------------------------------------------------

    /// Verifies that `has_higher_priority_data` correctly detects non-empty queues at priorities
    /// higher than the given level.
    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn test_has_higher_priority_data() {
        let cluster = Cluster::new(test_config(), None);
        cluster.stop();

        // No data -> no higher priority data at any level
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Highest as u8)
                .await
        );
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Lowest as u8)
                .await
        );

        // Queue data at Lowest only
        cluster
            .queue_message("s4".into(), vec![1], Priority::Lowest)
            .await;
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Highest as u8)
                .await
        );
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Medium as u8)
                .await
        );
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Lowest as u8)
                .await
        );

        // Queue data at Medium -- now when checking Lowest, there IS higher priority data
        cluster
            .queue_message("s3".into(), vec![2], Priority::Medium)
            .await;
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Highest as u8)
                .await
        );
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Medium as u8)
                .await
        );
        assert!(
            cluster
                .has_higher_priority_data(Priority::Lowest as u8)
                .await
        );

        // Queue data at Highest -- now Medium also sees higher priority data
        cluster
            .queue_message("s2".into(), vec![3], Priority::Highest)
            .await;
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Highest as u8)
                .await
        );
        assert!(
            cluster
                .has_higher_priority_data(Priority::Medium as u8)
                .await
        );
        assert!(
            cluster
                .has_higher_priority_data(Priority::Lowest as u8)
                .await
        );

        // Add more at Highest
        cluster
            .queue_message("s1".into(), vec![4], Priority::Highest)
            .await;
        cluster
            .queue_message("s0".into(), vec![5], Priority::Highest)
            .await;
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Highest as u8)
                .await
        );
        assert!(
            cluster
                .has_higher_priority_data(Priority::Medium as u8)
                .await
        );
        assert!(
            cluster
                .has_higher_priority_data(Priority::Lowest as u8)
                .await
        );

        // Clear all Highest data
        {
            let mut h = cluster
                .queue
                .get(&(Priority::Highest as u8))
                .unwrap()
                .write()
                .await;
            h.get_mut("s2").unwrap().pop_front();
            h.get_mut("s1").unwrap().pop_front();
            h.get_mut("s0").unwrap().pop_front();
        }
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Highest as u8)
                .await
        );
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Medium as u8)
                .await
        );
        assert!(
            cluster
                .has_higher_priority_data(Priority::Lowest as u8)
                .await
        );

        // Clear Medium and Lowest data
        {
            let mut m = cluster
                .queue
                .get(&(Priority::Medium as u8))
                .unwrap()
                .write()
                .await;
            m.get_mut("s3").unwrap().pop_front();
        }
        {
            let mut l = cluster
                .queue
                .get(&(Priority::Lowest as u8))
                .unwrap()
                .write()
                .await;
            l.get_mut("s4").unwrap().pop_front();
        }
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Highest as u8)
                .await
        );
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Medium as u8)
                .await
        );
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Lowest as u8)
                .await
        );

        // Testing a priority value beyond Lowest should return false
        assert!(
            !cluster
                .has_higher_priority_data(Priority::Lowest as u8 + 1)
                .await
        );
    }

    // -----------------------------------------------------------------------
    // run tests (scheduler sends messages in correct priority/source order)
    // -----------------------------------------------------------------------

    /// Verifies that the scheduler sends all Highest messages before Medium, Medium before Lowest,
    /// and round-robins within each priority tier.
    #[tokio::test]
    async fn test_scheduler_priority_and_round_robin_ordering() {
        let cluster = Cluster::new(test_config(), None);

        // Queue messages at various priorities from multiple sources
        // Highest: s1 (2 msgs), s2 (1 msg)
        let s1_d1 = vec![11u8];
        let s1_d2 = vec![12u8];
        let s2_d1 = vec![21u8];
        cluster
            .queue_message("s1".into(), s1_d1.clone(), Priority::Highest)
            .await;
        cluster
            .queue_message("s1".into(), s1_d2.clone(), Priority::Highest)
            .await;
        cluster
            .queue_message("s2".into(), s2_d1.clone(), Priority::Highest)
            .await;

        // Medium: s6 (3 msgs)
        let s6_d1 = vec![61u8];
        let s6_d2 = vec![62u8];
        let s6_d3 = vec![63u8];
        cluster
            .queue_message("s6".into(), s6_d1.clone(), Priority::Medium)
            .await;
        cluster
            .queue_message("s6".into(), s6_d2.clone(), Priority::Medium)
            .await;
        cluster
            .queue_message("s6".into(), s6_d3.clone(), Priority::Medium)
            .await;

        // Lowest: s3 (4 msgs), s4 (2 msgs), s5 (2 msgs)
        let s3_d1 = vec![31u8];
        let s3_d2 = vec![32u8];
        let s3_d3 = vec![33u8];
        let s3_d4 = vec![34u8];
        let s4_d1 = vec![41u8];
        let s4_d2 = vec![42u8];
        let s5_d1 = vec![51u8];
        let s5_d2 = vec![52u8];
        cluster
            .queue_message("s3".into(), s3_d1.clone(), Priority::Lowest)
            .await;
        cluster
            .queue_message("s3".into(), s3_d2.clone(), Priority::Lowest)
            .await;
        cluster
            .queue_message("s3".into(), s3_d3.clone(), Priority::Lowest)
            .await;
        cluster
            .queue_message("s3".into(), s3_d4.clone(), Priority::Lowest)
            .await;
        cluster
            .queue_message("s4".into(), s4_d1.clone(), Priority::Lowest)
            .await;
        cluster
            .queue_message("s4".into(), s4_d2.clone(), Priority::Lowest)
            .await;
        cluster
            .queue_message("s5".into(), s5_d1.clone(), Priority::Lowest)
            .await;
        cluster
            .queue_message("s5".into(), s5_d2.clone(), Priority::Lowest)
            .await;

        // Connect and start scheduler
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        cluster.set_connection(Some(tx)).await;
        cluster.start_tasks();

        // Collect all 14 messages
        let mut received = Vec::new();
        for _ in 0..14 {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(WsOutbound::Binary(data))) => received.push(data),
                _ => break,
            }
        }

        cluster.stop();

        assert_eq!(received.len(), 14, "should receive all 14 messages");

        // Verify priority ordering: all Highest before Medium, all Medium before Lowest
        // Within same priority, sources are round-robined
        // Highest messages (3 total: s1 has 2, s2 has 1)
        let highest: Vec<&Vec<u8>> = received[0..3].iter().collect();
        // All highest messages should be from s1 or s2
        for h in &highest {
            assert!(
                h == &&s1_d1 || h == &&s1_d2 || h == &&s2_d1,
                "first 3 should be highest priority"
            );
        }

        // Medium messages (3 total: s6 has 3)
        assert_eq!(received[3], s6_d1);
        assert_eq!(received[4], s6_d2);
        assert_eq!(received[5], s6_d3);

        // Lowest messages (8 total: s3/4/5 round-robined)
        let lowest = &received[6..14];
        // All lowest values should be from s3, s4, or s5
        let mut found_s3 = Vec::new();
        let mut found_s4 = Vec::new();
        let mut found_s5 = Vec::new();
        for l in lowest {
            if l == &s3_d1 || l == &s3_d2 || l == &s3_d3 || l == &s3_d4 {
                found_s3.push(l.clone());
            } else if l == &s4_d1 || l == &s4_d2 {
                found_s4.push(l.clone());
            } else if l == &s5_d1 || l == &s5_d2 {
                found_s5.push(l.clone());
            } else {
                eprintln!("WARNING: unexpected data in lowest priority: {l:?}");
            }
        }
        // Verify all messages from each source arrived in FIFO order
        assert_eq!(found_s3, vec![s3_d1, s3_d2, s3_d3, s3_d4]);
        assert_eq!(found_s4, vec![s4_d1, s4_d2]);
        assert_eq!(found_s5, vec![s5_d1, s5_d2]);
    }

    // -----------------------------------------------------------------------
    // waitForQueueDrain tests
    // Uses default settings: MAX_FILE_BUFFER_SIZE=50MB, MIN_FILE_BUFFER_SIZE=10MB,
    // CLIENT_TIMEOUT_SECONDS=30.
    // Tests that need timeouts use tokio::time::pause() for instant virtual time.
    // -----------------------------------------------------------------------

    /// Verifies that `wait_for_queue_drain` returns true immediately when the queue is empty.
    #[tokio::test]
    async fn test_wait_for_queue_drain_empty_queue() {
        let cluster = Cluster::new(test_config(), None);
        // Empty queue, waitForEmpty=false -> true
        assert!(cluster.wait_for_queue_drain(false).await);
        // Empty queue, waitForEmpty=true -> true
        assert!(cluster.wait_for_queue_drain(true).await);
    }

    /// Verifies that a queue below the MAX threshold returns true without waiting.
    #[tokio::test]
    async fn test_wait_for_queue_drain_below_threshold() {
        let cluster = Cluster::new(test_config(), None);
        cluster.stop();

        // Queue 1KB (well below 50MB threshold)
        cluster
            .queue_message("test".into(), vec![0u8; 1024], Priority::Medium)
            .await;

        // waitForEmpty=false should return true (below MAX)
        assert!(cluster.wait_for_queue_drain(false).await);
    }

    /// Verifies that `waitForEmpty=true` times out when the queue has data and the scheduler is stopped.
    #[tokio::test]
    async fn test_wait_for_queue_drain_below_threshold_wait_for_empty_timeout() {
        tokio::time::pause(); // use virtual time for instant timeout

        let cluster = Cluster::new(test_config(), None);
        cluster.stop();

        // Queue 1KB
        cluster
            .queue_message("test".into(), vec![0u8; 1024], Priority::Medium)
            .await;

        // waitForEmpty=true should timeout (cluster stopped, can't drain)
        assert!(!cluster.wait_for_queue_drain(true).await);
    }

    /// Verifies that a queue at exactly `MAX_FILE_BUFFER_SIZE` still returns true (boundary is inclusive).
    #[tokio::test]
    async fn test_wait_for_queue_drain_at_threshold() {
        let cluster = Cluster::new(test_config(), None);
        cluster.stop();

        // Queue exactly MAX_FILE_BUFFER_SIZE bytes (50MB default)
        let max_buf = *MAX_FILE_BUFFER_SIZE as usize;
        let msg_size = 1024 * 100; // 100KB per message
        let num_msgs = max_buf / msg_size;
        for _ in 0..num_msgs {
            cluster
                .queue_message("test".into(), vec![0u8; msg_size], Priority::Medium)
                .await;
        }

        // At exactly threshold, should still return true (using <=)
        assert!(cluster.wait_for_queue_drain(false).await);
    }

    /// Verifies that a queue above `MAX_FILE_BUFFER_SIZE` times out when the scheduler is stopped.
    #[tokio::test]
    async fn test_wait_for_queue_drain_above_threshold_timeout() {
        tokio::time::pause(); // use virtual time

        let cluster = Cluster::new(test_config(), None);
        cluster.stop();

        // Queue more than MAX_FILE_BUFFER_SIZE
        let max_buf = *MAX_FILE_BUFFER_SIZE as usize;
        let msg_size = 1024 * 100;
        let num_msgs = (max_buf / msg_size) + 10;
        for _ in 0..num_msgs {
            cluster
                .queue_message("test".into(), vec![0u8; msg_size], Priority::Medium)
                .await;
        }

        // Queue is above threshold, cluster stopped -> should timeout
        assert!(!cluster.wait_for_queue_drain(false).await);
    }

    /// Verifies that `wait_for_queue_drain` returns true once the active scheduler drains the queue
    /// below the MAX threshold.
    #[tokio::test]
    async fn test_wait_for_queue_drain_success_when_draining() {
        let cluster = Cluster::new(test_config(), None);

        // Queue more than MAX_FILE_BUFFER_SIZE with small messages for fast drain
        let max_buf = *MAX_FILE_BUFFER_SIZE as usize;
        let msg_size = 1024 * 10; // 10KB per message
        let num_msgs = (max_buf / msg_size) + 5;
        for _ in 0..num_msgs {
            cluster
                .queue_message("test".into(), vec![0u8; msg_size], Priority::Medium)
                .await;
        }

        // Connect and start scheduler to drain
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        cluster.set_connection(Some(tx)).await;
        cluster.start_tasks();

        // Drain should succeed once scheduler sends enough messages
        let result = cluster.wait_for_queue_drain(false).await;
        assert!(result);

        cluster.stop();
        while rx.try_recv().is_ok() {}
    }

    /// Verifies that `wait_for_queue_drain` returns false when the cluster is stopped and the
    /// queue cannot drain.
    #[tokio::test]
    async fn test_wait_for_queue_drain_timeout_when_stopped() {
        tokio::time::pause(); // virtual time for instant timeout

        let cluster = Cluster::new(test_config(), None);
        cluster.stop();

        // Queue enough to exceed threshold
        let max_buf = *MAX_FILE_BUFFER_SIZE as usize;
        let msg_size = 1024 * 100;
        let num_msgs = (max_buf / msg_size) + 20;
        for _ in 0..num_msgs {
            cluster
                .queue_message("test".into(), vec![0u8; msg_size], Priority::Medium)
                .await;
        }

        // No draining -> timeout
        assert!(!cluster.wait_for_queue_drain(false).await);
    }

    /// Verifies that `waitForEmpty=true` times out when there are messages and no scheduler is running.
    #[tokio::test]
    async fn test_wait_for_queue_drain_wait_for_empty_timeout() {
        tokio::time::pause(); // virtual time

        let cluster = Cluster::new(test_config(), None);
        cluster.stop();

        // Queue messages but don't start scheduler
        for _ in 0..10 {
            cluster
                .queue_message("test".into(), vec![0u8; 1024], Priority::Medium)
                .await;
        }

        // waitForEmpty=true should timeout
        assert!(!cluster.wait_for_queue_drain(true).await);
    }

    /// Verifies that `wait_for_queue_drain` succeeds when multiple sources collectively exceed the
    /// threshold and the scheduler is draining them.
    #[tokio::test]
    async fn test_wait_for_queue_drain_multiple_sources() {
        let cluster = Cluster::new(test_config(), None);

        // Queue from multiple sources to exceed MAX_FILE_BUFFER_SIZE
        let max_buf = *MAX_FILE_BUFFER_SIZE as usize;
        let msg_size = 1024 * 10;
        let per_source = (max_buf / 3 / msg_size) + 5;
        for i in 0..per_source {
            cluster
                .queue_message("source1".into(), vec![i as u8; msg_size], Priority::Medium)
                .await;
            cluster
                .queue_message(
                    "source2".into(),
                    vec![(i + 1) as u8; msg_size],
                    Priority::Medium,
                )
                .await;
            cluster
                .queue_message(
                    "source3".into(),
                    vec![(i + 2) as u8; msg_size],
                    Priority::Medium,
                )
                .await;
        }

        // Connect and start
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        cluster.set_connection(Some(tx)).await;
        cluster.start_tasks();

        assert!(cluster.wait_for_queue_drain(false).await);

        cluster.stop();
        while rx.try_recv().is_ok() {}
    }

    /// Verifies that `wait_for_queue_drain` succeeds when messages are spread across all priority
    /// levels and the scheduler is actively sending.
    #[tokio::test]
    async fn test_wait_for_queue_drain_mixed_priorities() {
        let cluster = Cluster::new(test_config(), None);

        // Queue across different priorities to exceed MAX_FILE_BUFFER_SIZE
        let max_buf = *MAX_FILE_BUFFER_SIZE as usize;
        let msg_size = 1024 * 10;
        let num_msgs = (max_buf / 3 / msg_size) + 5;
        for i in 0..num_msgs {
            cluster
                .queue_message("test".into(), vec![i as u8; msg_size], Priority::Highest)
                .await;
            cluster
                .queue_message(
                    "test".into(),
                    vec![(i + 1) as u8; msg_size],
                    Priority::Medium,
                )
                .await;
            cluster
                .queue_message(
                    "test".into(),
                    vec![(i + 2) as u8; msg_size],
                    Priority::Lowest,
                )
                .await;
        }

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        cluster.set_connection(Some(tx)).await;
        cluster.start_tasks();

        assert!(cluster.wait_for_queue_drain(false).await);

        cluster.stop();
        while rx.try_recv().is_ok() {}
    }

    // -----------------------------------------------------------------------
    // FileDownload constructor checks
    // -----------------------------------------------------------------------

    /// Verifies that `new_file_download` initialises all `FileDownloadState` fields to their zero
    /// values and sets up the correct number of priority queues.
    #[test]
    fn test_file_download_constructor_fields() {
        let state = Arc::new(FileDownloadState::new());
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let uuid = "test-dl-uuid-constructor";
        let cluster =
            Cluster::new_file_download(test_config(), uuid.to_string(), state.clone(), None, lock);

        // Verify 3 priority levels
        assert_eq!(cluster.queue.len(), 3);
        assert_eq!(cluster.uuid(), Some(uuid));

        // Verify initial download state
        assert_eq!(state.file_size.load(Ordering::Relaxed), 0);
        assert!(!state.error.load(Ordering::Relaxed));
        assert!(!state.data_ready.load(Ordering::Relaxed));
        assert!(!state.received_data.load(Ordering::Relaxed));
        assert_eq!(state.received_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(state.sent_bytes.load(Ordering::Relaxed), 0);
        assert!(!state.client_paused.load(Ordering::Relaxed));
    }

    // -----------------------------------------------------------------------
    // FileUpload constructor checks
    // -----------------------------------------------------------------------

    /// Verifies that `new_file_upload` initialises all `FileUploadState` fields to their zero
    /// values and sets up the correct number of priority queues.
    #[test]
    fn test_file_upload_constructor_fields() {
        let state = Arc::new(FileUploadState::new());
        let uuid = "test-ul-uuid-constructor";
        let cluster =
            Cluster::new_file_upload(test_config(), uuid.to_string(), state.clone(), None);

        // Verify 3 priority levels
        assert_eq!(cluster.queue.len(), 3);
        assert_eq!(cluster.uuid(), Some(uuid));

        // Verify initial upload state
        assert!(!state.error.load(Ordering::Relaxed));
        assert!(!state.data_ready.load(Ordering::Relaxed));
        assert!(!state.received_data.load(Ordering::Relaxed));
        assert!(!state.complete.load(Ordering::Relaxed));
    }
}
