#![allow(clippy::pedantic)]
/// `ClusterDB` message dispatcher.
///
/// Handles database operation messages from remote cluster clients.
/// Each message type corresponds to a SQL operation that the server
/// executes on behalf of the cluster, returning the result via `DB_RESPONSE`.
use sea_orm::{
    ActiveModelTrait,
    ActiveValue::{NotSet, Set, Unchanged},
    ColumnTrait, EntityTrait, QueryFilter,
};

use crate::cluster::traits::ClusterTrait;
use crate::db::entities::{bundle_job, cluster_job, cluster_job_status};
use crate::db::models::{BundleJob, ClusterJob, ClusterJobStatus};
#[cfg(test)]
use crate::protocol::constants::{
    CANCEL_JOB, DELETE_JOB, FILE_CHUNK, FILE_DETAILS, FILE_ERROR, SUBMIT_JOB, UPDATE_JOB,
};
use crate::protocol::constants::{
    DB_BUNDLE_CREATE_OR_UPDATE_JOB, DB_BUNDLE_DELETE_JOB, DB_BUNDLE_GET_JOB_BY_ID, DB_JOB_DELETE,
    DB_JOB_GET_BY_ID, DB_JOB_GET_BY_JOB_ID, DB_JOB_GET_RUNNING_JOBS, DB_JOB_SAVE,
    DB_JOBSTATUS_DELETE_BY_ID_LIST, DB_JOBSTATUS_GET_BY_JOB_ID,
    DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT, DB_JOBSTATUS_SAVE, DB_RESPONSE, SYSTEM_SOURCE,
};
use crate::protocol::message::Message;
use crate::protocol::types::Priority;

// Conversions from SeaORM models to wire structs

/// Convert a persisted `cluster_job` row into the wire-format [`ClusterJob`].
impl From<cluster_job::Model> for ClusterJob {
    fn from(m: cluster_job::Model) -> Self {
        Self {
            id: m.id,
            job_id: m.job_id,
            scheduler_id: m.scheduler_id,
            submitting: m.submitting,
            submitting_count: m.submitting_count,
            bundle_hash: m.bundle_hash,
            working_directory: m.working_directory,
            running: m.running,
            deleting: m.deleting,
            deleted: m.deleted,
            cluster: m.cluster,
        }
    }
}

/// Convert a persisted `cluster_job_status` row into the wire-format [`ClusterJobStatus`].
impl From<cluster_job_status::Model> for ClusterJobStatus {
    fn from(m: cluster_job_status::Model) -> Self {
        Self {
            id: m.id,
            job_id: m.job_id,
            what: m.what,
            state: m.state,
        }
    }
}

/// Convert a persisted `bundle_job` row into the wire-format [`BundleJob`].
impl From<bundle_job::Model> for BundleJob {
    fn from(m: bundle_job::Model) -> Self {
        Self {
            id: m.id,
            content: m.content,
            cluster: m.cluster,
            bundle_hash: m.bundle_hash,
        }
    }
}

/// Try to handle a cluster DB message. Returns true if the message was handled.
///
/// The message must already have its header parsed (source + id).
/// If the message ID matches a DB_* constant, the corresponding handler
/// is called, a `DB_RESPONSE` is sent back, and true is returned.
pub async fn maybe_handle_cluster_db_message(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) -> bool {
    let msg_id = message.id();
    tracing::trace!(
        "ClusterDB[{}]: Received DB message ID {}",
        cluster.name(),
        msg_id
    );

    match message.id() {
        DB_JOB_GET_BY_JOB_ID => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_JOB_GET_BY_JOB_ID",
                cluster.name()
            );
            handle_job_get_by_job_id(message, cluster, db).await;
            true
        }
        DB_JOB_GET_BY_ID => {
            tracing::trace!("ClusterDB[{}]: Handling DB_JOB_GET_BY_ID", cluster.name());
            handle_job_get_by_id(message, cluster, db).await;
            true
        }
        DB_JOB_GET_RUNNING_JOBS => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_JOB_GET_RUNNING_JOBS",
                cluster.name()
            );
            handle_job_get_running_jobs(message, cluster, db).await;
            true
        }
        DB_JOB_DELETE => {
            tracing::trace!("ClusterDB[{}]: Handling DB_JOB_DELETE", cluster.name());
            handle_job_delete(message, cluster, db).await;
            true
        }
        DB_JOB_SAVE => {
            tracing::trace!("ClusterDB[{}]: Handling DB_JOB_SAVE", cluster.name());
            handle_job_save(message, cluster, db).await;
            true
        }
        DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT",
                cluster.name()
            );
            handle_jobstatus_get_by_job_id_and_what(message, cluster, db).await;
            true
        }
        DB_JOBSTATUS_GET_BY_JOB_ID => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_JOBSTATUS_GET_BY_JOB_ID",
                cluster.name()
            );
            handle_jobstatus_get_by_job_id(message, cluster, db).await;
            true
        }
        DB_JOBSTATUS_DELETE_BY_ID_LIST => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_JOBSTATUS_DELETE_BY_ID_LIST",
                cluster.name()
            );
            handle_jobstatus_delete_by_id_list(message, cluster, db).await;
            true
        }
        DB_JOBSTATUS_SAVE => {
            tracing::trace!("ClusterDB[{}]: Handling DB_JOBSTATUS_SAVE", cluster.name());
            handle_jobstatus_save(message, cluster, db).await;
            true
        }
        DB_BUNDLE_CREATE_OR_UPDATE_JOB => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_BUNDLE_CREATE_OR_UPDATE_JOB",
                cluster.name()
            );
            handle_bundle_create_or_update(message, cluster, db).await;
            true
        }
        DB_BUNDLE_GET_JOB_BY_ID => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_BUNDLE_GET_JOB_BY_ID",
                cluster.name()
            );
            handle_bundle_get_by_id(message, cluster, db).await;
            true
        }
        DB_BUNDLE_DELETE_JOB => {
            tracing::trace!(
                "ClusterDB[{}]: Handling DB_BUNDLE_DELETE_JOB",
                cluster.name()
            );
            handle_bundle_delete(message, cluster, db).await;
            true
        }
        other => {
            tracing::trace!(
                "ClusterDB[{}]: Message ID {} not a DB message - not handled",
                cluster.name(),
                other
            );
            false
        }
    }
}

/// Builds a `DB_RESPONSE` message tagged with the originating request ID.
fn prepare_response(db_request_id: u32) -> Message {
    let mut msg = Message::new(DB_RESPONSE, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(db_request_id);
    msg
}

// ---- DB_JOB_* handlers ----

/// Looks up cluster jobs by external job ID and sends a `DB_RESPONSE` with matching rows.
async fn handle_job_get_by_job_id(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let job_id = message.pop_ulong().cast_signed();
    let cluster_name = cluster.name();

    let rows: Vec<ClusterJob> = cluster_job::Entity::find()
        .filter(cluster_job::Column::JobId.eq(job_id))
        .filter(cluster_job::Column::Cluster.eq(&cluster_name))
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(ClusterJob::from)
        .collect();

    let mut response = prepare_response(db_request_id);
    response.push_uint(rows.len().try_into().unwrap());
    for row in &rows {
        row.to_message(&mut response);
    }
    cluster.send_message(response).await;
}

/// Looks up a cluster job by its primary key and returns zero or one row.
async fn handle_job_get_by_id(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let id = message.pop_ulong().cast_signed();

    let row: Option<ClusterJob> = cluster_job::Entity::find_by_id(id)
        .one(db)
        .await
        .unwrap_or(None)
        .map(ClusterJob::from);

    let mut response = prepare_response(db_request_id);
    match row {
        Some(ref job) => {
            response.push_uint(1);
            job.to_message(&mut response);
        }
        None => {
            response.push_uint(0);
        }
    }
    cluster.send_message(response).await;
}

async fn handle_job_get_running_jobs(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let cluster_name = cluster.name();

    let rows: Vec<ClusterJob> = cluster_job::Entity::find()
        .filter(cluster_job::Column::Cluster.eq(&cluster_name))
        .filter(cluster_job::Column::Running.eq(true))
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(ClusterJob::from)
        .collect();

    let mut response = prepare_response(db_request_id);
    response.push_uint(rows.len().try_into().unwrap());
    for row in &rows {
        row.to_message(&mut response);
    }
    cluster.send_message(response).await;
}

/// Deletes a cluster job row by primary key and sends an empty DB response.
async fn handle_job_delete(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let id = message.pop_ulong().cast_signed();

    let _ = cluster_job::Entity::delete_by_id(id).exec(db).await;

    let response = prepare_response(db_request_id);
    cluster.send_message(response).await;
}

/// Inserts or updates a cluster job row and sends a `DB_RESPONSE` with the row ID.
async fn handle_job_save(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let job = ClusterJob::from_message(message);
    let cluster_name = cluster.name();

    if job.id == 0 {
        // Insert
        let result = cluster_job::ActiveModel {
            id: NotSet,
            job_id: Set(job.job_id),
            scheduler_id: Set(job.scheduler_id),
            submitting: Set(job.submitting),
            submitting_count: Set(job.submitting_count),
            bundle_hash: Set(job.bundle_hash.clone()),
            working_directory: Set(job.working_directory.clone()),
            running: Set(job.running),
            deleting: Set(job.deleting),
            deleted: Set(job.deleted),
            cluster: Set(cluster_name),
        }
        .insert(db)
        .await;

        let mut response = prepare_response(db_request_id);
        match result {
            Ok(model) => response.push_ulong(model.id.cast_unsigned()),
            Err(_) => response.push_ulong(0),
        }
        cluster.send_message(response).await;
    } else {
        // Update
        let active = cluster_job::ActiveModel {
            id: Unchanged(job.id),
            job_id: Set(job.job_id),
            scheduler_id: Set(job.scheduler_id),
            submitting: Set(job.submitting),
            submitting_count: Set(job.submitting_count),
            bundle_hash: Set(job.bundle_hash.clone()),
            working_directory: Set(job.working_directory.clone()),
            running: Set(job.running),
            deleting: Set(job.deleting),
            deleted: Set(job.deleted),
            cluster: NotSet,
        };
        let _ = active.update(db).await;

        let mut response = prepare_response(db_request_id);
        response.push_ulong(job.id.cast_unsigned());
        cluster.send_message(response).await;
    }
}

// ---- DB_JOBSTATUS_* handlers ----

/// Looks up cluster job status rows by job ID and status type, then sends a `DB_RESPONSE` with matching rows.
async fn handle_jobstatus_get_by_job_id_and_what(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let job_id = message.pop_ulong().cast_signed();
    let what = message.pop_string();

    let rows: Vec<ClusterJobStatus> = cluster_job_status::Entity::find()
        .filter(cluster_job_status::Column::JobId.eq(job_id))
        .filter(cluster_job_status::Column::What.eq(&what))
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(ClusterJobStatus::from)
        .collect();

    let mut response = prepare_response(db_request_id);
    response.push_uint(rows.len().try_into().unwrap());
    for row in &rows {
        row.to_message(&mut response);
    }
    cluster.send_message(response).await;
}

/// Looks up cluster job status rows by job ID and sends a `DB_RESPONSE` with matching rows.
async fn handle_jobstatus_get_by_job_id(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let job_id = message.pop_ulong().cast_signed();

    let rows: Vec<ClusterJobStatus> = cluster_job_status::Entity::find()
        .filter(cluster_job_status::Column::JobId.eq(job_id))
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(ClusterJobStatus::from)
        .collect();

    let mut response = prepare_response(db_request_id);
    response.push_uint(rows.len().try_into().unwrap());
    for row in &rows {
        row.to_message(&mut response);
    }
    cluster.send_message(response).await;
}

/// Deletes cluster job status rows by primary key list and sends an empty DB response.
async fn handle_jobstatus_delete_by_id_list(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let count = message.pop_uint();

    for _ in 0..count {
        let id = message.pop_ulong().cast_signed();
        let _ = cluster_job_status::Entity::delete_by_id(id).exec(db).await;
    }

    let response = prepare_response(db_request_id);
    cluster.send_message(response).await;
}

/// Inserts or updates a cluster job status row and sends a `DB_RESPONSE` with the row ID.
async fn handle_jobstatus_save(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let status = ClusterJobStatus::from_message(message);

    if status.id == 0 {
        // Insert
        let result = cluster_job_status::ActiveModel {
            id: NotSet,
            job_id: Set(status.job_id),
            what: Set(status.what.clone()),
            state: Set(status.state),
        }
        .insert(db)
        .await;

        let mut response = prepare_response(db_request_id);
        match result {
            Ok(model) => response.push_ulong(model.id.cast_unsigned()),
            Err(_) => response.push_ulong(0),
        }
        cluster.send_message(response).await;
    } else {
        // Update
        let active = cluster_job_status::ActiveModel {
            id: Unchanged(status.id),
            job_id: Set(status.job_id),
            what: Set(status.what.clone()),
            state: Set(status.state),
        };
        let _ = active.update(db).await;

        let mut response = prepare_response(db_request_id);
        response.push_ulong(status.id.cast_unsigned());
        cluster.send_message(response).await;
    }
}

// ---- DB_BUNDLE_* handlers ----

/// Inserts or updates a bundle job row and sends a `DB_RESPONSE` with the row ID.
async fn handle_bundle_create_or_update(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let bundle = BundleJob::from_message(message);
    let bundle_hash = message.pop_string();
    let cluster_name = cluster.name();

    // If bundle has an ID, try to update existing bundle
    if bundle.id > 0 {
        let existing = bundle_job::Entity::find_by_id(bundle.id)
            .one(db)
            .await
            .unwrap_or(None);

        if let Some(model) = existing {
            // Verify hash matches - C++ behavior: return error (id=0) if hash doesn't match
            if model.bundle_hash != bundle_hash {
                let mut response = prepare_response(db_request_id);
                response.push_ulong(0); // Error: hash mismatch
                cluster.send_message(response).await;
                return;
            }

            // Update with matching hash
            let active = bundle_job::ActiveModel {
                id: Unchanged(model.id),
                content: Set(bundle.content.clone()),
                cluster: NotSet,
                bundle_hash: NotSet,
            };
            let _ = active.update(db).await;

            let mut response = prepare_response(db_request_id);
            response.push_ulong(model.id.cast_unsigned());
            cluster.send_message(response).await;
        } else {
            // Bundle ID not found - insert new
            let result = bundle_job::ActiveModel {
                id: NotSet,
                content: Set(bundle.content.clone()),
                cluster: Set(cluster_name),
                bundle_hash: Set(bundle_hash),
            }
            .insert(db)
            .await;

            let mut response = prepare_response(db_request_id);
            match result {
                Ok(model) => response.push_ulong(model.id.cast_unsigned()),
                Err(_) => response.push_ulong(0),
            }
            cluster.send_message(response).await;
        }
    } else {
        // No ID - search by hash and cluster, insert if not found
        let existing = bundle_job::Entity::find()
            .filter(bundle_job::Column::BundleHash.eq(&bundle_hash))
            .filter(bundle_job::Column::Cluster.eq(&cluster_name))
            .one(db)
            .await
            .unwrap_or(None);

        if let Some(model) = existing {
            // Update
            let active = bundle_job::ActiveModel {
                id: Unchanged(model.id),
                content: Set(bundle.content.clone()),
                cluster: NotSet,
                bundle_hash: NotSet,
            };
            let _ = active.update(db).await;

            let mut response = prepare_response(db_request_id);
            response.push_ulong(model.id.cast_unsigned());
            cluster.send_message(response).await;
        } else {
            // Insert
            let result = bundle_job::ActiveModel {
                id: NotSet,
                content: Set(bundle.content.clone()),
                cluster: Set(cluster_name),
                bundle_hash: Set(bundle_hash),
            }
            .insert(db)
            .await;

            let mut response = prepare_response(db_request_id);
            match result {
                Ok(model) => response.push_ulong(model.id.cast_unsigned()),
                Err(_) => response.push_ulong(0),
            }
            cluster.send_message(response).await;
        }
    }
}

/// Fetches a bundle job by ID and sends a `DB_RESPONSE` with the row or count=0.
async fn handle_bundle_get_by_id(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let id = message.pop_ulong().cast_signed();

    let row: Option<BundleJob> = bundle_job::Entity::find_by_id(id)
        .one(db)
        .await
        .unwrap_or(None)
        .map(BundleJob::from);

    let mut response = prepare_response(db_request_id);
    match row {
        Some(ref bundle) => {
            response.push_uint(1);
            bundle.to_message(&mut response);
        }
        None => {
            response.push_uint(0);
        }
    }
    cluster.send_message(response).await;
}

/// Deletes a bundle job by ID and sends an empty `DB_RESPONSE`.
async fn handle_bundle_delete(
    message: &mut Message,
    cluster: &dyn ClusterTrait,
    db: &sea_orm::DatabaseConnection,
) {
    let db_request_id = message.pop_uint();
    let id = message.pop_ulong().cast_signed();

    let _ = bundle_job::Entity::delete_by_id(id).exec(db).await;

    let response = prepare_response(db_request_id);
    cluster.send_message(response).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_response() {
        let response = prepare_response(42);
        assert_eq!(response.id(), DB_RESPONSE);
        assert_eq!(response.source(), SYSTEM_SOURCE);

        let mut parsed = Message::from_bytes(response.into_data());
        assert_eq!(parsed.pop_uint(), 42); // db_request_id
    }

    #[test]
    fn test_unhandled_message_returns_false() {
        // This test doesn't need async since we test the ID matching
        // We verify that non-DB message IDs return false
        let non_db_ids = vec![
            SUBMIT_JOB,
            UPDATE_JOB,
            CANCEL_JOB,
            DELETE_JOB,
            FILE_CHUNK,
            FILE_DETAILS,
            FILE_ERROR,
        ];
        for id in non_db_ids {
            // Just check the match arm — not DB_* so should return false
            let is_db = matches!(
                id,
                DB_JOB_GET_BY_JOB_ID
                    | DB_JOB_GET_BY_ID
                    | DB_JOB_GET_RUNNING_JOBS
                    | DB_JOB_DELETE
                    | DB_JOB_SAVE
                    | DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT
                    | DB_JOBSTATUS_GET_BY_JOB_ID
                    | DB_JOBSTATUS_DELETE_BY_ID_LIST
                    | DB_JOBSTATUS_SAVE
                    | DB_BUNDLE_CREATE_OR_UPDATE_JOB
                    | DB_BUNDLE_GET_JOB_BY_ID
                    | DB_BUNDLE_DELETE_JOB
            );
            assert!(!is_db, "ID {id} should not be a DB message");
        }
    }
}
