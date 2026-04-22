use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use sea_orm::sea_query::{Condition, Expr, Func, Query};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::app::AppState;
use crate::db::entities::{job, job_history};
use crate::http::auth::{AuthResult, get_applications};
use crate::http::utils::{parse_csv_u64, parse_job_steps};
use crate::protocol::constants::*;
use crate::protocol::message::Message;
use crate::protocol::types::{JobStatus, Priority};

// ---- Request/Response types ----

#[derive(Debug, serde::Deserialize)]
pub struct CreateJobRequest {
    pub cluster: String,
    pub parameters: String,
    pub bundle: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct JobIdRequest {
    #[serde(rename = "jobId")]
    pub job_id: u64,
}

#[derive(Debug, serde::Deserialize)]
pub struct JobQueryParams {
    #[serde(rename = "jobIds")]
    pub job_ids: Option<String>,
    #[serde(rename = "startTimeGt")]
    pub start_time_gt: Option<i64>,
    #[serde(rename = "startTimeLt")]
    pub start_time_lt: Option<i64>,
    #[serde(rename = "endTimeGt")]
    pub end_time_gt: Option<i64>,
    #[serde(rename = "endTimeLt")]
    pub end_time_lt: Option<i64>,
    #[serde(rename = "jobSteps")]
    pub job_steps: Option<String>,
}

// ---- POST /job/apiv1/job/ ----

pub async fn create_job(
    auth: AuthResult,
    State(state): State<AppState>,
    Json(body): Json<CreateJobRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !auth.secret.clusters.contains(&body.cluster) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Application {} does not have access to cluster {}",
                auth.secret.name, body.cluster
            ),
        ));
    }

    let cluster = state
        .cluster_manager
        .get_cluster_by_name(&body.cluster)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid cluster".to_string()))?;

    let user_id = auth
        .payload
        .get("userId")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let job_row = job::ActiveModel {
        user: Set(user_id),
        parameters: Set(body.parameters.clone()),
        cluster: Set(body.cluster.clone()),
        bundle: Set(body.bundle.clone()),
        application: Set(auth.secret.name.clone()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

    let job_id = job_row.id;

    job_history::ActiveModel {
        job_id: Set(job_id),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        what: Set(SYSTEM_SOURCE.to_string()),
        state: Set(JobStatus::Pending as i32),
        details: Set("Job pending".to_string()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|_| (StatusCode::BAD_REQUEST, "Bad request".to_string()))?;

    if cluster.is_online() {
        let source = format!("{job_id}_{}", body.cluster);
        let mut msg = Message::new(SUBMIT_JOB, Priority::Medium, &source);
        msg.push_uint(job_id as u32);
        msg.push_string(&body.bundle);
        msg.push_string(&body.parameters);
        cluster.send_message(msg);

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
    }

    Ok(Json(serde_json::json!({ "jobId": job_id })))
}

// ---- GET /job/apiv1/job/ ----

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

    // Build the main query, applying all filters as database-level subqueries
    let mut job_query = job::Entity::find().filter(job::Column::Application.is_in(applications));

    // Filter by explicit job_ids
    if let Some(ref ids_str) = params.job_ids {
        let ids = parse_csv_u64(ids_str);
        if !ids.is_empty() {
            let ids_i64: Vec<i64> = ids.iter().map(|&id| id as i64).collect();
            job_query = job_query.filter(job::Column::Id.is_in(ids_i64));
        }
    }

    // start_time_gt: job must have a SYSTEM_SOURCE history entry where MIN(timestamp) >= cutoff
    if let Some(ts) = params.start_time_gt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = Query::select()
            .column(job_history::Column::JobId)
            .from(job_history::Entity)
            .and_where(Expr::col(job_history::Column::What).eq(SYSTEM_SOURCE))
            .group_by_col(job_history::Column::JobId)
            .and_having(
                Expr::expr(Func::min(Expr::col(job_history::Column::Timestamp))).gte(cutoff),
            )
            .to_owned();
        job_query = job_query.filter(job::Column::Id.in_subquery(subq));
    }

    // start_time_lt: job must have a SYSTEM_SOURCE history entry where MIN(timestamp) <= cutoff
    if let Some(ts) = params.start_time_lt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = Query::select()
            .column(job_history::Column::JobId)
            .from(job_history::Entity)
            .and_where(Expr::col(job_history::Column::What).eq(SYSTEM_SOURCE))
            .group_by_col(job_history::Column::JobId)
            .and_having(
                Expr::expr(Func::min(Expr::col(job_history::Column::Timestamp))).lte(cutoff),
            )
            .to_owned();
        job_query = job_query.filter(job::Column::Id.in_subquery(subq));
    }

    // end_time_gt: job must have a completion history entry with timestamp >= cutoff
    if let Some(ts) = params.end_time_gt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = Query::select()
            .distinct()
            .column(job_history::Column::JobId)
            .from(job_history::Entity)
            .and_where(Expr::col(job_history::Column::What).eq(JOB_COMPLETION_SOURCE))
            .and_where(Expr::col(job_history::Column::Timestamp).gte(cutoff))
            .to_owned();
        job_query = job_query.filter(job::Column::Id.in_subquery(subq));
    }

    // end_time_lt: job must have a completion history entry with timestamp <= cutoff
    if let Some(ts) = params.end_time_lt
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        let cutoff = dt.naive_utc();
        let subq = Query::select()
            .distinct()
            .column(job_history::Column::JobId)
            .from(job_history::Entity)
            .and_where(Expr::col(job_history::Column::What).eq(JOB_COMPLETION_SOURCE))
            .and_where(Expr::col(job_history::Column::Timestamp).lte(cutoff))
            .to_owned();
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
                        .add(job_history::Column::State.eq(*sv as i32)),
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

    // Fetch histories for filtered jobs, most recent first
    let histories = job_history::Entity::find()
        .filter(job_history::Column::JobId.is_in(filtered_ids))
        .order_by_desc(job_history::Column::Timestamp)
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

        result.as_array_mut().unwrap().push(serde_json::json!({
            "id": j.id,
            "user": j.user,
            "parameters": j.parameters,
            "cluster": j.cluster,
            "bundle": j.bundle,
            "history": job_histories,
        }));
    }

    Ok(Json(result))
}

// ---- PATCH /job/apiv1/job/ (Cancel) ----

pub async fn cancel_job(
    auth: AuthResult,
    State(state): State<AppState>,
    Json(body): Json<JobIdRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = get_job_with_access_check(&state, &auth, body.job_id).await?;

    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(body.job_id as i64))
        .order_by_desc(job_history::Column::Timestamp)
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

    if current_state == JobStatus::Pending as i32 {
        job_history::ActiveModel {
            job_id: Set(body.job_id as i64),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            what: Set(SYSTEM_SOURCE.to_string()),
            state: Set(JobStatus::Cancelled as i32),
            details: Set("Job cancelled".to_string()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;
    } else {
        job_history::ActiveModel {
            job_id: Set(body.job_id as i64),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            what: Set(SYSTEM_SOURCE.to_string()),
            state: Set(JobStatus::Cancelling as i32),
            details: Set("Job cancelling".to_string()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

        if cluster_obj.is_online() {
            let source = format!("{}_{}", body.job_id, job.cluster);
            let mut msg = Message::new(CANCEL_JOB, Priority::Medium, &source);
            msg.push_uint(body.job_id as u32);
            cluster_obj.send_message(msg);
        }
    }

    Ok(Json(serde_json::json!({ "cancelled": body.job_id })))
}

// ---- DELETE /job/apiv1/job/ (Delete) ----

pub async fn delete_job(
    auth: AuthResult,
    State(state): State<AppState>,
    Json(body): Json<JobIdRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = get_job_with_access_check(&state, &auth, body.job_id).await?;

    let latest = job_history::Entity::find()
        .filter(job_history::Column::JobId.eq(body.job_id as i64))
        .order_by_desc(job_history::Column::Timestamp)
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

    if current_state == JobStatus::Pending as i32 {
        job_history::ActiveModel {
            job_id: Set(body.job_id as i64),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            what: Set(SYSTEM_SOURCE.to_string()),
            state: Set(JobStatus::Deleted as i32),
            details: Set("Job deleted".to_string()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;
    } else {
        job_history::ActiveModel {
            job_id: Set(body.job_id as i64),
            timestamp: Set(chrono::Utc::now().naive_utc()),
            what: Set(SYSTEM_SOURCE.to_string()),
            state: Set(JobStatus::Deleting as i32),
            details: Set("Job deleting".to_string()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("DB error: {e}")))?;

        if cluster_obj.is_online() {
            let source = format!("{}_{}", body.job_id, job.cluster);
            let mut msg = Message::new(DELETE_JOB, Priority::Medium, &source);
            msg.push_uint(body.job_id as u32);
            cluster_obj.send_message(msg);
        }
    }

    Ok(Json(serde_json::json!({ "deleted": body.job_id })))
}

/// Fetch a job by ID and verify the requester's application has access to its cluster.
/// Does NOT verify cluster manager existence — callers that send WS messages check that.
pub async fn get_job_with_access_check(
    state: &AppState,
    auth: &AuthResult,
    job_id: u64,
) -> Result<job::Model, (StatusCode, String)> {
    let j = job::Entity::find_by_id(job_id as i64)
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
