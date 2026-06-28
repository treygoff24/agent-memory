use memory_substrate::config::{
    load_config, load_local_device_config, load_synced_config, DreamsConfig, SubstrateConfig, SyncedConfig,
};
use memory_substrate::tree::bootstrap_repo_tree;
use memory_substrate::Roots;
use std::sync::LazyLock;
use std::sync::Mutex;

static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[test]
fn fresh_clone_has_synced_config_but_no_local_device_until_adoption() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");

    let synced = load_synced_config(&repo).expect("synced config").expect("config.yaml present after bootstrap");
    let local = load_local_device_config(&runtime).expect("local config");

    assert_eq!(synced.schema_version, 1);
    assert!(local.is_none());
}

#[test]
fn loading_config_never_copies_device_id_from_synced_repo_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    std::fs::write(
        repo.join("config.yaml"),
        r#"schema_version: 1
device:
  id: dev_other_machine
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
"#,
    )
    .expect("synced config");

    let loaded = load_config(&repo, &runtime, None).expect("load config");

    assert!(loaded.local.is_none());
    assert!(!runtime.join("local-device.yaml").exists());
}

#[test]
fn env_overrides_are_visible_but_not_serialized_to_synced_config() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    let before = std::fs::read_to_string(repo.join("config.yaml")).expect("before");
    let env_repo = temp.path().join("env-repo");
    let env_runtime = temp.path().join("env-runtime");
    std::env::set_var("STREAM_A_MEMORY_ROOT", &env_repo);
    std::env::set_var("STREAM_A_RUNTIME_ROOT", &env_runtime);

    let loaded = load_config(&repo, &runtime, None).expect("load config");

    std::env::remove_var("STREAM_A_MEMORY_ROOT");
    std::env::remove_var("STREAM_A_RUNTIME_ROOT");
    assert_eq!(loaded.roots, Roots::new(env_repo, env_runtime));
    assert_eq!(std::fs::read_to_string(repo.join("config.yaml")).expect("after"), before);
}

#[test]
fn local_roots_win_over_synced_defaults_without_mutating_synced_config() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    let synced_text = r#"schema_version: 1
paths:
  memory_root: /synced/memory
  runtime_root: /synced/runtime
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
"#;
    std::fs::write(repo.join("config.yaml"), synced_text).expect("synced config");
    std::fs::create_dir_all(&runtime).expect("runtime");
    std::fs::write(
        runtime.join("local-device.yaml"),
        r#"schema_version: 1
device:
  id: dev_local
  name: local
  shard: a1b2c3d4e5f60718
paths:
  memory_root: /local/memory
  runtime_root: /local/runtime
"#,
    )
    .expect("local config");

    let loaded = load_config(&repo, &runtime, None).expect("load config");
    let local = loaded.local.expect("local config loaded");

    assert_eq!(local.device.id, "dev_local");
    assert_eq!(loaded.roots, Roots::new("/local/memory", "/local/runtime"));
    assert_eq!(std::fs::read_to_string(repo.join("config.yaml")).expect("synced unchanged"), synced_text);
}

#[test]
fn dreams_config_defaults_when_dreams_and_events_are_omitted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    std::fs::write(
        repo.join("config.yaml"),
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
"#,
    )
    .expect("synced config");

    let synced = load_synced_config(&repo).expect("synced config").expect("config present");

    assert!(synced.dreams.enabled);
    assert_eq!(synced.dreams.default_cli_priority, vec!["claude".to_string(), "codex".to_string()]);
    assert!(synced.dreams.scope_overrides.is_empty());
    assert_eq!(synced.dreams.per_pass_timeout_seconds, 300);
    assert_eq!(synced.dreams.pass_1_window_days, 7);
    assert_eq!(synced.dreams.pass_2_max_candidates, 8);
    assert_eq!(synced.dreams.pass_2_drift_threshold, 0.30);
    assert_eq!(synced.dreams.pass_3_max_questions, 12);
    assert_eq!(synced.dreams.pending_attention_per_scope_cap, 2);
    assert_eq!(synced.dreams.pending_attention_total_cap, 6);
    assert_eq!(synced.dreams.pending_attention_recent_window_days, 7);
    assert_eq!(synced.dreams.fragment_lifetime_days, 14);
    assert_eq!(synced.dreams.candidate_stale_days, 30);
    assert_eq!(synced.dreams.cleanup_run_hour_utc, 3);
    assert_eq!(synced.dreams.lease_window_seconds, 3600);
    assert_eq!(synced.dreams.dream_retry_window_minutes, 180);
    assert_eq!(synced.dreams.doctor_missed_threshold, 2);
    assert_eq!(synced.dreams.doctor_budget_exhausted_threshold, 500);
    assert_eq!(synced.dreams.capture_drought_days, 3);
    assert_eq!(synced.substrate.commit_debounce_ms, 2000);
    assert_eq!(synced.substrate.commit_stale_grace_ms, 5000);
    assert_eq!(synced.events.compaction_days, 90);
}

#[test]
fn synced_and_dreams_config_retain_eq_contract() {
    fn assert_eq_bound<T: Eq>() {}

    assert_eq_bound::<DreamsConfig>();
    assert_eq_bound::<SubstrateConfig>();
    assert_eq_bound::<SyncedConfig>();
}

#[test]
fn dreams_config_rejects_unknown_cli_names() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  default_cli_priority: [claude, unknown_harness]
"#,
    )
    .expect_err("unknown CLI rejected");

    assert!(err.contains("unknown_harness"), "actual error: {err}");
}

#[test]
fn dreams_config_rejects_deferred_gemini_harness_until_adapter_ships() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  default_cli_priority: [gemini]
"#,
    )
    .expect_err("deferred Gemini adapter is not a valid v0.2 config name");

    assert!(err.contains("gemini"), "actual error: {err}");
}

#[test]
fn dreams_config_rejects_bad_scope_override_keys() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  scope_overrides:
    project: [codex]
"#,
    )
    .expect_err("bad scope key rejected");

    assert!(err.contains("project"), "actual error: {err}");
}

#[test]
fn dreams_config_rejects_out_of_range_numeric_values() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  pass_1_window_days: 0
"#,
    )
    .expect_err("out of range value rejected");

    assert!(err.contains("pass_1_window_days"), "actual error: {err}");
}

#[test]
fn dreams_config_rejects_per_scope_cap_greater_than_total_cap() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  pending_attention_per_scope_cap: 7
  pending_attention_total_cap: 6
"#,
    )
    .expect_err("invalid cap relationship rejected");

    assert!(err.contains("pending_attention_per_scope_cap"), "actual error: {err}");
}

#[test]
fn substrate_config_rejects_out_of_range_commit_debounce() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
substrate:
  commit_debounce_ms: 30001
"#,
    )
    .expect_err("out of range substrate debounce rejected");

    assert!(err.contains("substrate.commit_debounce_ms"), "actual error: {err}");
}

#[test]
fn substrate_config_rejects_out_of_range_commit_stale_grace() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
substrate:
  commit_stale_grace_ms: 60001
"#,
    )
    .expect_err("out of range substrate stale grace rejected");

    assert!(err.contains("substrate.commit_stale_grace_ms"), "actual error: {err}");
}

#[test]
fn dreams_config_rejects_out_of_range_doctor_missed_threshold() {
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  doctor_missed_threshold: 101
"#,
    )
    .expect_err("out of range doctor missed threshold rejected");

    assert!(err.contains("dreams.doctor_missed_threshold"), "actual error: {err}");
}

#[test]
fn dreams_config_rejects_out_of_range_capture_drought_days() {
    // Note: `dreams.doctor_budget_exhausted_threshold` is an unbounded cumulative count
    // (no range gate by design — see config/mod.rs validate_config), so it has no
    // out-of-range case to assert.
    let err = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  capture_drought_days: 366
"#,
    )
    .expect_err("out of range capture drought days rejected");

    assert!(err.contains("dreams.capture_drought_days"), "actual error: {err}");
}

#[test]
fn dreams_config_parses_all_v0_2_keys_and_preserves_values() {
    let synced = load_synced_config_from_text(
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  enabled: false
  default_cli_priority: [codex, claude]
  scope_overrides:
    me: [claude]
    project:proj_abc: [codex, claude]
    org:org_123: [claude]
    agent: [codex]
  per_pass_timeout_seconds: 600
  pass_1_window_days: 21
  pass_2_max_candidates: 13
  pass_2_drift_threshold: 0.42
  pass_3_max_questions: 9
  pending_attention_per_scope_cap: 3
  pending_attention_total_cap: 11
  pending_attention_recent_window_days: 12
  fragment_lifetime_days: 31
  candidate_stale_days: 45
  cleanup_run_hour_utc: 11
  lease_window_seconds: 7200
  dream_retry_window_minutes: 240
  doctor_missed_threshold: 5
  doctor_budget_exhausted_threshold: 999
  capture_drought_days: 9
events:
  compaction_days: 180
substrate:
  commit_debounce_ms: 123
  commit_stale_grace_ms: 456
"#,
    )
    .expect("valid dreams config");

    assert!(!synced.dreams.enabled);
    assert_eq!(synced.dreams.default_cli_priority, vec!["codex".to_string(), "claude".to_string()]);
    assert_eq!(synced.dreams.scope_overrides["me"], vec!["claude".to_string()]);
    assert_eq!(synced.dreams.scope_overrides["project:proj_abc"], vec!["codex".to_string(), "claude".to_string()]);
    assert_eq!(synced.dreams.scope_overrides["org:org_123"], vec!["claude".to_string()]);
    assert_eq!(synced.dreams.scope_overrides["agent"], vec!["codex".to_string()]);
    assert_eq!(synced.dreams.per_pass_timeout_seconds, 600);
    assert_eq!(synced.dreams.pass_1_window_days, 21);
    assert_eq!(synced.dreams.pass_2_max_candidates, 13);
    assert_eq!(synced.dreams.pass_2_drift_threshold, 0.42);
    assert_eq!(synced.dreams.pass_3_max_questions, 9);
    assert_eq!(synced.dreams.pending_attention_per_scope_cap, 3);
    assert_eq!(synced.dreams.pending_attention_total_cap, 11);
    assert_eq!(synced.dreams.pending_attention_recent_window_days, 12);
    assert_eq!(synced.dreams.fragment_lifetime_days, 31);
    assert_eq!(synced.dreams.candidate_stale_days, 45);
    assert_eq!(synced.dreams.cleanup_run_hour_utc, 11);
    assert_eq!(synced.dreams.lease_window_seconds, 7200);
    assert_eq!(synced.dreams.dream_retry_window_minutes, 240);
    assert_eq!(synced.dreams.doctor_missed_threshold, 5);
    assert_eq!(synced.dreams.doctor_budget_exhausted_threshold, 999);
    assert_eq!(synced.dreams.capture_drought_days, 9);
    assert_eq!(synced.substrate.commit_debounce_ms, 123);
    assert_eq!(synced.substrate.commit_stale_grace_ms, 456);
    assert_eq!(synced.events.compaction_days, 180);
}

fn load_synced_config_from_text(text: &str) -> Result<memory_substrate::config::SyncedConfig, String> {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    std::fs::write(repo.join("config.yaml"), text).expect("config");
    load_synced_config(&repo).map(|loaded| loaded.expect("config exists"))
}
