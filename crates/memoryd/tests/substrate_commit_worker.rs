mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use memory_substrate::git::{commit_substrate_writes, count_substrate_write_changes};
use memory_substrate::tree::bootstrap_repo_tree;
use memory_substrate::{Roots, Substrate};

use common::{spawn_daemon, unique_socket_path, wait_for_socket};

#[tokio::test]
async fn commit_worker_coalesces_burst_into_one_commit() {
    let fixture = Fixture::new("dev_workerburst", 200).await;
    let before = fixture.commit_count();
    let socket = unique_socket_path("f1", "burst");
    let (shutdown_tx, server) = spawn_daemon(&socket, fixture.substrate.clone());
    wait_for_socket(&socket).await;

    fixture.write("me/identity/a.md", "---\nsummary: a\n---\na\n");
    fixture.write("me/identity/b.md", "---\nsummary: b\n---\nb\n");

    fixture.wait_clean();
    common::shutdown(shutdown_tx, server, &socket).await;

    assert_eq!(fixture.commit_count(), before + 1);
    assert_eq!(fixture.git(["log", "-1", "--format=%s"]), "substrate: commit 2 write(s)");
}

#[tokio::test]
async fn commit_worker_never_pushes() {
    let fixture = Fixture::new("dev_workernopush", 50).await;
    let remote = tempfile::tempdir().expect("remote tempdir");
    command(remote.path(), "git", ["init", "--bare"]);
    fixture.git(["remote", "add", "origin", path_arg(remote.path())]);
    fixture.git(["push", "origin", "HEAD:refs/heads/main"]);
    let remote_before = git_dir(remote.path(), ["rev-parse", "refs/heads/main"]);
    let socket = unique_socket_path("f1", "no-push");
    let (shutdown_tx, server) = spawn_daemon(&socket, fixture.substrate.clone());
    wait_for_socket(&socket).await;

    fixture.write("me/identity/local-only.md", "---\nsummary: local\n---\nlocal\n");
    fixture.wait_clean();
    common::shutdown(shutdown_tx, server, &socket).await;

    assert_ne!(fixture.git(["rev-parse", "HEAD"]), remote_before);
    assert_eq!(git_dir(remote.path(), ["rev-parse", "refs/heads/main"]), remote_before);
}

#[tokio::test]
async fn concurrent_worker_and_dream_flush_do_not_corrupt_index() {
    let fixture = Fixture::new("dev_workerlock", 10).await;
    let socket = unique_socket_path("f1", "lock");
    let (shutdown_tx, server) = spawn_daemon(&socket, fixture.substrate.clone());
    wait_for_socket(&socket).await;
    fixture.write("me/identity/worker.md", "---\nsummary: worker\n---\nworker\n");
    let repo = fixture.repo.clone();
    let runtime = fixture.runtime.clone();

    let dream_flush = thread::spawn(move || {
        let _lock = memoryd::substrate_git_lock::acquire_substrate_git_lock(&runtime).expect("dream lock");
        std::fs::create_dir_all(repo.join("agent/playbooks")).expect("agent dir");
        std::fs::write(repo.join("agent/playbooks/dream.md"), "---\nsummary: dream\n---\ndream\n")
            .expect("dream write");
        thread::sleep(Duration::from_millis(100));
        let write_count = count_substrate_write_changes(&repo).expect("status count");
        commit_substrate_writes(&repo, write_count).expect("dream flush");
    });

    dream_flush.join().expect("dream flush thread");
    fixture.wait_clean();
    common::shutdown(shutdown_tx, server, &socket).await;

    assert_eq!(fixture.git(["status", "--porcelain"]), "");
    fixture.git(["fsck", "--no-progress"]);
}

struct Fixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    runtime: PathBuf,
    substrate: Substrate,
}

impl Fixture {
    async fn new(device: &str, debounce_ms: u32) -> Self {
        let temp = tempfile::tempdir().expect("fixture tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        bootstrap_repo_tree(&repo).expect("bootstrap repo");
        std::fs::write(
            repo.join("config.yaml"),
            format!(
                "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\nsubstrate:\n  commit_debounce_ms: {debounce_ms}\n"
            ),
        )
        .expect("config");
        command(&repo, "git", ["init"]);
        commit_substrate_writes(&repo, 1).expect("baseline commit");
        std::fs::create_dir_all(&runtime).expect("runtime");
        std::fs::write(
            runtime.join("local-device.yaml"),
            format!("schema_version: 1\ndevice:\n  id: {device}\n  name: local\n  shard: test\n"),
        )
        .expect("local device");
        let substrate = Substrate::open(Roots::new(repo.clone(), runtime.clone())).await.expect("open substrate");
        let write_count = count_substrate_write_changes(&repo).expect("post-open status");
        if write_count > 0 {
            commit_substrate_writes(&repo, write_count).expect("post-open commit");
        }
        Self { _temp: temp, repo, runtime, substrate }
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.repo.join(relative);
        std::fs::create_dir_all(path.parent().expect("relative path has parent")).expect("parent dir");
        std::fs::write(path, text).expect("write file");
    }

    fn wait_clean(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.git(["status", "--porcelain"]).is_empty() {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("repo did not become clean:\n{}", self.git(["status", "--porcelain"]));
    }

    fn commit_count(&self) -> usize {
        self.git(["rev-list", "--count", "HEAD"]).parse().expect("commit count")
    }

    fn git<const N: usize>(&self, args: [&str; N]) -> String {
        command(&self.repo, "git", args)
    }
}

fn command<const N: usize>(cwd: &Path, program: &str, args: [&str; N]) -> String {
    let output = Command::new(program).args(args).current_dir(cwd).output().expect("command starts");
    assert!(
        output.status.success(),
        "{program} failed in {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_dir<const N: usize>(git_dir: &Path, args: [&str; N]) -> String {
    let mut full_args = vec![format!("--git-dir={}", git_dir.display())];
    full_args.extend(args.into_iter().map(str::to_string));
    let output = Command::new("git").args(full_args).output().expect("git starts");
    assert!(output.status.success(), "git-dir failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("test path is utf8")
}
