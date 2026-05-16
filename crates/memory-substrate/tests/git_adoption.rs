use memory_substrate::git::git_preflight;
use memory_substrate::tree::bootstrap_repo_tree;
use memory_substrate::{AdoptOptions, GitError, MemoryId, Roots, Substrate};
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn fresh_clone_adoption_regenerates_local_identity_event_log_and_merge_config() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    git(&repo, &["init"]).expect("git init");

    let substrate = Substrate::adopt_clone(Roots::new(&repo, &runtime), adopt_options()).await.expect("adopt");

    let local_device = std::fs::read_to_string(runtime.join("local-device.yaml")).expect("local device");
    assert!(local_device.contains("device:"));
    assert!(local_device.contains("  id: dev_"));
    let events_dir = repo.join("events");
    assert!(std::fs::read_dir(&events_dir)
        .expect("events dir")
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl")));
    let driver = git(&repo, &["config", "merge.memory-merge-driver.driver"]).expect("merge driver config");
    assert!(driver.contains("--base %O --ours %A --theirs %B --path %P"));
    assert!(matches!(
        substrate.durability_tier(),
        memory_substrate::DurabilityTier::BestEffort | memory_substrate::DurabilityTier::Full
    ));
}

#[tokio::test]
async fn fresh_clone_without_adoption_preflight_returns_repair_instruction() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let merge_driver = temp.path().join("memory-merge-driver");
    write_executable(&merge_driver, "#!/bin/sh\nexit 0\n");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    git(&repo, &["init"]).expect("git init");

    let err = git_preflight(&repo, &merge_driver).expect_err("real git repo still needs adoption-specific config");

    assert!(matches!(err, GitError::InvalidRepoRoot(message) if message.contains("git::adopt_clone")));

    Substrate::adopt_clone(
        Roots::new(&repo, &runtime),
        AdoptOptions { merge_driver_path: Some(merge_driver.clone()), ..adopt_options() },
    )
    .await
    .expect("adopt");
    git_preflight(&repo, &merge_driver).expect("adopted repo passes preflight");
}

#[tokio::test]
async fn fresh_clone_with_adoption_invokes_configured_git_merge_driver() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let merge_driver = temp.path().join("memory-merge-driver");
    write_semantic_merge_driver(&merge_driver);
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    git(&repo, &["init"]).expect("git init");
    configure_git_identity(&repo);
    Substrate::adopt_clone(
        Roots::new(&repo, &runtime),
        AdoptOptions { merge_driver_path: Some(merge_driver), ..adopt_options() },
    )
    .await
    .expect("adopt");
    let memory_path = repo.join("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md");
    let base = doc("base", 0.5, "base body");
    let ours = doc("ours summary", 0.5, "base body");
    let theirs = doc("base", 0.8, "base body");
    std::fs::write(&memory_path, &base).expect("write base");
    git(&repo, &["add", "."]).expect("add base");
    git(&repo, &["commit", "-m", "base"]).expect("commit base");

    git(&repo, &["checkout", "-b", "ours"]).expect("checkout ours");
    std::fs::write(&memory_path, ours).expect("write ours");
    git(&repo, &["add", "agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md"]).expect("add ours");
    git(&repo, &["commit", "-m", "ours summary"]).expect("commit ours");

    git(&repo, &["checkout", "-b", "theirs", "HEAD~1"]).expect("checkout theirs");
    std::fs::write(&memory_path, theirs).expect("write theirs");
    git(&repo, &["add", "agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md"]).expect("add theirs");
    git(&repo, &["commit", "-m", "theirs confidence"]).expect("commit theirs");

    git(&repo, &["merge", "ours"]).expect("git merge through configured driver");
    let merged = std::fs::read_to_string(&memory_path).expect("merged memory");

    assert!(merged.contains("summary: ours summary"));
    assert!(merged.contains("confidence: 0.8"));
    assert_eq!(git(&repo, &["status", "--porcelain"]).expect("status"), "");
}

#[tokio::test]
async fn adoption_force_new_device_regenerates_local_identity_before_writes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    git(&repo, &["init"]).expect("git init");
    std::fs::create_dir_all(&runtime).expect("runtime");
    std::fs::write(
        runtime.join("local-device.yaml"),
        "schema_version: 1\ndevice:\n  id: dev_copied\n  name: copied\n  shard: copied\npaths:\n  memory_root: copied\n  runtime_root: copied\n",
    )
    .expect("copied local device");

    let substrate =
        Substrate::adopt_clone(Roots::new(&repo, &runtime), AdoptOptions { force_new_device: true, ..adopt_options() })
            .await
            .expect("adopt with forced identity repair");
    let local_device = std::fs::read_to_string(runtime.join("local-device.yaml")).expect("local device");
    let regenerated_device = local_device_id(&local_device);
    substrate
        .record_recall_hit(MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000099"))
        .expect("record event after forced adoption");
    let event_bytes = read_all_event_logs(&repo);

    assert!(local_device.contains("  id: dev_"));
    assert!(!local_device.contains("dev_copied"));
    assert!(!event_bytes.contains("dev_copied"));
    assert!(event_bytes.contains(&regenerated_device), "event logs should use regenerated device id");
}

fn configure_git_identity(repo: &std::path::Path) {
    git(repo, &["config", "user.name", "Memorum Test"]).expect("git user name");
    git(repo, &["config", "user.email", "memorum-test@example.invalid"]).expect("git user email");
}

fn git(repo: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output =
        std::process::Command::new("git").args(args).current_dir(repo).output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn adopt_options() -> AdoptOptions {
    AdoptOptions { force_new_device: false, merge_driver_path: Some(std::env::current_exe().expect("current_exe")) }
}

fn write_executable(path: &std::path::Path, body: &str) {
    std::fs::write(path, body).expect("write executable");
    let mut permissions = std::fs::metadata(path).expect("executable metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod executable");
}

fn write_semantic_merge_driver(path: &std::path::Path) {
    write_executable(
        path,
        r#"#!/bin/sh
set -eu
base=
ours=
theirs=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --base) base="$2"; shift 2 ;;
    --ours) ours="$2"; shift 2 ;;
    --theirs) theirs="$2"; shift 2 ;;
    --path) shift 2 ;;
    *) shift ;;
  esac
done
summary=$(awk '/^summary: / { sub(/^summary: /, ""); print; exit }' "$theirs")
confidence=$(awk '/^confidence: / { sub(/^confidence: /, ""); print; exit }' "$ours")
awk -v summary="$summary" -v confidence="$confidence" '
  /^summary: / { print "summary: " summary; next }
  /^confidence: / { print "confidence: " confidence; next }
  { print }
' "$base" > "$ours.tmp"
mv "$ours.tmp" "$ours"
"#,
    );
}

fn local_device_id(yaml: &str) -> String {
    yaml.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("id: "))
        .expect("local-device.yaml contains id")
        .to_string()
}

fn read_all_event_logs(repo: &std::path::Path) -> String {
    let mut body = String::new();
    for entry in std::fs::read_dir(repo.join("events")).expect("events dir") {
        let entry = entry.expect("event dir entry");
        if entry.path().extension().is_some_and(|ext| ext == "jsonl") {
            body.push_str(&std::fs::read_to_string(entry.path()).expect("event log text"));
        }
    }
    body
}

fn doc(summary: &str, confidence: f64, body: &str) -> String {
    format!(
        r#"---
schema_version: 1
id: mem_20260424_a1b2c3d4e5f60718_000001
type: pattern
scope: agent
summary: {summary}
confidence: {confidence}
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: null
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: test
---
{body}
"#
    )
}
