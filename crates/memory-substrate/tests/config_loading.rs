use memory_substrate::config::{load_config, load_local_device_config, load_synced_config};
use memory_substrate::tree::bootstrap_repo_tree;
use memory_substrate::Roots;
use once_cell::sync::Lazy;
use std::sync::Mutex;

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

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
