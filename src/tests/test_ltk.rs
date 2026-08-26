//! Integration tests for Long-Term Secret Keys (LTK) authentication.

use adacs_job_controller::config::clusters::ClusterConfig;

// ---------------------------------------------------------------------------
// Test: LTK configuration deserialization
// ---------------------------------------------------------------------------

#[test]
fn test_ltk_config_deserialization() {
    let json = r#"[{
        "name": "test-ltk",
        "host": "localhost",
        "username": "test",
        "path": "/test",
        "ltk": "my-secret-ltk"
    }]"#;

    let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "test-ltk");
    assert_eq!(configs[0].ltk, Some("my-secret-ltk".to_string()));
}

#[test]
fn test_ltk_config_missing_field() {
    let json = r#"[{
        "name": "test-ssh",
        "host": "localhost",
        "username": "test",
        "path": "/test"
    }]"#;

    let configs: Vec<ClusterConfig> = serde_json::from_str(json).unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "test-ssh");
    assert_eq!(configs[0].ltk, None);
}
