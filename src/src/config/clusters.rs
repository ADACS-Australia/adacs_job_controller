use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ClusterConfig {
    pub name: String,
    pub host: String,
    pub username: String,
    pub path: String,
    #[serde(default)]
    pub key: String,
    #[serde(default = "default_connection_type")]
    pub connection_type: String,
    #[serde(default)]
    pub keytab: String,
    #[serde(default)]
    pub kerberos_principal: String,
    #[serde(default)]
    pub ltk: Option<String>,
}

fn default_connection_type() -> String {
    "ssh".to_string()
}

/// Load cluster configurations from a JSON file.
pub fn load_cluster_configs(path: &Path) -> anyhow::Result<Vec<ClusterConfig>> {
    let content = std::fs::read_to_string(path)?;
    let configs: Vec<ClusterConfig> = serde_json::from_str(&content)?;
    Ok(configs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_minimal_cluster() {
        let json = r#"[{
            "name": "ozstar",
            "host": "ozstar.swin.edu.au",
            "username": "admin",
            "path": "/home/admin/jobcontroller"
        }]"#;

        let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "ozstar");
        assert_eq!(configs[0].host, "ozstar.swin.edu.au");
        assert_eq!(configs[0].username, "admin");
        assert_eq!(configs[0].path, "/home/admin/jobcontroller");
        assert_eq!(configs[0].connection_type, "ssh");
        assert!(configs[0].key.is_empty());
        assert!(configs[0].keytab.is_empty());
        assert!(configs[0].kerberos_principal.is_empty());
    }

    #[test]
    fn test_deserialize_full_cluster() {
        let json = r#"[{
            "name": "nci",
            "host": "gadi.nci.org.au",
            "username": "nciuser",
            "path": "/scratch/jobcontroller",
            "key": "/home/user/.ssh/id_rsa",
            "connection_type": "kerberos",
            "keytab": "/etc/krb5.keytab",
            "kerberos_principal": "user@NCI.ORG.AU"
        }]"#;

        let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].connection_type, "kerberos");
        assert_eq!(configs[0].keytab, "/etc/krb5.keytab");
        assert_eq!(configs[0].kerberos_principal, "user@NCI.ORG.AU");
        assert_eq!(configs[0].key, "/home/user/.ssh/id_rsa");
    }

    #[test]
    fn test_deserialize_multiple_clusters() {
        let json = r#"[
            {"name": "cluster1", "host": "host1", "username": "user1", "path": "/p1"},
            {"name": "cluster2", "host": "host2", "username": "user2", "path": "/p2"}
        ]"#;

        let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "cluster1");
        assert_eq!(configs[1].name, "cluster2");
    }

    #[test]
    fn test_load_cluster_configs_missing_file() {
        let result = load_cluster_configs(Path::new("/nonexistent/path.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_cluster_with_ltk() {
        let json = r#"[{
            "name": "ltk-cluster",
            "host": "cluster.example.com",
            "username": "jobclient",
            "path": "/path/to/client",
            "ltk": "super-secret-random-string"
        }]"#;

        let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "ltk-cluster");
        assert_eq!(
            configs[0].ltk,
            Some("super-secret-random-string".to_string())
        );
    }

    #[test]
    fn test_deserialize_cluster_without_ltk() {
        let json = r#"[{
            "name": "ssh-cluster",
            "host": "cluster.example.com",
            "username": "jobclient",
            "path": "/path/to/client"
        }]"#;

        let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "ssh-cluster");
        assert_eq!(configs[0].ltk, None);
    }

    #[test]
    fn test_deserialize_mixed_ltk_ssh_clusters() {
        let json = r#"[
            {
                "name": "ltk-cluster",
                "host": "ltk.example.com",
                "username": "jobclient",
                "path": "/path/ltk",
                "ltk": "ltk-secret-123"
            },
            {
                "name": "ssh-cluster",
                "host": "ssh.example.com",
                "username": "jobclient",
                "path": "/path/ssh",
                "key": "/home/user/.ssh/id_rsa"
            }
        ]"#;

        let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "ltk-cluster");
        assert_eq!(configs[0].ltk, Some("ltk-secret-123".to_string()));
        assert_eq!(configs[1].name, "ssh-cluster");
        assert_eq!(configs[1].ltk, None);
    }
}
