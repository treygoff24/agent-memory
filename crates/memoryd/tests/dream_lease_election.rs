use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{Duration, TimeZone, Utc};
use memoryd::dream::lease::{acquire_manual_lease, release_manual_lease, LeaseAcquireRequest, LeaseError};
use memoryd::protocol::LeaseRecord;
use tempfile::TempDir;

#[test]
fn manual_acquire_succeeds_and_commits_only_lease_file_with_fixed_identity() {
    let env = GitLeaseEnv::new("dev_local");

    acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: Some("codex".to_string()),
    })
    .expect("lease acquisition succeeds");

    let changed_files = env.git(["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(changed_files.lines().collect::<Vec<_>>(), ["leases/journal.lease"]);

    assert_eq!(env.git(["log", "-1", "--format=%an <%ae>"]), "memoryd lease-bot <noreply@memoryd.local>");
    assert_eq!(env.git(["log", "-1", "--format=%cn <%ce>"]), "memoryd lease-bot <noreply@memoryd.local>");
    assert_eq!(env.git(["log", "-1", "--format=%s"]), "dream: lease acquire me on dev_local");
}

#[test]
fn active_foreign_lease_returns_lease_held_and_cli_exits_5() {
    let env = GitLeaseEnv::new("dev_local");
    env.append_lease(LeaseRecord {
        device: "dev_foreign".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(1),
        expires_at: Utc::now() + Duration::days(1),
        run_id: "run_foreign".to_string(),
    });
    env.commit_all("seed active lease");

    let err = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect_err("foreign active lease blocks");
    assert!(matches!(err, LeaseError::Held { by_device, .. } if by_device == "dev_foreign"));

    let output =
        env.memoryd(["dream", "now", "--repo", env.repo_str(), "--runtime", env.runtime_str(), "--scope", "me"]);
    assert_eq!(output.status.code(), Some(5));
    assert!(stderr(&output).contains("lease_held"), "stderr was: {}", stderr(&output));
}

#[test]
fn active_same_device_lease_is_reentrant_without_force() {
    let env = GitLeaseEnv::new("dev_local");
    env.append_lease(LeaseRecord {
        device: "dev_local".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(1),
        expires_at: Utc::now() + Duration::days(1),
        run_id: "run_existing_local".to_string(),
    });
    env.commit_all("seed active local lease");

    acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("same-device active lease is re-entrant");

    acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: true,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("--force bypasses an active same-device lease");
}

#[test]
fn local_clean_tree_lease_acquire_succeeds_without_origin() {
    // F2: a local-only repo (no origin remote) grants the lease with zero network —
    // fetch/push no-op, all local lease logic (held-check, dirty guard, local commit)
    // still runs. This is the P0.1 gate: clean-tree lease acquire on a no-remote install.
    let env = GitLeaseEnv::new_without_origin("dev_local");

    let acquired = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("local-only repo (no origin) grants the lease — local-first, no network");

    assert_eq!(acquired.record.device, "dev_local");
    assert_eq!(env.git(["log", "-1", "--format=%s"]), "dream: lease acquire me on dev_local");
    assert_eq!(
        env.git(["show", "--name-only", "--format=", "HEAD"]).lines().collect::<Vec<_>>(),
        ["leases/journal.lease"]
    );
    assert_eq!(env.git(["status", "--short"]), "", "local lease acquire leaves a clean tree");
}

#[test]
fn configured_origin_with_fetch_failure_still_unavailable() {
    // I-F2.4: "no remote by design" is not "broken remote". A *configured* origin
    // whose fetch fails must still surface lease_unavailable — never be silently
    // treated as a local-only install.
    let env = GitLeaseEnv::new("dev_local");
    env.git(["remote", "set-url", "origin", "/nonexistent/memorum-origin.git"]);

    let err = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect_err("a configured-but-broken origin must not be silently treated as no-remote");
    assert!(matches!(err, LeaseError::Unavailable { .. }));
}

#[test]
fn foreign_active_lease_blocks_with_no_remote() {
    // I-F2.3: no-remote mode does not relax held-semantics. A foreign active lease
    // still blocks, exactly as it would with a remote.
    let env = GitLeaseEnv::new_without_origin("dev_local");
    env.append_lease(LeaseRecord {
        device: "dev_foreign".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(1),
        expires_at: fixed_now() + Duration::days(1),
        run_id: "run_foreign".to_string(),
    });

    let err = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect_err("a foreign active lease blocks even with no remote");
    assert!(matches!(err, LeaseError::Held { by_device, .. } if by_device == "dev_foreign"));
}

#[test]
fn stale_self_owned_lease_is_evicted_for_fresh_acquire_with_no_remote() {
    // spec §8.2: with no remote there is no fetch to refresh a stale journal, so a
    // crashed prior run can leave a still-active self-owned lease. Eviction supersedes
    // it and grants a fresh full-window lease rather than reusing one about to expire.
    let env = GitLeaseEnv::new_without_origin("dev_local");
    env.append_lease(LeaseRecord {
        device: "dev_local".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(59),
        expires_at: fixed_now() + Duration::minutes(1), // crashed run, about to expire
        run_id: "run_crashed".to_string(),
    });

    let acquired = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("a stale self-owned lease is evicted and a fresh lease acquired with no remote");

    assert_eq!(acquired.record.device, "dev_local");
    assert_ne!(acquired.record.run_id, "run_crashed", "fresh acquire, not a reuse of the crashed record");
    assert_eq!(
        acquired.record.expires_at,
        fixed_now() + Duration::seconds(3_600),
        "fresh full-window lease, not the about-to-expire crashed one",
    );
    assert_eq!(env.git(["status", "--short"]), "", "eviction + acquire leaves a clean tree");
}

#[test]
fn dirty_tree_with_stale_self_owned_lease_aborts_without_mutating_journal() {
    // The §8.2 eviction must run *after* the dirty-tree gate: a dirty-tree abort
    // must leave the journal byte-identical (no orphan release record), not
    // half-evicted. Regression guard for the eviction-before-dirty-guard bug.
    let env = GitLeaseEnv::new_without_origin("dev_local");
    env.append_lease(LeaseRecord {
        device: "dev_local".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(59),
        expires_at: fixed_now() + Duration::minutes(1),
        run_id: "run_crashed".to_string(),
    });
    let journal_before = std::fs::read_to_string(env.repo.join("leases/journal.lease")).expect("journal before");
    env.write("me/user-work.md", "uncommitted user work\n");

    let err = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect_err("a dirty tree blocks the lease even when a stale self-owned lease would be evicted");
    assert!(matches!(err, LeaseError::DirtyTree { .. }));

    let journal_after = std::fs::read_to_string(env.repo.join("leases/journal.lease")).expect("journal after");
    assert_eq!(journal_before, journal_after, "dirty-tree abort must not append an eviction release record");
}

#[test]
fn push_race_rollback_leaves_no_failed_local_lease_records_or_commits() {
    let env = GitLeaseEnv::new("dev_local");
    env.install_rejecting_origin_hook();
    let original_commit_count = env.commit_count();

    let err = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect_err("persistent push rejection exhausts retries");

    assert!(matches!(err, LeaseError::Unavailable { .. }));
    assert_eq!(env.commit_count(), original_commit_count, "failed lease commits must be rolled back");
    assert_eq!(env.lease_record_count(), 0, "failed local lease records must be removed before returning");
    assert_eq!(env.git(["status", "--short"]), "", "lease rollback must leave the worktree clean");
}

#[test]
fn push_race_retries_three_times_with_fetch_between_attempts_then_unavailable() {
    let mut git = memoryd::dream::git::ScriptedLeaseGit::new()
        .with_fetch_results([Ok(()), Ok(()), Ok(()), Ok(())])
        .with_push_results([
            Err("non-fast-forward".to_string()),
            Err("non-fast-forward".to_string()),
            Err("non-fast-forward".to_string()),
            Err("non-fast-forward".to_string()),
        ]);
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(repo.join("leases")).expect("leases");
    std::fs::create_dir_all(&runtime).expect("runtime");
    std::fs::write(
        runtime.join("local-device.yaml"),
        format!(
            "schema_version: 1\ndevice:\n  id: dev_local\n  name: test\n  shard: test\npaths:\n  memory_root: {}\n  runtime_root: {}\n",
            repo.display(),
            runtime.display()
        ),
    )
    .expect("local device");

    let err = memoryd::dream::lease::acquire_manual_lease_with_git(
        &mut git,
        LeaseAcquireRequest {
            repo,
            runtime,
            scope: "me".to_string(),
            force: false,
            now: fixed_now(),
            lease_window_seconds: 3_600,
            cli_used: None,
        },
    )
    .expect_err("push race exhausts retries");

    assert!(matches!(err, LeaseError::Unavailable { .. }));
    assert_eq!(git.fetch_calls(), 4, "initial fetch plus one fetch before each of three retry attempts");
    assert_eq!(git.push_calls(), 4, "initial push plus three retry attempts");
}

#[test]
fn force_overrides_active_foreign_lease() {
    let env = GitLeaseEnv::new("dev_local");
    env.append_lease(LeaseRecord {
        device: "dev_foreign".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(1),
        expires_at: Utc::now() + Duration::days(1),
        run_id: "run_foreign".to_string(),
    });
    env.commit_all("seed active lease");

    acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: true,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("--force bypasses active foreign lease");

    let lease_text = std::fs::read_to_string(env.repo.join("leases/journal.lease")).expect("lease file");
    assert!(lease_text.contains("\"device\":\"dev_local\""));
}

#[test]
fn explicit_release_leaves_no_active_lease_to_block_later_acquire() {
    let env = GitLeaseEnv::new("dev_local");

    acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("initial lease acquisition succeeds");

    release_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now() + Duration::minutes(1),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("lease release succeeds");

    acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now() + Duration::minutes(2),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("released lease must not block a later acquire");
}

#[test]
fn forced_takeover_makes_forced_holder_active_and_ignores_stale_prior_holder() {
    let env = GitLeaseEnv::new("dev_local");
    env.append_lease(LeaseRecord {
        device: "dev_foreign".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(1),
        expires_at: fixed_now() + Duration::days(1),
        run_id: "run_foreign".to_string(),
    });
    env.commit_all("seed stale active lease");

    acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: true,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("forced takeover succeeds");

    let reacquired = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now() + Duration::minutes(1),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect("re-entrant acquire by dev_local succeeds because the forced takeover made it the active holder; if the stale dev_foreign lease still owned the slot, this acquire would fail with Held { by_device: \"dev_foreign\" }");

    assert_eq!(
        reacquired.record.device, "dev_local",
        "forced takeover made dev_local the active holder; the stale dev_foreign lease is ignored",
    );
}

#[test]
fn dirty_tree_outside_lease_file_returns_lease_dirty_tree_and_does_not_commit_user_work() {
    let env = GitLeaseEnv::new("dev_local");
    env.write("me/user-work.md", "uncommitted user work\n");

    let err = acquire_manual_lease(LeaseAcquireRequest {
        repo: env.repo.clone(),
        runtime: env.runtime.clone(),
        scope: "me".to_string(),
        force: false,
        now: fixed_now(),
        lease_window_seconds: 3_600,
        cli_used: None,
    })
    .expect_err("dirty user tree blocks lease commit");

    assert!(matches!(err, LeaseError::DirtyTree { .. }));
    assert_eq!(env.git(["log", "--oneline"]).lines().count(), 1, "no lease commit should be created");
    assert_eq!(env.git(["status", "--short", "--", "me/user-work.md"]), "?? me/user-work.md");
}

fn fixed_now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 30, 3, 0, 0).single().expect("valid fixed time")
}

struct GitLeaseEnv {
    _temp: TempDir,
    repo: PathBuf,
    runtime: PathBuf,
}

impl GitLeaseEnv {
    fn new(device_id: &str) -> Self {
        let env = Self::new_without_origin(device_id);
        let origin = env._temp.path().join("origin.git");
        command_in(env._temp.path(), "git", ["init", "--bare", origin.to_str().expect("origin path")]);
        env.git(["remote", "add", "origin", origin.to_str().expect("origin path")]);
        env.git(["push", "-u", "origin", "main"]);
        env
    }

    fn new_without_origin(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(repo.join("leases")).expect("leases dir");
        std::fs::create_dir_all(repo.join("me")).expect("me dir");
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
        command_in(&repo, "git", ["add", "leases/journal.lease"]);
        command_in(&repo, "git", ["commit", "-m", "bootstrap"]);
        Self { _temp: temp, repo, runtime }
    }

    fn append_lease(&self, record: LeaseRecord) {
        use std::io::Write;

        let mut file =
            std::fs::OpenOptions::new().append(true).open(self.repo.join("leases/journal.lease")).expect("open lease");
        writeln!(file, "{}", serde_json::to_string(&record).expect("lease serializes")).expect("append lease");
    }

    fn commit_all(&self, message: &str) {
        self.git(["add", "."]);
        self.git(["commit", "-m", message]);
        self.git(["push"]);
    }

    fn install_rejecting_origin_hook(&self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let origin = self.git(["remote", "get-url", "origin"]);
            let hook = PathBuf::from(origin).join("hooks/pre-receive");
            std::fs::write(&hook, "#!/bin/sh\necho rejected for test >&2\nexit 1\n").expect("write hook");
            let mut permissions = std::fs::metadata(&hook).expect("hook metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(hook, permissions).expect("chmod hook");
        }

        #[cfg(not(unix))]
        panic!("rejecting origin hook test requires unix permissions");
    }

    fn commit_count(&self) -> usize {
        self.git(["rev-list", "--count", "HEAD"]).parse().expect("commit count")
    }

    fn lease_record_count(&self) -> usize {
        std::fs::read_to_string(self.repo.join("leases/journal.lease"))
            .expect("lease file")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.repo.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("write");
    }

    fn memoryd<const N: usize>(&self, args: [&str; N]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_memoryd")).args(args).output().expect("memoryd command")
    }

    fn git<const N: usize>(&self, args: [&str; N]) -> String {
        command_in(&self.repo, "git", args)
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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
