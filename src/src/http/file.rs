use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Json;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use crate::app::AppState;
use crate::config::settings;
use crate::db::entities::{file_download, file_list_cache, job, job_history};
use crate::http::auth::{AuthResult, get_applications};
use crate::http::utils::filter_files;
use crate::protocol::constants::{
    DOWNLOAD_FILE, FILE_LIST, FILE_UPLOAD_CHUNK, FILE_UPLOAD_COMPLETE, JOB_COMPLETION_SOURCE,
    RESUME_FILE_CHUNK_STREAM, UPLOAD_FILE,
};
use crate::protocol::message::Message;
use crate::protocol::types::{FileInfo, FileListState, Priority};
use crate::utils::general::generate_uuid;

// ---- Request/Response types ----

#[derive(Debug, serde::Deserialize)]
pub struct CreateFileDownloadRequest {
    #[serde(rename = "jobId")]
    pub job_id: Option<u64>,
    pub path: Option<String>,
    pub paths: Option<Vec<String>>,
    pub cluster: Option<String>,
    pub bundle: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct FileDownloadQuery {
    #[serde(rename = "fileId")]
    pub file_id: Option<String>,
    #[serde(rename = "forceDownload")]
    pub force_download: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct FileUploadQuery {
    #[serde(rename = "jobId")]
    pub job_id: Option<u64>,
    pub cluster: Option<String>,
    pub bundle: Option<String>,
    #[serde(rename = "targetPath")]
    pub target_path: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct FileListRequest {
    #[serde(rename = "jobId")]
    pub job_id: Option<u64>,
    pub recursive: bool,
    pub path: String,
    pub cluster: Option<String>,
    pub bundle: Option<String>,
}

// ---- POST /file/apiv1/file/ (Create file download records) ----

/// Create file download records in the database.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Authorization fails
/// - No path is provided in the request
/// - Cluster or bundle resolution fails
/// - Database operation fails
pub async fn create_file_download(
    auth: AuthResult,
    State(state): State<AppState>,
    Json(body): Json<CreateFileDownloadRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let applications = get_applications(&auth.secret);

    let has_paths = body.paths.is_some();
    let file_paths = if let Some(paths) = body.paths {
        paths
    } else if let Some(path) = body.path {
        vec![path]
    } else {
        return Err((StatusCode::BAD_REQUEST, "No path provided".to_string()));
    };

    if file_paths.is_empty() {
        return Ok(Json(serde_json::json!({ "fileIds": [] })));
    }

    let (s_cluster, s_bundle) = resolve_cluster_bundle(
        &state,
        &auth,
        &applications,
        body.job_id.unwrap_or(0),
        body.cluster.as_deref(),
        body.bundle.as_deref(),
    )
    .await?;

    let user_id = auth
        .payload
        .get("userId")
        .and_then(sea_orm::JsonValue::as_i64)
        .unwrap_or(0) as i32;

    let job_id = body.job_id.unwrap_or(0) as i32;

    let mut uuids = Vec::new();
    for path in &file_paths {
        let uuid = generate_uuid();
        file_download::ActiveModel {
            user: Set(user_id),
            job: Set(job_id),
            cluster: Set(s_cluster.clone()),
            bundle: Set(s_bundle.clone()),
            uuid: Set(uuid.clone()),
            path: Set(path.clone()),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

        uuids.push(uuid);
    }

    if has_paths {
        Ok(Json(serde_json::json!({ "fileIds": uuids })))
    } else {
        Ok(Json(serde_json::json!({ "fileId": uuids[0] })))
    }
}

// ---- GET /file/apiv1/file/ (Stream file download) ----

/// Stream a file download from a remote cluster.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - File ID parameter is missing
/// - File download record is not found
/// - Cluster is offline or unavailable
/// - File transmission fails
/// - Response header construction fails
#[allow(clippy::too_many_lines)]
pub async fn download_file(
    State(state): State<AppState>,
    Query(params): Query<FileDownloadQuery>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let original_uuid = params
        .file_id
        .filter(|s| !s.is_empty())
        .ok_or((StatusCode::BAD_REQUEST, "Bad Request".to_string()))?;

    let force_download = params.force_download.is_some();

    // Expire old download records
    let expiry_secs = *settings::FILE_DOWNLOAD_EXPIRY_TIME as i64;
    let expiry_dt = chrono::Utc::now().naive_utc()
        - chrono::Duration::try_seconds(expiry_secs).unwrap_or_default();
    let _ = file_download::Entity::delete_many()
        .filter(file_download::Column::Timestamp.lte(expiry_dt))
        .exec(&state.db)
        .await;

    // Fetch the download record
    let dl = file_download::Entity::find()
        .filter(file_download::Column::Uuid.eq(&original_uuid))
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?
        .ok_or((StatusCode::BAD_REQUEST, "Bad Request".to_string()))?;

    let s_cluster = dl.cluster;
    let s_bundle = dl.bundle;
    let s_file_path = dl.path.clone();
    let job_id = dl.job as u64;

    let cluster = state
        .cluster_manager
        .get_cluster_by_name(&s_cluster)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid cluster".to_string()))?;

    if !cluster.is_online() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Remote Cluster Offline".to_string(),
        ));
    }

    let uuid = generate_uuid();

    let fd_cluster = state
        .cluster_manager
        .create_file_download(&cluster, &uuid)
        .await;

    let mut msg = Message::new(DOWNLOAD_FILE, Priority::Highest, &uuid);
    msg.push_uint(job_id as u32);
    msg.push_string(&uuid);
    msg.push_string(&s_bundle);
    msg.push_string(&s_file_path);
    cluster.send_message(msg);

    let fd_state = state.cluster_manager.get_file_download(&uuid).ok_or((
        StatusCode::BAD_REQUEST,
        "File download session not found".to_string(),
    ))?;

    let timeout = std::time::Duration::from_secs(*settings::CLIENT_TIMEOUT_SECONDS);
    let ready = tokio::time::timeout(timeout, async {
        loop {
            if fd_state.data_ready.load(Ordering::Acquire) {
                return;
            }
            fd_state.data_notify.notified().await;
        }
    })
    .await;

    if ready.is_err() {
        fd_state.error.store(true, Ordering::Release);
        *fd_state.error_details.lock().await = "Client took too long to respond.".to_string();
    }

    if fd_state.error.load(Ordering::Acquire) {
        let details = fd_state.error_details.lock().await.clone();
        return Err((StatusCode::BAD_REQUEST, details));
    }

    if !fd_state.received_data.load(Ordering::Acquire) {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Remote Cluster Offline".to_string(),
        ));
    }

    let file_size = fd_state.file_size.load(Ordering::Acquire);

    let filename = std::path::Path::new(&s_file_path).file_name().map_or_else(
        || "download".to_string(),
        |n| n.to_string_lossy().to_string(),
    );

    let content_disposition = if force_download {
        format!("attachment; filename=\"{filename}\"")
    } else {
        format!("filename=\"{filename}\"")
    };

    let fd_state_stream = Arc::clone(&fd_state);
    let min_buffer = *settings::MIN_FILE_BUFFER_SIZE;
    let uuid_for_resume = uuid.clone();

    let stream = async_stream::stream! {
        let mut receiver = fd_state_stream.chunk_receiver.lock().await;
        let mut sent: u64 = 0;

        while sent < file_size && !fd_state_stream.error.load(Ordering::Acquire) {
            match tokio::time::timeout(
                std::time::Duration::from_secs(*settings::CLIENT_TIMEOUT_SECONDS),
                receiver.recv()
            ).await {
                Ok(Some(chunk)) => {
                    sent += chunk.len() as u64;
                    fd_state_stream.sent_bytes.store(sent, Ordering::Release);

                    // Backpressure: when buffer drains below MIN_FILE_BUFFER_SIZE, send RESUME
                    // to the cluster so it resumes sending chunks. Synchronized via
                    // compare_exchange to prevent races with the WS-side PAUSE logic.
                    let received_bytes = fd_state_stream.received_bytes.load(Ordering::Acquire);
                    if fd_state_stream.client_paused.load(Ordering::Acquire)
                        && received_bytes.saturating_sub(sent) < min_buffer
                        && fd_state_stream
                            .client_paused
                            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
                            .is_ok()
                    {
                                let resume_msg = Message::new(
                                    RESUME_FILE_CHUNK_STREAM,
                                    Priority::Highest,
                                    &uuid_for_resume,
                                );
                        fd_cluster.send_message(resume_msg);
                    }

                    yield Ok::<_, std::io::Error>(bytes::Bytes::from(chunk));
                }
                Ok(None) => break,
                Err(_) => {
                    yield Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Client took too long to respond",
                    ));
                    break;
                }
            }
        }
    };

    let body = Body::from_stream(stream);

    let response = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", file_size.to_string())
        .header("Content-Disposition", content_disposition)
        .body(body)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Response error: {e}"),
            )
        })?;

    Ok(response)
}

// ---- PUT /file/apiv1/file/upload/ (Stream file upload) ----

/// Handle a file upload to a remote cluster.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Target path parameter is missing
/// - Content-Length header is missing or invalid
/// - Authorization fails
/// - Cluster or bundle resolution fails
/// - Cluster is offline
/// - File upload session cannot be created
/// - Upload timeout occurs
/// - Chunk reception fails
#[allow(clippy::too_many_lines)]
pub async fn upload_file(
    auth: AuthResult,
    State(state): State<AppState>,
    Query(params): Query<FileUploadQuery>,
    request: axum::extract::Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use axum::body::to_bytes;

    let target_path = params.target_path.ok_or((
        StatusCode::BAD_REQUEST,
        "targetPath parameter is required".to_string(),
    ))?;

    let content_length: u64 = request
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Content-Length header is required".to_string(),
        ))?;

    let applications = get_applications(&auth.secret);

    let (s_cluster, s_bundle) = resolve_cluster_bundle(
        &state,
        &auth,
        &applications,
        params.job_id.unwrap_or(0),
        params.cluster.as_deref(),
        params.bundle.as_deref(),
    )
    .await?;

    let cluster = state
        .cluster_manager
        .get_cluster_by_name(&s_cluster)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid cluster".to_string()))?;

    if !cluster.is_online() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Remote cluster is offline".to_string(),
        ));
    }

    let uuid = generate_uuid();

    let upload_cluster = state
        .cluster_manager
        .create_file_upload(&cluster, &uuid)
        .await;

    let mut msg = Message::new(UPLOAD_FILE, Priority::Highest, &uuid);
    msg.push_uint(params.job_id.unwrap_or(0) as u32);
    msg.push_string(&s_bundle);
    msg.push_string(&target_path);
    msg.push_ulong(content_length);
    cluster.send_message(msg);

    let fu_state = state.cluster_manager.get_file_upload(&uuid).ok_or((
        StatusCode::BAD_REQUEST,
        "File upload session not found".to_string(),
    ))?;

    let timeout = std::time::Duration::from_secs(*settings::CLIENT_TIMEOUT_SECONDS);
    let ready = tokio::time::timeout(timeout, async {
        loop {
            if fu_state.data_ready.load(Ordering::Acquire) {
                return;
            }
            fu_state.data_notify.notified().await;
        }
    })
    .await;

    if ready.is_err() {
        fu_state.error.store(true, Ordering::Release);
        *fu_state.error_details.lock().await =
            "Remote cluster took too long to respond.".to_string();
    }

    if fu_state.error.load(Ordering::Acquire) {
        let details = fu_state.error_details.lock().await.clone();
        return Err((StatusCode::BAD_REQUEST, details));
    }

    let chunk_size = *settings::FILE_CHUNK_SIZE as usize;
    let mut total_read: u64 = 0;

    let body_bytes = to_bytes(request.into_body(), content_length as usize + 1)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Failed to read body: {e}")))?;

    while total_read < content_length {
        if !upload_cluster.wait_for_queue_drain(false).await {
            return Err((
                StatusCode::BAD_REQUEST,
                "Timeout waiting for queue to drain during upload".to_string(),
            ));
        }

        if fu_state.error.load(Ordering::Acquire) {
            let details = fu_state.error_details.lock().await.clone();
            return Err((StatusCode::BAD_REQUEST, details));
        }

        let remaining = (content_length - total_read) as usize;
        let this_chunk = remaining.min(chunk_size);
        let start = total_read as usize;
        let end = start + this_chunk;

        let mut chunk_msg = Message::new(FILE_UPLOAD_CHUNK, Priority::Lowest, &uuid);
        chunk_msg.push_bytes(&body_bytes[start..end]);
        upload_cluster.send_message(chunk_msg);

        total_read += this_chunk as u64;
    }

    if !upload_cluster.wait_for_queue_drain(true).await {
        return Err((
            StatusCode::BAD_REQUEST,
            "Timeout waiting for queue to empty before sending completion".to_string(),
        ));
    }

    if fu_state.error.load(Ordering::Acquire) {
        let details = fu_state.error_details.lock().await.clone();
        return Err((StatusCode::BAD_REQUEST, details));
    }

    let complete_msg = Message::new(FILE_UPLOAD_COMPLETE, Priority::Highest, &uuid);
    upload_cluster.send_message(complete_msg);

    let confirm = tokio::time::timeout(timeout, async {
        loop {
            if fu_state.complete.load(Ordering::Acquire) || fu_state.error.load(Ordering::Acquire) {
                return;
            }
            fu_state.data_notify.notified().await;
        }
    })
    .await;

    if confirm.is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Upload completion confirmation timeout".to_string(),
        ));
    }

    if fu_state.error.load(Ordering::Acquire) {
        let details = fu_state.error_details.lock().await.clone();
        return Err((StatusCode::BAD_REQUEST, details));
    }

    Ok(Json(serde_json::json!({
        "uploadId": uuid,
        "status": "completed",
    })))
}

// ---- PATCH /file/apiv1/file/ (List files) ----

/// List files on a remote cluster.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Authorization fails
/// - Cluster or bundle resolution fails
/// - Cluster is offline
/// - File list request times out
/// - Database operations fail
#[allow(clippy::too_many_lines)]
pub async fn list_files(
    auth: AuthResult,
    State(state): State<AppState>,
    Json(body): Json<FileListRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let applications = get_applications(&auth.secret);
    let job_id = body.job_id.unwrap_or(0);

    let (s_cluster, s_bundle) = if job_id != 0 {
        resolve_cluster_bundle_for_file_list(&state, &applications, &auth.secret.name, job_id)
            .await?
    } else {
        let cluster = body.cluster.ok_or((
            StatusCode::BAD_REQUEST,
            "The 'cluster' and 'bundle' parameters were not provided in the absence of 'jobId'"
                .to_string(),
        ))?;
        let bundle = body.bundle.ok_or((
            StatusCode::BAD_REQUEST,
            "The 'cluster' and 'bundle' parameters were not provided in the absence of 'jobId'"
                .to_string(),
        ))?;

        if !auth.secret.clusters.contains(&cluster) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Application {} does not have access to cluster {}",
                    auth.secret.name, cluster
                ),
            ));
        }

        (cluster, bundle)
    };

    let cluster = state
        .cluster_manager
        .get_cluster_by_name(&s_cluster)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid cluster".to_string()))?;

    if !cluster.is_online() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Remote Cluster Offline".to_string(),
        ));
    }

    // Check if job is complete (enables caching)
    let job_complete = if job_id != 0 {
        job_history::Entity::find()
            .filter(job_history::Column::JobId.eq(job_id as i64))
            .filter(job_history::Column::What.eq(JOB_COMPLETION_SOURCE))
            .one(&state.db)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?
            .is_some()
    } else {
        false
    };

    // Cache hit?
    if job_id != 0 && job_complete {
        let cached = file_list_cache::Entity::find()
            .filter(file_list_cache::Column::JobId.eq(job_id as i64))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

        if !cached.is_empty() {
            let files: Vec<FileInfo> = cached
                .iter()
                .map(|c| FileInfo {
                    file_name: c.path.clone(),
                    file_size: c.file_size as u64,
                    permissions: c.permissions as u32,
                    is_directory: c.is_dir,
                })
                .collect();

            let filtered = filter_files(&files, &body.path, body.recursive);
            return Ok(Json(serde_json::json!({ "files": filtered })));
        }
    }

    // Request file list from cluster via WebSocket
    let uuid = generate_uuid();
    let fl_state = Arc::new(tokio::sync::Mutex::new(FileListState::new()));
    state
        .file_list_map
        .insert(uuid.clone(), Arc::clone(&fl_state));

    let mut msg = Message::new(FILE_LIST, Priority::Highest, &uuid);
    msg.push_uint(job_id as u32);
    msg.push_string(&uuid);
    msg.push_string(&s_bundle);
    msg.push_string(&body.path);
    msg.push_bool(body.recursive);
    cluster.send_message(msg);

    let timeout = std::time::Duration::from_secs(*settings::CLIENT_TIMEOUT_SECONDS);
    let fl_state_clone = Arc::clone(&fl_state);
    let wait_result = tokio::time::timeout(timeout, async {
        loop {
            // Extract the notify Arc before releasing the lock, so we can await
            // without holding the mutex (holding would deadlock the writer).
            let notify = {
                let locked = fl_state_clone.lock().await;
                if locked.data_ready {
                    return;
                }
                Arc::clone(&locked.notify)
            };
            notify.notified().await;
        }
    })
    .await;

    if wait_result.is_err() {
        let mut locked = fl_state.lock().await;
        locked.error = true;
        locked.error_details = "Client took too long to respond.".to_string();
    }

    let locked = fl_state.lock().await;
    if locked.error {
        let details = locked.error_details.clone();
        drop(locked);
        state.file_list_map.remove(&uuid);
        return Err((StatusCode::BAD_REQUEST, details));
    }

    let files = locked.files.clone();
    drop(locked);

    let filtered = filter_files(&files, &body.path, body.recursive);

    // Cache if job is complete and this was a root recursive listing
    if job_complete && body.path.is_empty() && body.recursive {
        for file in &files {
            let _ = file_list_cache::ActiveModel {
                job_id: Set(job_id as i64),
                path: Set(file.file_name.clone()),
                is_dir: Set(file.is_directory),
                file_size: Set(file.file_size as i64),
                permissions: Set(file.permissions as i32),
                ..Default::default()
            }
            .insert(&state.db)
            .await;
        }
    } else if job_complete {
        // Background: cache the full root recursive listing for this job
        let state_clone = state.clone();
        let cluster_clone = Arc::clone(&cluster);
        let bundle_clone = s_bundle.clone();
        tokio::spawn(async move {
            let _ = spawn_background_cache(state_clone, cluster_clone, bundle_clone, job_id).await;
        });
    }

    state.file_list_map.remove(&uuid);

    Ok(Json(serde_json::json!({ "files": filtered })))
}

/// Spawn a background file-list request to populate the cache for a completed job.
async fn spawn_background_cache(
    state: AppState,
    cluster: Arc<dyn crate::cluster::traits::ClusterTrait>,
    bundle: String,
    job_id: u64,
) -> Result<(), String> {
    if !cluster.is_online() {
        return Err("Cluster offline".to_string());
    }

    let uuid = generate_uuid();
    let fl_state = Arc::new(tokio::sync::Mutex::new(FileListState::new()));
    state
        .file_list_map
        .insert(uuid.clone(), Arc::clone(&fl_state));

    let mut msg = Message::new(FILE_LIST, Priority::Highest, &uuid);
    msg.push_uint(job_id as u32);
    msg.push_string(&uuid);
    msg.push_string(&bundle);
    msg.push_string("");
    msg.push_bool(true);
    cluster.send_message(msg);

    let timeout = std::time::Duration::from_secs(*settings::CLIENT_TIMEOUT_SECONDS);
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
        for file in &locked.files {
            let _ = file_list_cache::ActiveModel {
                job_id: Set(job_id as i64),
                path: Set(file.file_name.clone()),
                is_dir: Set(file.is_directory),
                file_size: Set(file.file_size as i64),
                permissions: Set(file.permissions as i32),
                ..Default::default()
            }
            .insert(&state.db)
            .await;
        }
    }
    drop(locked);

    state.file_list_map.remove(&uuid);
    Ok(())
}

/// Resolve cluster and bundle from a job ID, or from explicit cluster+bundle params.
async fn resolve_cluster_bundle(
    state: &AppState,
    auth: &AuthResult,
    applications: &[String],
    job_id: u64,
    cluster_param: Option<&str>,
    bundle_param: Option<&str>,
) -> Result<(String, String), (StatusCode, String)> {
    if job_id != 0 {
        resolve_cluster_bundle_for_file_list(state, applications, &auth.secret.name, job_id).await
    } else {
        let cluster = cluster_param
            .filter(|s| !s.is_empty())
            .ok_or((StatusCode::BAD_REQUEST, "Bad request".to_string()))?;
        let bundle = bundle_param
            .filter(|s| !s.is_empty())
            .ok_or((StatusCode::BAD_REQUEST, "Bad request".to_string()))?;

        if !auth.secret.clusters.contains(&cluster.to_string()) {
            return Err((StatusCode::BAD_REQUEST, "Bad request".to_string()));
        }

        // Verify the cluster actually exists
        state
            .cluster_manager
            .get_cluster_by_name(cluster)
            .ok_or((StatusCode::BAD_REQUEST, "Bad request".to_string()))?;

        Ok((cluster.to_string(), bundle.to_string()))
    }
}

/// Look up cluster and bundle from a job record, verifying application access.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Database query fails
/// - Job is not found or inaccessible to the application
pub async fn resolve_cluster_bundle_for_file_list(
    state: &AppState,
    applications: &[String],
    app_name: &str,
    job_id: u64,
) -> Result<(String, String), (StatusCode, String)> {
    let j = job::Entity::find_by_id(job_id as i64)
        .filter(job::Column::Application.is_in(applications.to_vec()))
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?
        .ok_or((
            StatusCode::BAD_REQUEST,
            format!("Unable to find job with ID {job_id} for application {app_name}"),
        ))?;

    Ok((j.cluster, j.bundle))
}
