use memory_substrate::config::{load_config, load_local_device_config, PrivacyEnforcement};
use memory_substrate::tree::bootstrap_repo_tree;
use once_cell::sync::Lazy;
use std::sync::Mutex;

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[test]
fn privacy_enforcement_defaults_to_safe_when_local_config_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");

    let loaded = load_config(&repo, &runtime, None).expect("load config");

    assert_eq!(loaded.privacy_enforcement(), PrivacyEnforcement::default());
    assert_eq!(loaded.privacy_enforcement(), PrivacyEnforcement { classifier: true, encryption: true, masking: true });
}

#[test]
fn local_device_privacy_is_per_device_and_not_synced() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    std::fs::create_dir_all(&runtime).expect("runtime");
    std::fs::write(
        runtime.join("local-device.yaml"),
        r#"schema_version: 1
device:
  id: dev_local
  name: local
  shard: a1b2c3d4e5f60718
privacy:
  classifier: true
  encryption: true
  masking: true
"#,
    )
    .expect("local config");

    let local = load_local_device_config(&runtime).expect("load local").expect("local config");
    let loaded = load_config(&repo, &runtime, None).expect("load config");

    assert_eq!(local.privacy, PrivacyEnforcement::paranoid());
    assert_eq!(loaded.privacy_enforcement(), PrivacyEnforcement::paranoid());
    let synced = std::fs::read_to_string(repo.join("config.yaml")).expect("synced config");
    assert!(!synced.contains("privacy:"));
}

#[test]
fn local_device_config_without_privacy_deserializes_to_safe_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(&runtime).expect("runtime");
    std::fs::write(
        runtime.join("local-device.yaml"),
        r#"schema_version: 1
device:
  id: dev_local
  name: local
  shard: a1b2c3d4e5f60718
"#,
    )
    .expect("local config");

    let local = load_local_device_config(&runtime).expect("load local").expect("local config");

    assert_eq!(local.privacy, PrivacyEnforcement::paranoid());
}

#[test]
fn privacy_enforcement_yaml_and_env_parse_bool_switches() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    std::env::remove_var("MEMORUM_PRIVACY_CLASSIFIER");
    std::env::remove_var("MEMORUM_PRIVACY_ENCRYPTION");
    std::env::remove_var("MEMORUM_PRIVACY_MASKING");

    let parsed = PrivacyEnforcement::from_yaml(
        r#"classifier: true
encryption: false
masking: true
"#,
    )
    .expect("parse yaml");
    assert_eq!(parsed, PrivacyEnforcement { classifier: true, encryption: false, masking: true });

    std::env::set_var("MEMORUM_PRIVACY_CLASSIFIER", "on");
    std::env::set_var("MEMORUM_PRIVACY_ENCRYPTION", "1");
    std::env::set_var("MEMORUM_PRIVACY_MASKING", "no");
    let env = PrivacyEnforcement::from_env().expect("parse env");
    std::env::remove_var("MEMORUM_PRIVACY_CLASSIFIER");
    std::env::remove_var("MEMORUM_PRIVACY_ENCRYPTION");
    std::env::remove_var("MEMORUM_PRIVACY_MASKING");

    assert_eq!(env, PrivacyEnforcement { classifier: true, encryption: true, masking: false });
}
