use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::task;
use tokio::time;

use crate::recall::error::RecallError;
use crate::recall::types::{ProjectBinding, ProjectBindingSource};

const PROJECT_FILE: &str = ".memory-project.yaml";
const MAX_PROJECT_FIELD_BYTES: usize = 128;
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const GIT_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    canonical_id: String,
    alias: Option<String>,
}

pub async fn resolve_project_binding(cwd: &Path) -> Result<Option<ProjectBinding>, RecallError> {
    if let Some(path) = find_project_file(cwd) {
        return parse_project_file(&path).map(Some);
    }

    let Some(remote) = git_origin_remote(cwd).await else {
        return Ok(None);
    };
    let normalized = normalize_remote(&remote)?;
    let canonical_id = format!("proj_{}", hex::encode(Sha256::digest(normalized.as_bytes())));
    Ok(Some(ProjectBinding { canonical_id, alias: None, resolved_via: ProjectBindingSource::GitRemote }))
}

fn find_project_file(cwd: &Path) -> Option<PathBuf> {
    for ancestor in cwd.ancestors() {
        let candidate = ancestor.join(PROJECT_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn parse_project_file(path: &Path) -> Result<ProjectBinding, RecallError> {
    let yaml = fs::read_to_string(path)
        .map_err(|error| RecallError::invalid_request(format!("failed to read {PROJECT_FILE}: {error}")))?;
    reject_malformed_project_yaml(&yaml)?;
    let project: ProjectFile = serde_yaml::from_str(&yaml)
        .map_err(|error| RecallError::invalid_request(format!("invalid {PROJECT_FILE}: {error}")))?;
    let canonical_id = validate_canonical_id(&project.canonical_id)?;
    let alias = project.alias.as_deref().map(validate_alias).transpose()?;

    Ok(ProjectBinding { canonical_id, alias, resolved_via: ProjectBindingSource::YamlOverride })
}

fn reject_malformed_project_yaml(yaml: &str) -> Result<(), RecallError> {
    if yaml.trim().is_empty() {
        return Err(RecallError::invalid_request(format!("{PROJECT_FILE} must not be empty")));
    }

    let mut keys = HashSet::new();
    let mut saw_field = false;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') || trimmed.starts_with('-') {
            return Err(RecallError::invalid_request(format!("{PROJECT_FILE} must be a flat mapping")));
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            return Err(RecallError::invalid_request(format!("{PROJECT_FILE} must be a mapping")));
        };
        let key = key.trim();
        if key.is_empty() || !matches!(key, "canonical_id" | "alias") {
            return Err(RecallError::invalid_request(format!("unknown {PROJECT_FILE} field: {key}")));
        }
        if !keys.insert(key.to_owned()) {
            return Err(RecallError::invalid_request(format!("duplicate {PROJECT_FILE} field: {key}")));
        }
        reject_unsupported_scalar(value.trim())?;
        saw_field = true;
    }

    if !saw_field {
        return Err(RecallError::invalid_request(format!("{PROJECT_FILE} must be a mapping")));
    }
    Ok(())
}

fn reject_unsupported_scalar(value: &str) -> Result<(), RecallError> {
    if value.is_empty()
        || matches!(value, "true" | "false" | "null" | "~")
        || value.starts_with('[')
        || value.starts_with('{')
        || value.starts_with('&')
        || value.starts_with('*')
        || value.starts_with('!')
        || value.parse::<i64>().is_ok()
        || value.parse::<f64>().is_ok()
    {
        return Err(RecallError::invalid_request(format!("{PROJECT_FILE} fields must be plain string scalars")));
    }
    Ok(())
}

fn validate_canonical_id(value: &str) -> Result<String, RecallError> {
    let trimmed = value.trim();
    if !(3..=MAX_PROJECT_FIELD_BYTES).contains(&trimmed.len()) {
        return Err(RecallError::invalid_request("canonical_id must be 3..=128 bytes"));
    }
    if !trimmed.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) {
        return Err(RecallError::invalid_request(
            "canonical_id must contain only ASCII letters, digits, underscore, or hyphen",
        ));
    }
    Ok(trimmed.to_owned())
}

fn validate_alias(value: &str) -> Result<String, RecallError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PROJECT_FIELD_BYTES {
        return Err(RecallError::invalid_request("alias must be non-empty and at most 128 bytes"));
    }
    Ok(trimmed.to_owned())
}

async fn git_origin_remote(cwd: &Path) -> Option<String> {
    let root = git_worktree_root(cwd).await?;
    let output = git_output(root, ["remote", "get-url", "origin"]).await?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8(output.stdout).ok()?;
    let trimmed = remote.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

async fn git_worktree_root(cwd: &Path) -> Option<PathBuf> {
    let output = git_output(cwd.to_path_buf(), ["rev-parse", "--show-toplevel"]).await?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?;
    Some(PathBuf::from(root.trim()))
}

async fn git_output<const N: usize>(cwd: PathBuf, args: [&'static str; N]) -> Option<Output> {
    let task = task::spawn_blocking(move || {
        let mut command = Command::new("git");
        command.args(args).current_dir(cwd);
        command_output_with_deadline(command, GIT_COMMAND_TIMEOUT)
    });
    time::timeout(GIT_COMMAND_TIMEOUT + GIT_POLL_INTERVAL, task).await.ok()?.ok()?
}

fn command_output_with_deadline(mut command: Command, timeout: Duration) -> Option<Output> {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let deadline = Instant::now() + timeout;

    loop {
        if child.try_wait().ok()?.is_some() {
            return child.wait_with_output().ok();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(GIT_POLL_INTERVAL);
    }
}

fn normalize_remote(remote: &str) -> Result<String, RecallError> {
    let trimmed = remote.trim();
    if let Some(path) = trimmed.strip_prefix("file://") {
        return canonicalize_local_remote(path);
    }
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        return match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" | "git" => Ok(normalize_host_path_url(rest)),
            _ => canonicalize_local_remote(trimmed),
        };
    }
    if let Some(ssh) = normalize_ssh_remote(trimmed) {
        return Ok(ssh);
    }
    canonicalize_local_remote(trimmed)
}

fn normalize_ssh_remote(remote: &str) -> Option<String> {
    let colon = remote.find(':')?;
    let slash = remote.find('/');
    if slash.is_some_and(|slash| slash < colon) {
        return None;
    }
    let host_with_user = &remote[..colon];
    if host_with_user.is_empty() {
        return None;
    }
    let host = host_with_user.rsplit_once('@').map_or(host_with_user, |(_, host)| host);
    Some(normalize_host_and_path(host, &remote[colon + 1..]))
}

fn normalize_host_path_url(rest: &str) -> String {
    let without_user = rest.rsplit_once('@').map_or(rest, |(_, tail)| tail);
    let (host_port, path) = without_user.split_once('/').unwrap_or((without_user, ""));
    let host = host_port.split_once(':').map_or(host_port, |(host, _)| host);
    normalize_host_and_path(host, path)
}

fn normalize_host_and_path(host: &str, path: &str) -> String {
    let host = host.to_ascii_lowercase();
    let path = normalize_remote_path(path);
    if path.is_empty() {
        host
    } else {
        format!("{host}/{path}")
    }
}

fn normalize_remote_path(path: &str) -> String {
    let collapsed = path.split('/').filter(|segment| !segment.is_empty()).collect::<Vec<_>>().join("/");
    let stripped_slash = collapsed.trim_end_matches('/');
    stripped_slash.strip_suffix(".git").unwrap_or(stripped_slash).to_owned()
}

fn canonicalize_local_remote(path: &str) -> Result<String, RecallError> {
    fs::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| RecallError::invalid_request(format!("invalid local git remote path: {error}")))
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::{command_output_with_deadline, normalize_remote_path};

    #[test]
    fn strips_single_git_suffix_after_trailing_slashes() {
        assert_eq!(normalize_remote_path("foo//bar.git/"), "foo/bar");
    }

    #[test]
    fn blocking_command_deadline_returns_before_process_completes() {
        let start = Instant::now();
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 1; echo should-not-print"]);

        let output = command_output_with_deadline(command, Duration::from_millis(50));

        assert!(output.is_none());
        assert!(start.elapsed() < Duration::from_millis(500));
    }
}
