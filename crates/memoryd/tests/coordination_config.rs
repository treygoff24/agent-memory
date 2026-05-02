use memoryd::coordination_config::load_coordination_config;

#[test]
fn coordination_config_loads_defaults_when_block_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("config.yaml"), "schema_version: 1\n").expect("write config");

    let config = load_coordination_config(temp.path()).expect("load config");

    assert_eq!(config.level, 2);
    assert_eq!(config.relevance_gate.threshold, 0.6);
    assert_eq!(config.presence.stale_after_seconds, 300);
    assert_eq!(config.claim_lock.ttl_seconds, 300);
}

#[test]
fn coordination_config_loads_non_default_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("config.yaml"),
        r#"
schema_version: 1
coordination:
  level: 3
  relevance_gate:
    threshold: 0.8
    recency_window_seconds: 900
    per_turn_cap: 1
  presence:
    heartbeat_seconds: 20
    stale_after_seconds: 60
  claim_lock:
    ttl_seconds: 120
"#,
    )
    .expect("write config");

    let config = load_coordination_config(temp.path()).expect("load config");

    assert_eq!(config.level, 3);
    assert_eq!(config.relevance_gate.threshold, 0.8);
    assert_eq!(config.relevance_gate.recency_window_seconds, 900);
    assert_eq!(config.relevance_gate.per_turn_cap, 1);
    assert_eq!(config.presence.heartbeat_seconds, 20);
    assert_eq!(config.presence.stale_after_seconds, 60);
    assert_eq!(config.claim_lock.ttl_seconds, 120);
}

#[test]
fn coordination_config_rejects_invalid_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("config.yaml"),
        r#"
schema_version: 1
coordination:
  level: 4
"#,
    )
    .expect("write config");

    let error = load_coordination_config(temp.path()).expect_err("invalid level should fail");

    assert!(error.contains("coordination.level"), "{error}");
}
