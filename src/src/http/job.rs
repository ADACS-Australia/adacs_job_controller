#![allow(clippy::pedantic)]
use std::collections::HashMap;
use std::sync::Arc;

use crate::cluster::traits::ClusterTrait;
use crate::http::utils::LenientJson;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use sea_orm::sea_query::{Condition, Expr, Func, Query, SelectStatement};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::app::AppState;
use crate::db::entities::{job, job_history};
use crate::http::auth::{AuthResult, get_applications};
use crate::http::utils::{parse_csv_u64, parse_job_steps};
use crate::protocol::constants::{
    CANCEL_JOB, DELETE_JOB, JOB_COMPLETION_SOURCE, SUBMIT_JOB, SYSTEM_SOURCE,
};
use crate::protocol::message::Message;
use crate::protocol::types::{JobStatus, Priority};

// ---- Request/Response types ----

/// JSON body for `POST /job/apiv1/job/` — submit a new job to a cluster.
#[derive(Debug, serde::Deserialize)]
pub struct CreateJobRequest {
    /// Target cluster name (must match a configured cluster).
    pub cluster: String,
    /// Opaque job parameters passed to the cluster scheduler.
    pub parameters: String,
    /// Bundle identifier or hash for the job's executable bundle.
    pub bundle: String,
}

/// JSON body for job cancel/delete endpoints that accept a single job ID.
#[derive(Debug, serde::Deserialize)]
pub struct JobIdRequest {
    /// Internal job ID to cancel or delete.
    #[serde(rename = "jobId")]
    pub job_id: u64,
}

/// Query parameters for `GET /job/apiv1/job/` — filter the job list.
#[derive(Debug, serde::Deserialize)]
pub struct JobQueryParams {
    /// Comma-separated list of job IDs to restrict results to.
    #[serde(rename = "jobIds")]
    pub job_ids: Option<String>,
    /// Return jobs with `start_time` strictly greater than this Unix timestamp.
    #[serde(rename = "startTimeGt")]
    pub start_time_gt: Option<i64>,
    /// Return jobs with `start_time` strictly less than this Unix timestamp.
    #[serde(rename = "startTimeLt")]
    pub start_time_lt: Option<i64>,
    /// Return jobs with `end_time` strictly greater than this Unix timestamp.
    #[serde(rename = "endTimeGt")]
    pub end_time_gt: Option<i64>,
    /// Return jobs with `end_time` strictly less than this Unix timestamp.
    #[serde(rename = "endTimeLt")]
    pub end_time_lt: Option<i64>,
    /// CSV-encoded job-step filter: `what1,state1,what2,state2,...`.
    #[serde(rename = "jobSteps")]
    pub job_steps: Option<String>,
}

// ---- POST /job/apiv1/job/ ----

/// Create a new job on a remote cluster.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Authorization fails
/// - Application lacks access to the specified cluster
/// - Cluster is not found or offline
/// - Database operation fails
/// - Job submission message send fails
pub async fn create_job(
    auth: AuthResult,
    State(state): State<AppState>,
    // TODO: Content-Type tolerance - remove when client sends proper headers
    LenientJson(body): LenientJson<CreateJobRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    const MAX_PARAMETERS_LEN: usize = 100_000;
    const MAX_BUNDLE_LEN: usize = 10_000;

    tracing::debug!(
        "HTTP: Create job request received for cluster '{}'",
        body.cluster
    );
    tracing::trace!(
        "HTTP: Job parameters length: {}, bundle length: {}",
        body.parameters.len(),
        body.bundle.len()
    );

    if body.parameters.len() > MAX_PARAMETERS_LEN {
        tracing::warn!(
            "HTTP: Job creation rejected - parameters too long ({} > {})",
            body.parameters.len(),
            MAX_PARAMETERS_LEN
        );
        return Err((StatusCode::BAD_REQUEST, "parameters too long".to_string()));
    }
    if body.bundle.len() > MAX_BUNDLE_LEN {
        tracing::warn!(
            "HTTP: Job creation rejected - bundle too long ({} > {})",
            body.bundle.len(),
            MAX_BUNDLE_LEN
        );
        return Err((StatusCode::BAD_REQUEST, "bundle too long".to_string()));
    }

    if !auth.secret.clusters.contains(&body.cluster) {
        tracing::warn!(
            "HTTP: Job creation rejected - application '{}' lacks access to cluster '{}'",
            auth.secret.name,
            body.cluster
        );
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Application {} does not have access to cluster {}",
                auth.secret.name, body.cluster
            ),
        ));
    }

    tracing::trace!("HTTP: Fetching cluster '{}'", body.cluster);
    let cluster = state
        .cluster_manager
        .get_cluster_by_name(&body.cluster)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid cluster".to_string()))?;
    tracing::debug!(
        "HTTP: Cluster '{}' found, online status: {}",
        body.cluster,
        cluster.is_online()
    );

    let user_id = auth
        .payload
        .get("userId")
        .and_then(sea_orm::JsonValue::as_i64)
        .unwrap_or(0);
    tracing::trace!("HTTP: User ID extracted from token: {}", user_id);

    tracing::debug!("HTTP: Starting database transaction for job creation");
    let job_id: i64 = state
        .db
        .transaction::<_, _, sea_orm::DbErr>(|txn| {
            let parameters = body.parameters.clone();
            let cluster_name = body.cluster.clone();
            let bundle = body.bundle.clone();
            let app_name = auth.secret.name.clone();
            Box::pin(async move {
                tracing::trace!("HTTP: Inserting job record");
                let job_row = job::ActiveModel {
                    user: Set(user_id),
                    parameters: Set(parameters),
                    cluster: Set(cluster_name),
                    bundle: Set(bundle),
                    application: Set(app_name),
                    ..Default::default()
                }
                .insert(txn)
                .await?;

                let jid = job_row.id;
                tracing::trace!("HTTP: Job record inserted with ID {}", jid);

                tracing::trace!("HTTP: Inserting job history (Pending)");
                job_history::ActiveModel {
                    job_id: Set(jid),
                    timestamp: Set(chrono::Utc::now().naive_utc()),
                    what: Set(SYSTEM_SOURCE.to_string()),
                    state: Set(JobStatus::Pending as i32),
                    details: Set("Job pending".to_string()),
                    ..Default::default()
                }
                .insert(txn)
                .await?;

                Ok(jid)
            })
        })
        .await
        .map_err(|e| {
            tracing::error!("HTTP: Database transaction failed: {}", e);
            (StatusCode::BAD_REQUEST, format!("DB error: {e}"))
        })?;
    tracing::debug!("HTTP: Job {} created successfully", job_id);

    if cluster.is_online() {
        tracing::debug!(
            "HTTP: Cluster online - submitting job {} to cluster",
            job_id
        );
        let source = format!("{job_id}_{}", body.cluster);
        let mut msg = Message::new(SUBMIT_JOB, Priority::Medium, &source);
        msg.push_uint(u32::try_from(job_id).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("Job ID {job_id} exceeds maximum supported value"),
            )
        })?);
        msg.push_string(&body.bundle);
        msg.push_string(&body.parameters);
        tracing::trace!(
            "HTTP: Submit job message constructed (source: {}, priority: {:?})",
            source,
            msg.priority()
        );
        cluster.send_message(msg).await;
        tracing::trace!("HTTP: Submit job message queued");

        tracing::trace!("HTTP: Updating job history to Submitting");
        let _ = job_history::ActiveModel {
            job_id: Set(job_id),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            what: Set(SYSTEM_SOURCE.to_string()),
            state: Set(JobStatus::Submitting as i32),
            details: Set("Job submitting".to_string()),
            ..Default::default()
        }
        .insert(&state.db)
        .await;
    } else {
        tracing::warn!(
            "HTTP: Cluster '{}' is offline - job {} will be submitted when cluster reconnects",
            body.cluster,
            job_id
        );
    }

    tracing::info!(
        "HTTP: Job creation complete - ID: {}, cluster: {}",
        job_id,
        body.cluster
    );
    Ok(Json(serde_json::json!({ "jobId": job_id })))
}

// ---- GET /job/apiv1/job/ ----

/// Subquery of job IDs whose earliest SYSTEM_SOURCE history timestamp is
/// strictly greater (`gt = true`) or strictly less (`gt = false`) than `cutoff`.
fn min_timestamp_job_subquery(cutoff: chrono::NaiveDateTime, gt: bool) -> SelectStatement {
    let cmp = if gt {
        Expr::expr(Func::min(Expr::col(job_history::Column::Timestamp))).gt(cutoff)
    } else {
        Expr::expr(Func::min(Expr::col(job_history::Column::Timestamp))).lt(cutoff)
    };
    Query::select()
        .column(job_history::Column::JobId)
        .from(job_history::Entity)
        .and_where(Expr::col(job_history::Column::What).eq(SYSTEM_SOURCE))
        .group_by_col(job_history::Column::JobId)
        .and_having(cmp)
        .to_owned()
}

/// Subquery of job IDs that have a completion history entry whose timestamp is
/// strictly greater (`gt = true`) or strictly less (`gt = false`) than `cutoff`.
fn completion_timestamp_job_subquery(cutoff: chrono::NaiveDateTime, gt: bool) -> SelectStatement {
    let cmp = if gt {
        Expr::col(job_history::Column::Timestamp).gt(cutoff)
    } else {
        Expr::col(job_history::Column::Timestamp).lt(cutoff)
    };
    Query::select()
        .distinct()
        .column(job_history::Column::JobId)
        .from(job_history::Entity)
        .and_where(Expr::col(job_history::Column::What).eq(JOB_COMPLETION_SOURCE))
        .and_where(cmp)
        .to_owned()
}

/// Retrieve jobs filtered by various criteria.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Authorization fails
/// - Both startTimeGt and startTimeLt are provided
/// - Both endTimeGt and endTimeLt are provided
/// - Database query fails
pub async fn get_jobs(
    auth: AuthResult,
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<JobQueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let applications = get_applications(&auth.secret);

    if params.start_time_gt.is_some() && params.start_time_lt.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Can't have both startTimeGt and startTimeLt".to_string(),
        ));
    }
    if params.end_time_gt.is_some() && params.end_time_lt.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Can't have both endTimeGt and endTimeLt".to_string(),
        ));
    }

    // A present-but-invalid timestamp (e.g. microseconds instead of seconds) must not
    // silently drop the filter and widen the result set — return an empty list instead.
    for ts in [
        params.start_time_gt,
        params.start_time_lt,
        params.end_time_gt,
        params.end_time_lt,
    ]
    .into_iter()
    .flatten()
    {
        if chrono::DateTime::from_timestamp(ts, 0).is_none() {
            return Ok(Json(serde_json::json!([])));
        }
    }

    // Build the main query, applying all filters as database-level subqueries
    let mut job_query = job::Entity::find().filter(job::Column::Application.is_in(applications));

    // Filter by explicit job_ids
    if let Some(ref ids_str) = params.job_ids {
        let ids = parse_csv_u64(ids_str);
        if ids.is_empty() && !ids_str.trim().is_empty() {
            // A malformed filter (e.g. all non-numeric) must not widen the result set.
            return Ok(Json(serde_json::json!([])));
        }
        if !ids.is_empty() {
            let ids_i64: Vec<i64> = ids.iter().map(|&id| id.cast_signed()).collect();
            job_query = job_query.filter(job::Column::Id.is_in(ids_i64));
        }
    }

    // start_time_gt: job must have a SYSTEM_SOURCE history entry where MIN(timestamp) > cutoff
    if let Some(ts) = params.start_time_gt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = min_timestamp_job_subquery(cutoff, true);
        job_query = job_query.filter(job::Column::Id.in_subquery(subq));
    }

    // start_time_lt: job must have a SYSTEM_SOURCE history entry where MIN(timestamp) < cutoff
    if let Some(ts) = params.start_time_lt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = min_timestamp_job_subquery(cutoff, false);
        job_query = job_query.filter(job::Column::Id.in_subquery(subq));
    }

    // end_time_gt: job must have a completion history entry with timestamp > cutoff
    if let Some(ts) = params.end_time_gt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = completion_timestamp_job_subquery(cutoff, true);
        job_query = job_query.filter(job::Column::Id.in_subquery(subq));
    }

    // end_time_lt: job must have a completion history entry with timestamp < cutoff
    if let Some(ts) = params.end_time_lt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = completion_timestamp_job_subquery(cutoff, false);
        job_query = job_query.filter(job::Column::Id.in_subquery(subq));
    }

    // job_steps: job must have at least one history entry matching a (what, state) pair
    if let Some(ref steps_str) = params.job_steps {
        let steps = parse_job_steps(steps_str);
        if !steps.is_empty() {
            let mut step_cond = Condition::any();
            for (what, sv) in &steps {
                step_cond = step_cond.add(
                    Condition::all()
                        .add(job_history::Column::What.eq(what.clone()))
                        .add(job_history::Column::State.eq((*sv).cast_signed())),
                );
            }
            let subq = Query::select()
                .distinct()
                .column(job_history::Column::JobId)
                .from(job_history::Entity)
                .cond_where(step_cond)
                .to_owned();
            job_query = job_query.filter(job::Column::Id.in_subquery(subq));
        }
    }

    let filtered_jobs = job_query
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

    if filtered_jobs.is_empty() {
        return Ok(Json(serde_json::json!([])));
    }

    let filtered_ids: Vec<i64> = filtered_jobs.iter().map(|j| j.id).collect();

    // Fetch histories for filtered jobs, most recent first.
    // `id DESC` breaks ties when multiple rows are written in the same
    // microsecond (Pending + Submitting can be inserted back-to-back at job
    // creation time), matching check_resend_jobs.
    let histories = job_history::Entity::find()
        .filter(job_history::Column::JobId.is_in(filtered_ids))
        .order_by_desc(job_history::Column::Timestamp)
        .order_by_desc(job_history::Column::Id)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

    let mut histories_by_job: HashMap<i64, Vec<&job_history::Model>> = HashMap::new();
    for h in &histories {
        histories_by_job.entry(h.job_id).or_default().push(h);
    }

    let mut result = serde_json::json!([]);
    for j in &filtered_jobs {
        let job_histories: Vec<serde_json::Value> = histories_by_job
            .get(&j.id)
            .map(|hs| {
                hs.iter()
                    .map(|h| {
                        serde_json::json!({
                            "jobId": h.job_id,
                            "timestamp": h.timestamp.format("%Y-%m-%d %H:%M:%S%.6f UTC").to_string(),
                            "what": h.what,
                            "state": h.state,
                            "details": h.details,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        if let Some(arr) = result.as_array_mut() {
            arr.push(serde_json::json!({
                "id": j.id,
                "user": j.user,
                "parameters": j.parameters,
                "cluster": j.cluster,
                "bundle": j.bundle,
                "history": job_histories,
            }));
        }
    }

    Ok(Json(result))
}

// ---- PATCH /job/apiv1/job/ (Cancel) ----

/// Record a job-history state transition for a cancel/delete operation.
#[allow(clippy::too_many_arguments)]
async fn record_job_transition(
    state: &AppState,
    cluster_obj: &Arc<dyn ClusterTrait>,
    job: &job::Model,
    job_id: u64,
    current_state: i32,
    terminal_state: JobStatus,
    terminal_details: &str,
    intermediate_state: JobStatus,
    intermediate_details: &str,
    wire_msg_id: u32,
) -> Result<(), (StatusCode, String)> {
    let pending = current_state == JobStatus::Pending as i32;
    let (new_state, details) = if pending {
        (terminal_state as i32, terminal_details.to_string())
    } else {
        (intermediate_state as i32, intermediate_details.to_string())
    };

    job_history::ActiveModel {
        job_id: Set(job.id),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        what: Set(SYSTEM_SOURCE.to_string()),
        state: Set(new_state),
        details: Set(details),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

    if !pending && cluster_obj.is_online() {
        let source = format!("{}_{}", job_id, job.cluster);
        let mut msg = Message::new(wire_msg_id, Priority::Medium, &source);
        msg.push_uint(u32::try_from(job_id).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("Job ID {} exceeds maximum supported value", job_id),
            )
        })?);
        cluster_obj.send_message(msg).await;
    }

    Ok(())
}

/// Cancel a running job.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Authorization fails or job is not accessible
/// - Job is not found
/// - Job history is not found
/// - Job state does not allow cancellation
/// - Cluster is not found
/// - Database operation fails
pub async fn cancel_job(
    auth: AuthResult,
    State(state): State<AppState>,
    // TODO: Content-Type tolerance - remove when client sends proper headers
    LenientJson(body): LenientJson<JobIdRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = get_job_with_access_check(&state, &auth, body.job_id).await?;

    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job.id))
        .order_by_desc(job_history::Column::Timestamp)
        .order_by_desc(job_history::Column::Id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?
        .ok_or((StatusCode::BAD_REQUEST, "No history for job".to_string()))?;

    let current_state = latest.state;

    let invalid_states = [
        JobStatus::Cancelling as i32,
        JobStatus::Cancelled as i32,
        JobStatus::Deleting as i32,
        JobStatus::Deleted as i32,
        JobStatus::Error as i32,
        JobStatus::WallTimeExceeded as i32,
        JobStatus::OutOfMemory as i32,
        JobStatus::Completed as i32,
    ];

    if invalid_states.contains(&current_state) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Job is in invalid state".to_string(),
        ));
    }

    // Validate cluster exists (checked after state check so state errors take priority)
    let cluster_obj = state
        .cluster_manager
        .get_cluster_by_name(&job.cluster)
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Cluster for job did not exist".to_string(),
        ))?;

    record_job_transition(
        &state,
        &cluster_obj,
        &job,
        body.job_id,
        current_state,
        JobStatus::Cancelled,
        "Job cancelled",
        JobStatus::Cancelling,
        "Job cancelling",
        CANCEL_JOB,
    )
    .await?;

    Ok(Json(serde_json::json!({ "cancelled": body.job_id })))
}

// ---- DELETE /job/apiv1/job/ (Delete) ----

/// Delete a job record.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Authorization fails or job is not accessible
/// - Job is not found
/// - Job history is not found
/// - Job state does not allow deletion
/// - Cluster is not found
/// - Database operation fails
pub async fn delete_job(
    auth: AuthResult,
    State(state): State<AppState>,
    // TODO: Content-Type tolerance - remove when client sends proper headers
    LenientJson(body): LenientJson<JobIdRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = get_job_with_access_check(&state, &auth, body.job_id).await?;

    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(job.id))
        .order_by_desc(job_history::Column::Timestamp)
        .order_by_desc(job_history::Column::Id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?
        .ok_or((StatusCode::BAD_REQUEST, "No history for job".to_string()))?;

    let current_state = latest.state;

    let invalid_states = [
        JobStatus::Submitting as i32,
        JobStatus::Submitted as i32,
        JobStatus::Queued as i32,
        JobStatus::Running as i32,
        JobStatus::Cancelling as i32,
        JobStatus::Deleting as i32,
        JobStatus::Deleted as i32,
    ];

    if invalid_states.contains(&current_state) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Job is in invalid state".to_string(),
        ));
    }

    // Validate cluster exists (checked after state check so state errors take priority)
    let cluster_obj = state
        .cluster_manager
        .get_cluster_by_name(&job.cluster)
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Cluster for job did not exist".to_string(),
        ))?;

    record_job_transition(
        &state,
        &cluster_obj,
        &job,
        body.job_id,
        current_state,
        JobStatus::Deleted,
        "Job deleted",
        JobStatus::Deleting,
        "Job deleting",
        DELETE_JOB,
    )
    .await?;

    Ok(Json(serde_json::json!({ "deleted": body.job_id })))
}

/// Fetch a job by ID and verify the requester's application has access to its cluster.
/// Does NOT verify cluster manager existence — callers that send WS messages check that.
///
/// # Errors
///
/// Returns an HTTP error if:
/// - Database query fails
/// - Job is not found
/// - Application lacks access to the job's cluster
pub async fn get_job_with_access_check(
    state: &AppState,
    auth: &AuthResult,
    job_id: u64,
) -> Result<job::Model, (StatusCode, String)> {
    let j = job::Entity::find_by_id(job_id.cast_signed())
        .filter(job::Column::Application.is_in(get_applications(&auth.secret)))
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Job did not exist with the specified jobId".to_string(),
        ))?;

    if !auth.secret.clusters.contains(&j.cluster) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Application {} does not have access to cluster {}",
                auth.secret.name, j.cluster
            ),
        ));
    }

    Ok(j)
}
