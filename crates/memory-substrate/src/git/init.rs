//! Git init.

use std::path::Path;

use crate::error::GitError;
use crate::git::commit::{auto_commit_with_outcome, CommitOutcome};
use crate::git::run_git;
use crate::tree::bootstrap_repo_tree;

/// Initialize repo tree and git config, then commit the bootstrap files.
pub fn init_git_repo(repo: &Path, merge_driver_binary: &Path) -> Result<(), GitError> {
    bootstrap_repo_tree(repo)?;
    if !repo.join(".git").exists() {
        run_git(repo, &["init"])?;
    }
    configure_merge_driver(repo, merge_driver_binary)?;
    run_git(
        repo,
        &["add", ".gitattributes", ".gitignore", "config.yaml", "events/.keep", "policies/.keep", "leases/.keep"],
    )?;
    match auto_commit_with_outcome(repo, "Initialize Stream A memory substrate")? {
        CommitOutcome::NoChanges => {}
        CommitOutcome::Committed { .. } => {}
    }
    Ok(())
}

/// Configure the `memory-merge-driver` git merge driver in the local repo config.
pub fn configure_merge_driver(repo: &Path, merge_driver_binary: &Path) -> Result<(), GitError> {
    run_git(repo, &["config", "merge.memory-merge-driver.name", "Stream A memory merge driver"])?;
    let merge_driver_command = format!(
        "{} --base %O --ours %A --theirs %B --path %P",
        posix_single_quote(&merge_driver_binary.display().to_string())
    );
    run_git(repo, &["config", "merge.memory-merge-driver.driver", &merge_driver_command]).map(|_| ())
}

fn posix_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn posix_single_quote_escapes_embedded_single_quote() {
        assert_eq!(posix_single_quote("/tmp/it's/driver"), "'/tmp/it'\\''s/driver'");
    }

    #[test]
    fn posix_single_quote_wraps_shell_metacharacter_paths() {
        for path in [
            "/tmp/my dir/memory-merge-driver",
            "/tmp/a\"b/driver",
            "/tmp/$(touch pwned)/driver",
            "/tmp/`id`/driver",
            "/tmp/a;rm -rf b/driver",
            "/tmp/x&&y/driver",
            "/tmp/a\\b/driver",
        ] {
            let quoted = posix_single_quote(path);
            assert!(quoted.starts_with('\''), "{path}: {quoted}");
            assert!(quoted.ends_with('\''), "{path}: {quoted}");
            assert_eq!(&quoted[1..quoted.len() - 1], path, "{path}: {quoted}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn posix_single_quote_round_trips_through_shell_without_expansion() {
        for path in [
            "/tmp/my dir/memory-merge-driver",
            "/tmp/it's/driver",
            "/tmp/a\"b/driver",
            "/tmp/$(touch pwned)/driver",
            "/tmp/`id`/driver",
            "/tmp/a;rm -rf b/driver",
            "/tmp/x&&y/driver",
            "/tmp/a\\b/driver",
        ] {
            let temp = match tempfile::tempdir() {
                Ok(temp) => temp,
                Err(err) => panic!("tempdir failed: {err}"),
            };
            let output = match Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("printf '%s' {}", posix_single_quote(path)))
                .current_dir(temp.path())
                .output()
            {
                Ok(output) => output,
                Err(err) => panic!("shell failed: {err}"),
            };

            assert!(output.status.success(), "{path}: {output:?}");
            assert_eq!(String::from_utf8_lossy(&output.stdout), path);
            assert!(!temp.path().join("pwned").exists(), "command substitution ran for {path}");
        }
    }

    #[test]
    fn configure_merge_driver_stores_single_quoted_binary_path() {
        let temp = match tempfile::tempdir() {
            Ok(temp) => temp,
            Err(err) => panic!("tempdir failed: {err}"),
        };
        if let Err(err) = run_git(temp.path(), &["init"]) {
            panic!("git init failed: {err}");
        }
        let merge_driver_binary = Path::new("/tmp/$(touch pwned)/driver");

        if let Err(err) = configure_merge_driver(temp.path(), merge_driver_binary) {
            panic!("configure merge driver failed: {err}");
        }
        let output = match Command::new("git")
            .args(["config", "merge.memory-merge-driver.driver"])
            .current_dir(temp.path())
            .output()
        {
            Ok(output) => output,
            Err(err) => panic!("git config failed: {err}"),
        };

        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "'/tmp/$(touch pwned)/driver' --base %O --ours %A --theirs %B --path %P"
        );
    }
}
