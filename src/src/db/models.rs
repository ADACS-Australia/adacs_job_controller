#![allow(clippy::pedantic)]
use crate::protocol::message::Message;

// ============================================================
// Cluster-specific tables (accessed via binary message protocol)
// ============================================================

/// A cluster job record (wire format only — DB ops use `SeaORM` entities).
#[derive(Debug, Clone, Default)]
pub struct ClusterJob {
    /// Primary key in the cluster-side job table.
    pub id: i64,
    /// Controller-assigned job identifier shared with HTTP clients.
    pub job_id: i64,
    /// Scheduler-specific job ID on the remote cluster.
    pub scheduler_id: i64,
    /// Whether the job is currently being submitted to the scheduler.
    pub submitting: bool,
    /// Number of in-flight submit attempts (used for retry tracking).
    pub submitting_count: i32,
    /// Content hash of the job bundle payload.
    pub bundle_hash: String,
    /// Working directory on the cluster where the job runs.
    pub working_directory: String,
    /// Whether the job is actively running on the cluster.
    pub running: bool,
    /// Whether a delete/cancel operation is in progress.
    pub deleting: bool,
    /// Whether the job record has been marked deleted on the cluster.
    pub deleted: bool,
    /// Cluster name (populated locally; not serialized on the wire).
    #[allow(dead_code)]
    pub cluster: String,
}

impl ClusterJob {
    pub fn to_message(&self, msg: &mut Message) {
        msg.push_ulong(self.id.cast_unsigned());
        msg.push_ulong(self.job_id.cast_unsigned());
        msg.push_ulong(self.scheduler_id.cast_unsigned());
        msg.push_bool(self.submitting);
        msg.push_uint(self.submitting_count.cast_unsigned());
        msg.push_string(&self.bundle_hash);
        msg.push_string(&self.working_directory);
        msg.push_bool(self.running);
        msg.push_bool(self.deleting);
        msg.push_bool(self.deleted);
    }

    pub fn from_message(msg: &mut Message) -> Self {
        Self {
            id: msg.pop_ulong().cast_signed(),
            job_id: msg.pop_ulong().cast_signed(),
            scheduler_id: msg.pop_ulong().cast_signed(),
            submitting: msg.pop_bool(),
            submitting_count: msg.pop_uint().cast_signed(),
            bundle_hash: msg.pop_string(),
            working_directory: msg.pop_string(),
            running: msg.pop_bool(),
            deleting: msg.pop_bool(),
            deleted: msg.pop_bool(),
            cluster: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClusterJobStatus {
    pub id: i64,
    pub job_id: i64,
    pub what: String,
    pub state: i32,
}

impl ClusterJobStatus {
    pub fn to_message(&self, msg: &mut Message) {
        msg.push_ulong(self.id.cast_unsigned());
        msg.push_ulong(self.job_id.cast_unsigned());
        msg.push_string(&self.what);
        msg.push_uint(self.state.cast_unsigned());
    }

    pub fn from_message(msg: &mut Message) -> Self {
        Self {
            id: msg.pop_ulong().cast_signed(),
            job_id: msg.pop_ulong().cast_signed(),
            what: msg.pop_string(),
            state: msg.pop_uint().cast_signed(),
        }
    }
}

/// A cluster bundle job record (wire format only — DB ops use `SeaORM` entities).
///
/// Bundles group related job definitions; the cluster stores content and a hash
/// for deduplication. Serialized over the binary protocol via `to_message` /
/// `from_message`.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct BundleJob {
    pub id: i64,
    pub content: String,
    pub cluster: String,
    pub bundle_hash: String,
}

impl BundleJob {
    pub fn to_message(&self, msg: &mut Message) {
        msg.push_ulong(self.id.cast_unsigned());
        msg.push_string(&self.content);
    }

    pub fn from_message(msg: &mut Message) -> Self {
        Self {
            id: msg.pop_ulong().cast_signed(),
            content: msg.pop_string(),
            cluster: String::new(),
            bundle_hash: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_job_message_roundtrip() {
        let job = ClusterJob {
            id: 42,
            job_id: 100,
            scheduler_id: 7,
            submitting: true,
            submitting_count: 3,
            bundle_hash: "abc123".to_string(),
            working_directory: "/home/user/work".to_string(),
            running: true,
            deleting: false,
            deleted: false,
            cluster: "test-cluster".to_string(),
        };

        let mut msg = Message::new(5000, crate::protocol::types::Priority::Medium, "test");
        job.to_message(&mut msg);

        let mut read_msg = Message::from_bytes(msg.into_data());
        let restored = ClusterJob::from_message(&mut read_msg);

        assert_eq!(job.id, restored.id);
        assert_eq!(job.job_id, restored.job_id);
        assert_eq!(job.scheduler_id, restored.scheduler_id);
        assert_eq!(job.submitting, restored.submitting);
        assert_eq!(job.submitting_count, restored.submitting_count);
        assert_eq!(job.bundle_hash, restored.bundle_hash);
        assert_eq!(job.working_directory, restored.working_directory);
        assert_eq!(job.running, restored.running);
        assert_eq!(job.deleting, restored.deleting);
        assert_eq!(job.deleted, restored.deleted);
    }

    #[test]
    fn test_cluster_job_status_message_roundtrip() {
        let status = ClusterJobStatus {
            id: 10,
            job_id: 42,
            what: "scheduler_id".to_string(),
            state: 500,
        };

        let mut msg = Message::new(6000, crate::protocol::types::Priority::Medium, "test");
        status.to_message(&mut msg);

        let mut read_msg = Message::from_bytes(msg.into_data());
        let restored = ClusterJobStatus::from_message(&mut read_msg);

        assert_eq!(status.id, restored.id);
        assert_eq!(status.job_id, restored.job_id);
        assert_eq!(status.what, restored.what);
        assert_eq!(status.state, restored.state);
    }

    #[test]
    fn test_bundle_job_message_roundtrip() {
        let bundle = BundleJob {
            id: 5,
            content: r#"{"key": "value"}"#.to_string(),
            cluster: "cluster1".to_string(),
            bundle_hash: "hash123".to_string(),
        };

        let mut msg = Message::new(8000, crate::protocol::types::Priority::Medium, "test");
        bundle.to_message(&mut msg);

        let mut read_msg = Message::from_bytes(msg.into_data());
        let restored = BundleJob::from_message(&mut read_msg);

        assert_eq!(bundle.id, restored.id);
        assert_eq!(bundle.content, restored.content);
    }
}
