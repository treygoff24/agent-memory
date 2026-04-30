use std::fs;
use std::path::Path;
use std::process::Command;

use memoryd::recall::{validate_startup_request, ProjectBindingSource, RecallError, StartupRequest};

#[test]
fn validates_absolute_cwd_and_trims_session_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let nested = temp.path().join("repo").join("child");
    fs::create_dir_all(&nested).expect("nested");

    let binding = validate_startup_request(request(nested.to_string_lossy(), " sess-1 ", " codex ", Some(" 1.0 ")))
        .expect("valid binding");

    assert_eq!(binding.cwd, canonical(&nested));
    assert_eq!(binding.session_id, "sess-1");
    assert_eq!(binding.harness, "codex");
    assert_eq!(binding.harness_version.as_deref(), Some("1.0"));
}

#[test]
fn rejects_relative_and_missing_cwd() {
    assert_invalid(validate_startup_request(request("relative/path", "sess", "codex", None)));

    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing");
    assert_invalid(validate_startup_request(request(missing.to_string_lossy(), "sess", "codex", None)));
}

#[test]
fn rejects_empty_or_overlong_session_fields_after_trim() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().to_string_lossy();
    let overlong = "x".repeat(129);

    assert_invalid(validate_startup_request(request(&cwd, " ", "codex", None)));
    assert_invalid(validate_startup_request(request(&cwd, &overlong, "codex", None)));
    assert_invalid(validate_startup_request(request(&cwd, "sess", " ", None)));
    assert_invalid(validate_startup_request(request(&cwd, "sess", &overlong, None)));
    assert_invalid(validate_startup_request(request(&cwd, "sess", "codex", Some(&overlong))));
}

#[test]
fn yaml_project_binding_wins_over_git_remote_and_orders_namespaces() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init"]);
    git(&repo, &["remote", "add", "origin", "https://github.com/other/repo.git"]);
    fs::write(repo.join(".memory-project.yaml"), "canonical_id: proj_agent_memory\nalias: agent-memory\n")
        .expect("project yaml");

    let binding =
        validate_startup_request(request(repo.to_string_lossy(), "sess", "codex", None)).expect("valid binding");
    let project = binding.project.expect("project binding");

    assert_eq!(project.canonical_id, "proj_agent_memory");
    assert_eq!(project.alias.as_deref(), Some("agent-memory"));
    assert_eq!(project.resolved_via, ProjectBindingSource::YamlOverride);
    assert_eq!(binding.namespaces_in_scope, ["me", "project:proj_agent_memory", "agent"]);
}

#[test]
fn malformed_project_yaml_fails_closed() {
    let cases = [
        ("", "empty"),
        ("[]", "non-mapping"),
        ("canonical_id: proj_one\ncanonical_id: proj_two\n", "duplicate-key"),
        ("canonical_id: proj_one\nextra: nope\n", "unknown-field"),
        ("canonical_id: true\n", "unsupported-scalar"),
    ];

    for (yaml, name) in cases {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(temp.path().join(".memory-project.yaml"), yaml).expect("project yaml");
        assert_invalid_named(
            validate_startup_request(request(temp.path().to_string_lossy(), "sess", "codex", None)),
            name,
        );
    }
}

#[test]
fn rejects_invalid_yaml_canonical_ids_and_overlong_aliases() {
    let invalid_ids = [
        "",
        "ab",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "proj:bad",
        "proj.bad",
        "proj-é",
    ];

    for canonical_id in invalid_ids {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(temp.path().join(".memory-project.yaml"), format!("canonical_id: {canonical_id:?}\n"))
            .expect("project yaml");
        assert_invalid_named(
            validate_startup_request(request(temp.path().to_string_lossy(), "sess", "codex", None)),
            canonical_id,
        );
    }

    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(
        temp.path().join(".memory-project.yaml"),
        format!("canonical_id: proj_valid\nalias: {}\n", "a".repeat(129)),
    )
    .expect("project yaml");
    assert_invalid(validate_startup_request(request(temp.path().to_string_lossy(), "sess", "codex", None)));
}

#[test]
fn missing_git_remote_is_not_an_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init"]);

    let binding =
        validate_startup_request(request(repo.to_string_lossy(), "sess", "codex", None)).expect("valid binding");

    assert!(binding.project.is_none());
    assert_eq!(binding.namespaces_in_scope, ["me", "agent"]);
}

#[test]
fn git_remote_forms_canonicalize_to_same_project_id() {
    let ssh = binding_for_remote("git@GitHub.com:foo/bar.git");
    let https = binding_for_remote("https://github.com/foo/bar/");
    let git_protocol = binding_for_remote("git://github.com/foo//bar.git");

    assert_eq!(
        ssh.project.as_ref().expect("ssh project").canonical_id,
        https.project.as_ref().expect("https project").canonical_id
    );
    assert_eq!(
        https.project.as_ref().expect("https project").canonical_id,
        git_protocol.project.as_ref().expect("git project").canonical_id
    );
    assert_eq!(ssh.namespaces_in_scope, https.namespaces_in_scope);
}

#[test]
fn file_and_bare_git_remotes_canonicalize_equivalent_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let upstream = temp.path().join("upstream.git");
    fs::create_dir_all(&upstream).expect("upstream");
    let symlink = temp.path().join("upstream-link.git");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&upstream, &symlink).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&upstream, &symlink).expect("symlink");

    let file = binding_for_remote_in(temp.path(), &format!("file://{}", upstream.display()));
    let bare = binding_for_remote_in(temp.path(), &symlink.to_string_lossy());

    assert_eq!(
        file.project.as_ref().expect("file project").canonical_id,
        bare.project.as_ref().expect("bare project").canonical_id
    );
}

fn binding_for_remote(remote: &str) -> memoryd::recall::SessionBinding {
    let temp = tempfile::tempdir().expect("tempdir");
    binding_for_remote_in(temp.path(), remote)
}

fn binding_for_remote_in(parent: &Path, remote: &str) -> memoryd::recall::SessionBinding {
    let repo = parent.join(format!("repo-{}", uuidish(remote)));
    fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init"]);
    git(&repo, &["remote", "add", "origin", remote]);
    validate_startup_request(request(repo.to_string_lossy(), "sess", "codex", None)).expect("valid binding")
}

fn request(cwd: impl AsRef<str>, session_id: &str, harness: &str, harness_version: Option<&str>) -> StartupRequest {
    StartupRequest {
        cwd: cwd.as_ref().to_owned(),
        session_id: session_id.to_owned(),
        harness: harness.to_owned(),
        harness_version: harness_version.map(str::to_owned),
        include_recent: true,
        since_event_id: None,
        budget_tokens: None,
    }
}

fn assert_invalid(result: Result<memoryd::recall::SessionBinding, RecallError>) {
    assert!(matches!(result, Err(RecallError::InvalidRequest { .. })), "expected invalid_request, got {result:?}");
}

fn assert_invalid_named(result: Result<memoryd::recall::SessionBinding, RecallError>, name: &str) {
    assert!(
        matches!(result, Err(RecallError::InvalidRequest { .. })),
        "{name}: expected invalid_request, got {result:?}"
    );
}

fn canonical(path: &Path) -> String {
    path.canonicalize().expect("canonical path").to_string_lossy().into_owned()
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(cwd).status().expect("git command");
    assert!(status.success(), "git {args:?} failed");
}

fn uuidish(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect::<String>()
}
