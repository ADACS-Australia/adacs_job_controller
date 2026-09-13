pub mod uuid;

/// Build the job-source key used as the wire message source for job submissions.
///
/// The key is the concatenation of the numeric job id and the target cluster name,
/// e.g. `"42_mycluster"`. It is shared by the HTTP job handlers and the cluster
/// resend path so the format stays consistent across both.
pub fn job_source_key(job_id: impl std::fmt::Display, cluster: impl std::fmt::Display) -> String {
    format!("{job_id}_{cluster}")
}
