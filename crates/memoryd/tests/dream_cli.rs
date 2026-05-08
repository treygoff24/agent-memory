use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{Duration, Utc};
use memory_substrate::tree::bootstrap_repo_tree;
use memoryd::protocol::{DreamStatusReport, LeaseRecord};
use serde_json::Value;
use tempfile::TempDir;

const DISCLOSURE_PREFIX: &str = "Dreaming uses whichever agent-harness CLI";

#[test]
fn dream_status_human_first_line_contains_privacy_disclosure() {
    let env = DreamCliEnv::new("dev_status");

    let output = env.memoryd(["dream", "status", "--repo", env.repo_str(), "--runtime", env.runtime_str()]);

    assert_success(&output);
    let stdout = stdout(&output);
    let first = stdout.lines().next().expect("status has first line");
    assert!(first.contains(DISCLOSURE_PREFIX), "first line was: {first}");
}

#[test]
fn dream_status_json_includes_inventory_leases_runs_and_counters() {
    let env = DreamCliEnv::new("dev_status_json");
    env.append_lease(LeaseRecord {
        device: "dev_status_json".to_string(),
        scope: "me".to_string(),
        acquired_at: Utc::now() - Duration::minutes(5),
        expires_at: Utc::now() + Duration::minutes(55),
        run_id: "run_status_json".to_string(),
    });
    env.write("dreams/journal/me/2026-04-30.md", "# Journal\nshort safe body\n");

    let output = env.memoryd(["dream", "status", "--repo", env.repo_str(), "--runtime", env.runtime_str(), "--json"]);

    assert_success(&output);
    let report: DreamStatusReport = serde_json::from_str(&stdout(&output)).expect("status report json");
    assert!(report.enabled);
    assert!(report.privacy_disclosure.contains(DISCLOSURE_PREFIX));
    assert!(report.cli_inventory.iter().any(|cli| cli.name == "claude"));
    assert!(report.active_leases.iter().any(|lease| lease.run_id == "run_status_json"));
    assert!(report.last_runs.iter().any(|run| run.scope == "me"));
    assert!(report.counters.dream_runs_invoked_total >= 1);
}

#[test]
fn dream_status_json_omits_released_leases_from_active_leases() {
    let env = DreamCliEnv::new("dev_status_release");
    let now = Utc::now();
    env.append_lease(LeaseRecord {
        device: "dev_status_release".to_string(),
        scope: "me".to_string(),
        acquired_at: now - Duration::minutes(5),
        expires_at: now + Duration::minutes(55),
        run_id: "run_active_then_released".to_string(),
    });
    env.append_lease(LeaseRecord {
        device: "dev_status_release".to_string(),
        scope: "me".to_string(),
        acquired_at: now,
        expires_at: now,
        run_id: "release_active_then_released".to_string(),
    });

    let output = env.memoryd(["dream", "status", "--repo", env.repo_str(), "--runtime", env.runtime_str(), "--json"]);

    assert_success(&output);
    let report: DreamStatusReport = serde_json::from_str(&stdout(&output)).expect("status report json");
    assert!(
        report.active_leases.iter().all(|lease| lease.scope != "me"),
        "released scope should not remain active: {:?}",
        report.active_leases
    );
}

#[test]
#[cfg(feature = "dev-fixtures")]
fn dream_now_echo_runs_pipeline_after_acquiring_lease() {
    let env = DreamCliEnv::new("dev_echo");

    let output = env.memoryd([
        "dream",
        "now",
        "--repo",
        env.repo_str(),
        "--runtime",
        env.runtime_str(),
        "--scope",
        "me",
        "--cli",
        "echo",
        "--json",
    ]);

    assert_success(&output);
    let json: Value = serde_json::from_str(&stdout(&output)).expect("dream report json");
    assert_eq!(json["scope"], "me");
    assert_eq!(json["cli_used"], "echo");
    assert_eq!(json["pass_1"]["status"], "success");
    assert_eq!(json["pass_3"]["status"], "success");
    let journal = json["pass_1"]["output_path"].as_str().expect("journal output path");
    assert!(env.repo.join(journal).is_file(), "journal should be written at {journal}");
}

#[test]
#[cfg(feature = "dev-fixtures")]
fn dream_now_respects_device_disabled_sentinel_before_lease_or_outputs() {
    let env = DreamCliEnv::new("dev_disabled_now");
    std::fs::write(env.runtime.join("dream-disabled"), "disabled\n").expect("disabled sentinel");

    let output = env.memoryd([
        "dream",
        "now",
        "--repo",
        env.repo_str(),
        "--runtime",
        env.runtime_str(),
        "--scope",
        "me",
        "--cli",
        "echo",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("dream_disabled"));
    assert!(!env.repo.join("dreams/journal/me").exists(), "disabled manual dreams must not write pass outputs");
    let lease_text = std::fs::read_to_string(env.repo.join("leases/journal.lease")).expect("lease file");
    assert!(lease_text.trim().is_empty(), "disabled manual dreams must not acquire a lease: {lease_text}");
}

#[test]
#[cfg(feature = "dev-fixtures")]
fn dream_scheduled_echo_runs_pipeline_and_writes_scheduled_summary() {
    let env = DreamCliEnv::new("dev_scheduled");

    let output = env.memoryd([
        "dream",
        "scheduled",
        "--repo",
        env.repo_str(),
        "--runtime",
        env.runtime_str(),
        "--scope",
        "me",
        "--cli",
        "echo",
        "--json",
    ]);

    assert_success(&output);
    let json: Value = serde_json::from_str(&stdout(&output)).expect("scheduled lease report json");
    assert_eq!(json["outcome"], "success");
    assert_eq!(json["consecutive_missed_runs"], 0);
    let cleanup_dir = env.repo.join("dreams/cleanup/dev_scheduled");
    assert!(cleanup_dir.is_dir(), "scheduled run should write cleanup summary under {}", cleanup_dir.display());
    let journal_dir = env.repo.join("dreams/journal/me");
    assert!(journal_dir.is_dir(), "scheduled run should execute the dream pipeline");
}

#[test]
#[cfg(feature = "dev-fixtures")]
fn dream_scheduled_respects_device_disabled_sentinel_before_lease_or_outputs() {
    let env = DreamCliEnv::new("dev_disabled_scheduled");
    std::fs::write(env.runtime.join("dream-disabled"), "disabled\n").expect("disabled sentinel");

    let output = env.memoryd([
        "dream",
        "scheduled",
        "--repo",
        env.repo_str(),
        "--runtime",
        env.runtime_str(),
        "--scope",
        "me",
        "--cli",
        "echo",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("dream_disabled"));
    assert!(!env.repo.join("dreams/journal/me").exists(), "disabled scheduled dreams must not write pass outputs");
    let lease_text = std::fs::read_to_string(env.repo.join("leases/journal.lease")).expect("lease file");
    assert!(lease_text.trim().is_empty(), "disabled scheduled dreams must not acquire a lease: {lease_text}");
}

#[test]
fn dream_review_lists_outputs_without_dumping_full_unsafe_bodies() {
    let env = DreamCliEnv::new("dev_review");
    env.write(
        "dreams/journal/project/proj_abc/2026-04-30.md",
        "# Journal\nSAFE JOURNAL SUMMARY\nSECRET_BODY_SHOULD_NOT_DUMP\n",
    );
    env.write(
        "dreams/questions/project/proj_abc/2026-04-30.jsonl",
        "{\"entities\":[\"ent_auth\"],\"question\":\"What safe question remains?\"}\n",
    );
    env.write(
        "dreams/cleanup/dev_review/2026-04-30.json",
        "{\"schema_version\":1,\"device_id\":\"dev_review\",\"date\":\"2026-04-30\",\"operations\":{\"fragments_archived\":2},\"findings\":[{\"kind\":\"memory_lint\",\"path\":\"agent/foo.md\",\"message\":\"safe finding\"}]}",
    );
    env.write(
        "agent/claims/dream-candidate.md",
        r#"---
schema_version: 1
id: mem_20260430_a1b2c3d4e5f60718_000001
type: claim
scope: agent
summary: Safe candidate summary
confidence: 0.8
trust_level: candidate
sensitivity: internal
status: candidate
created_at: "2026-04-30T00:00:00Z"
updated_at: "2026-04-30T00:00:00Z"
author:
  kind: dreaming
source:
  kind: session
  reference: "dream:run"
evidence: []
requires_user_confirmation: true
review_state: candidate
retrieval_policy:
  index_body: false
  index_embeddings: false
  passive_recall: false
  mask_personal_for_synthesis: true
  max_scope: agent
write_policy:
  human_review_required: true
  policy_applied: dreaming-strict
---
SECRET_CANDIDATE_BODY_SHOULD_NOT_DUMP
"#,
    );

    let output =
        env.memoryd(["dream", "review", "--repo", env.repo_str(), "--runtime", env.runtime_str(), "--since", "7d"]);

    assert_success(&output);
    let out = stdout(&output);
    for expected in ["journal", "question", "candidate", "cleanup", "SAFE JOURNAL SUMMARY", "Safe candidate summary"] {
        assert!(out.contains(expected), "review output missing {expected}: {out}");
    }
    assert!(!out.contains("SECRET_BODY_SHOULD_NOT_DUMP"));
    assert!(!out.contains("SECRET_CANDIDATE_BODY_SHOULD_NOT_DUMP"));
}

#[test]
fn dream_enable_disable_toggle_runtime_local_sentinel() {
    let env = DreamCliEnv::new("dev_toggle");
    let sentinel = env.runtime.join("dream-disabled");

    assert_success(&env.memoryd(["dream", "disable", "--runtime", env.runtime_str()]));
    assert!(sentinel.is_file(), "disable should create runtime-local sentinel at {}", sentinel.display());

    assert_success(&env.memoryd(["dream", "enable", "--runtime", env.runtime_str()]));
    assert!(!sentinel.exists(), "enable should remove runtime-local sentinel at {}", sentinel.display());
}

#[test]
fn dream_enable_first_run_prints_privacy_disclosure_before_enabling() {
    let env = DreamCliEnv::new("dev_enable");
    std::fs::write(env.runtime.join("dream-disabled"), "disabled\n").expect("seed disabled sentinel");

    let output = env.memoryd(["dream", "enable", "--runtime", env.runtime_str()]);

    assert_success(&output);
    let out = stdout(&output);
    let first = out.lines().next().expect("enable first line");
    assert!(first.contains(DISCLOSURE_PREFIX), "first line was: {first}");
    assert!(!env.runtime.join("dream-disabled").exists());
}

#[test]
#[cfg(feature = "dev-fixtures")]
fn dream_manual_lease_failure_exit_code_5_remains_covered() {
    let env = DreamCliEnv::new("dev_lease_failure");
    env.append_lease(LeaseRecord {
        device: "dev_foreign".to_string(),
        scope: "me".to_string(),
        acquired_at: Utc::now() - Duration::minutes(1),
        expires_at: Utc::now() + Duration::days(1),
        run_id: "run_foreign".to_string(),
    });
    env.commit_all("seed foreign lease");

    let output = env.memoryd([
        "dream",
        "now",
        "--repo",
        env.repo_str(),
        "--runtime",
        env.runtime_str(),
        "--scope",
        "me",
        "--cli",
        "echo",
    ]);

    assert_eq!(output.status.code(), Some(5));
    assert!(stderr(&output).contains("lease_held"));
}

#[test]
fn dream_cleanup_writes_report_json() {
    let env = DreamCliEnv::new("dev_cleanupcli");

    let output = env.memoryd([
        "dream",
        "cleanup",
        "--repo",
        env.repo_str(),
        "--runtime",
        env.runtime_str(),
        "--now",
        "2026-04-30T03:00:00Z",
        "--json",
    ]);

    assert_success(&output);
    let json: Value = serde_json::from_str(&stdout(&output)).expect("cleanup report json");
    assert_eq!(json["device_id"], "dev_cleanupcli");
    assert_eq!(json["date"], "2026-04-30");
    assert!(json["commit_deferred"].is_boolean());
    assert!(env.repo.join("dreams/cleanup/dev_cleanupcli/2026-04-30.json").is_file());
}

#[test]
fn dream_cleanup_defers_commit_when_user_work_is_dirty() {
    let env = DreamCliEnv::new("dev_cleanupdirty");
    env.write("human-uncommitted.txt", "do not commit me\n");

    let output = env.memoryd([
        "dream",
        "cleanup",
        "--repo",
        env.repo_str(),
        "--runtime",
        env.runtime_str(),
        "--now",
        "2026-04-30T03:00:00Z",
        "--json",
    ]);

    assert_success(&output);
    let json: Value = serde_json::from_str(&stdout(&output)).expect("cleanup report json");
    assert_eq!(json["commit_deferred"], true);
    assert!(env.repo.join("dreams/cleanup/dev_cleanupdirty/2026-04-30.json").is_file());
    let status = command_in(&env.repo, "git", ["status", "--porcelain=v1", "--untracked-files=all"]);
    assert!(status.contains("human-uncommitted.txt"), "dirty user work should remain uncommitted: {status}");
}

struct DreamCliEnv {
    _temp: TempDir,
    repo: PathBuf,
    runtime: PathBuf,
}

impl DreamCliEnv {
    fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        bootstrap_repo_tree(&repo).expect("bootstrap repo tree");
        std::fs::create_dir_all(&runtime).expect("runtime dir");
        std::fs::write(
            runtime.join("local-device.yaml"),
            format!(
                "schema_version: 1\ndevice:\n  id: {device_id}\n  name: test\n  shard: test\npaths:\n  memory_root: {}\n  runtime_root: {}\n",
                repo.display(),
                runtime.display()
            ),
        )
        .expect("local device");
        command_in(&repo, "git", ["init", "-b", "main"]);
        command_in(&repo, "git", ["config", "user.email", "test@example.com"]);
        command_in(&repo, "git", ["config", "user.name", "Test User"]);
        std::fs::write(repo.join("leases/journal.lease"), "").expect("lease file");
        command_in(&repo, "git", ["add", "."]);
        command_in(&repo, "git", ["commit", "-m", "bootstrap"]);
        let origin = temp.path().join("origin.git");
        command_in(temp.path(), "git", ["init", "--bare", origin.to_str().expect("origin path")]);
        command_in(&repo, "git", ["remote", "add", "origin", origin.to_str().expect("origin path")]);
        command_in(&repo, "git", ["push", "-u", "origin", "main"]);
        Self { _temp: temp, repo, runtime }
    }

    fn append_lease(&self, record: LeaseRecord) {
        use std::io::Write;
        let mut file =
            std::fs::OpenOptions::new().append(true).open(self.repo.join("leases/journal.lease")).expect("open lease");
        writeln!(file, "{}", serde_json::to_string(&record).expect("lease serializes")).expect("append lease");
    }

    #[cfg(feature = "dev-fixtures")]
    fn commit_all(&self, message: &str) {
        command_in(&self.repo, "git", ["add", "."]);
        command_in(&self.repo, "git", ["commit", "-m", message]);
        command_in(&self.repo, "git", ["push"]);
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.repo.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(path, contents).expect("write fixture file");
    }

    fn memoryd<const N: usize>(&self, args: [&str; N]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .env("MEMORYD_ENABLE_ECHO_DREAM_HARNESS", "1")
            .args(args)
            .output()
            .expect("memoryd command")
    }

    fn repo_str(&self) -> &str {
        self.repo.to_str().expect("repo path")
    }

    fn runtime_str(&self) -> &str {
        self.runtime.to_str().expect("runtime path")
    }
}

fn command_in<const N: usize>(cwd: &Path, program: &str, args: [&str; N]) -> String {
    let output = Command::new(program).args(args).current_dir(cwd).output().expect("command runs");
    if output.status.success() {
        String::from_utf8(output.stdout).expect("stdout utf8").trim().to_string()
    } else {
        panic!(
            "{program} {args:?} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
