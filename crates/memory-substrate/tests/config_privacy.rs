use memory_substrate::config::{load_config, load_local_device_config, PrivacyEnforcement};
use memory_substrate::tree::bootstrap_repo_tree;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use walkdir::WalkDir;

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[test]
fn privacy_enforcement_defaults_to_dogfood_when_local_config_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");

    let loaded = load_config(&repo, &runtime, None).expect("load config");

    assert_eq!(loaded.privacy_enforcement(), PrivacyEnforcement::default());
    assert_eq!(
        loaded.privacy_enforcement(),
        PrivacyEnforcement { classifier: false, encryption: false, masking: false }
    );
}

#[test]
fn local_device_privacy_is_per_device_and_not_synced() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime_a = temp.path().join("runtime-a");
    let runtime_b = temp.path().join("runtime-b");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    std::fs::create_dir_all(&runtime_a).expect("runtime a");
    std::fs::write(
        runtime_a.join("local-device.yaml"),
        r#"schema_version: 1
device:
  id: dev_private
  name: private
  shard: a1b2c3d4e5f60718
privacy:
  classifier: true
  encryption: true
  masking: true
"#,
    )
    .expect("local config a");
    std::fs::create_dir_all(&runtime_b).expect("runtime b");
    std::fs::write(
        runtime_b.join("local-device.yaml"),
        r#"schema_version: 1
device:
  id: dev_default
  name: default
  shard: a1b2c3d4e5f60718
"#,
    )
    .expect("local config b");

    let local_a = load_local_device_config(&runtime_a).expect("load local a").expect("local config a");
    let loaded_a = load_config(&repo, &runtime_a, None).expect("load config a");
    let local_b = load_local_device_config(&runtime_b).expect("load local b").expect("local config b");
    let loaded_b = load_config(&repo, &runtime_b, None).expect("load config b");

    assert_eq!(local_a.privacy, PrivacyEnforcement::paranoid());
    assert_eq!(loaded_a.privacy_enforcement(), PrivacyEnforcement::paranoid());
    assert_eq!(local_b.privacy, PrivacyEnforcement::default());
    assert_eq!(loaded_b.privacy_enforcement(), PrivacyEnforcement::default());
    assert_repo_has_no_local_privacy_state(&repo);
}

#[test]
fn local_device_config_without_privacy_deserializes_to_dogfood_default() {
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

    assert_eq!(local.privacy, PrivacyEnforcement::default());
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

fn assert_repo_has_no_local_privacy_state(repo: &std::path::Path) {
    let forbidden = ["local-device", "dev_private", "dev_default", "privacy", "classifier", "encryption", "masking"];
    for entry in WalkDir::new(repo).into_iter().filter_map(Result::ok).filter(|entry| entry.file_type().is_file()) {
        let relative = entry.path().strip_prefix(repo).expect("repo-relative path");
        let relative_text = relative.to_string_lossy();
        for token in forbidden {
            assert!(
                !relative_text.contains(token),
                "local privacy token {token:?} leaked into repo path {relative_text}"
            );
        }
        let contents = std::fs::read_to_string(entry.path()).expect("repo file is utf8 test fixture");
        for token in forbidden {
            assert!(
                !contents.contains(token),
                "local privacy token {token:?} leaked into synced repo file {}",
                relative.display()
            );
        }
    }
}
