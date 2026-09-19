pub mod uuid;

/// Build the job-source key used as the wire message source for job submissions.
///
/// The key is the concatenation of the numeric job id and the target cluster name,
/// e.g. `"42_mycluster"`. It is shared by the HTTP job handlers and the cluster
/// resend path so the format stays consistent across both.
pub fn job_source_key(job_id: impl std::fmt::Display, cluster: impl std::fmt::Display) -> String {
    format!("{job_id}_{cluster}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_source_key_concatenates_id_and_cluster() {
        assert_eq!(job_source_key(42, "cluster"), "42_cluster");
        assert_eq!(job_source_key(0, "a"), "0_a");
    }

    #[test]
    fn job_source_key_preserves_large_id_and_special_chars_verbatim() {
        assert_eq!(
            job_source_key(9_007_199_254_740_993_i64, "my-cluster"),
            "9007199254740993_my-cluster"
        );
        assert_eq!(job_source_key(7, "cluster_alpha"), "7_cluster_alpha");
    }
}
