use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct AccessSecret {
    pub name: String,
    pub secret: String,
    pub applications: Vec<String>,
    pub clusters: Vec<String>,
}

/// Load access secret configurations from a JSON file.
pub fn load_access_secrets(path: &Path) -> anyhow::Result<Vec<AccessSecret>> {
    let content = std::fs::read_to_string(path)?;
    let secrets: Vec<AccessSecret> = serde_json::from_str(&content)?;
    Ok(secrets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_access_secret() {
        let json = r#"[{
            "name": "gwcloud",
            "secret": "super_secret_key_123",
            "applications": ["compas", "bilby"],
            "clusters": ["ozstar", "nci"]
        }]"#;

        let secrets: Vec<AccessSecret> = serde_json::from_str(json).unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].name, "gwcloud");
        assert_eq!(secrets[0].secret, "super_secret_key_123");
        assert_eq!(secrets[0].applications, vec!["compas", "bilby"]);
        assert_eq!(secrets[0].clusters, vec!["ozstar", "nci"]);
    }

    #[test]
    fn test_deserialize_multiple_secrets() {
        let json = r#"[
            {"name": "app1", "secret": "s1", "applications": ["a"], "clusters": ["c1"]},
            {"name": "app2", "secret": "s2", "applications": ["b", "c"], "clusters": ["c2", "c3"]}
        ]"#;

        let secrets: Vec<AccessSecret> = serde_json::from_str(json).unwrap();
        assert_eq!(secrets.len(), 2);
        assert_eq!(secrets[1].applications.len(), 2);
        assert_eq!(secrets[1].clusters.len(), 2);
    }

    #[test]
    fn test_deserialize_empty_arrays() {
        let json = r#"[{
            "name": "test",
            "secret": "key",
            "applications": [],
            "clusters": []
        }]"#;

        let secrets: Vec<AccessSecret> = serde_json::from_str(json).unwrap();
        assert!(secrets[0].applications.is_empty());
        assert!(secrets[0].clusters.is_empty());
    }

    #[test]
    fn test_load_access_secrets_missing_file() {
        let result = load_access_secrets(Path::new("/nonexistent/path.json"));
        assert!(result.is_err());
    }
}
