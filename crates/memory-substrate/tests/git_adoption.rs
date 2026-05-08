use memory_substrate::git::git_preflight;
use memory_substrate::merge::{merge_markdown, MergeInput, MergeResult};
use memory_substrate::tree::bootstrap_repo_tree;
use memory_substrate::{AdoptOptions, GitError, Roots, Substrate};

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

#[test]
fn fresh_clone_without_adoption_preflight_returns_repair_instruction() {
    let temp = tempfile::tempdir().expect("tempdir");
    let merge_driver = temp.path().join("memory-merge-driver");
    std::fs::write(&merge_driver, "#!/bin/sh\n").expect("driver");

    let err = git_preflight(temp.path(), &merge_driver).expect_err("not adopted");

    assert!(matches!(err, GitError::InvalidRepoRoot(message) if message.contains("git::adopt_clone")));
}

#[tokio::test]
async fn fresh_clone_with_adoption_can_perform_semantic_same_file_merge() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    bootstrap_repo_tree(&repo).expect("bootstrap repo");
    git(&repo, &["init"]).expect("git init");
    Substrate::adopt_clone(Roots::new(&repo, &runtime), adopt_options()).await.expect("adopt");
    let base = doc("base", 0.5, "base body");
    let ours = doc("ours summary", 0.5, "base body");
    let theirs = doc("base", 0.8, "base body");

    let MergeResult::Clean(merged) = merge_markdown(MergeInput {
        base: &base,
        ours: &ours,
        theirs: &theirs,
        path: "agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md",
    })
    .expect("semantic merge") else {
        panic!("independent same-file edits should merge cleanly");
    };

    assert!(merged.contains("summary: ours summary"));
    assert!(merged.contains("confidence: 0.8"));
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

    Substrate::adopt_clone(Roots::new(&repo, &runtime), AdoptOptions { force_new_device: true, ..adopt_options() })
        .await
        .expect("adopt with forced identity repair");
    let local_device = std::fs::read_to_string(runtime.join("local-device.yaml")).expect("local device");

    assert!(local_device.contains("  id: dev_"));
    assert!(!local_device.contains("dev_copied"));
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
