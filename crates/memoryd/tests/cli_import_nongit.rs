use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use memoryd::import::pipeline::{
    run_import_session, DaemonClient, ExecuteOptions, HarnessFilter, ImportOptions, SupersedeOutcome, SupersedeRequest,
    WriteMemoryOutcome, WriteMemoryRequest,
};
use memoryd::import::project_map::{FixedDispositionBackend, PromptedDisposition};
use memoryd::import::report::ImportReport;
use memoryd::protocol::GovernanceStatus;
use serial_test::serial;

#[test]
#[serial]
fn absent_non_git_default_skips_and_text_enumerates_cwd() {
    let fixture = ImportFixture::new();
    let report_path = fixture.temp.path().join("report.json");

    let output = run_import([
        "import",
        "--harness",
        "codex",
        "--from-codex",
        path_arg(&fixture.codex_root),
        "--repo",
        path_arg(&fixture.repo),
        "--report",
        path_arg(&report_path),
        "--quiet",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(
        stdout.contains(
            "1 memories skipped (non-git cwd); re-run with --non-git-cwd-default {me|generate} to place them"
        ),
        "skip guidance missing from stdout:\n{stdout}"
    );
    assert!(stdout.contains(&fixture.non_git_cwd.display().to_string()), "cwd not enumerated in stdout:\n{stdout}");

    let report = read_report(&report_path);
    let codex = report.harnesses.get("codex").expect("codex counters");
    assert_eq!(codex.skipped_by_prompt, 1);
    let disposition = only_cwd_disposition(&report);
    assert_eq!(disposition.resolution, "prompted_skip");
    assert_eq!(disposition.cwd.as_deref(), Some(fixture.non_git_cwd.as_path()));
}

#[test]
#[serial]
fn explicit_me_default_maps_non_git_cwd_to_user_scope() {
    let fixture = ImportFixture::new();
    let report_path = fixture.temp.path().join("report.json");

    let output = run_import([
        "import",
        "--harness",
        "codex",
        "--from-codex",
        path_arg(&fixture.codex_root),
        "--repo",
        path_arg(&fixture.repo),
        "--report",
        path_arg(&report_path),
        "--dry-run",
        "--non-git-cwd-default",
        "me",
        "--quiet",
    ]);

    assert_success(&output);
    let report = read_report(&report_path);
    let disposition = only_cwd_disposition(&report);
    assert_eq!(disposition.resolution, "prompted_drop_to_me");
    assert_eq!(disposition.canonical_namespace_id, None);
    assert!(disposition.project_yaml.is_none(), "user-scope mapping must not plan project yaml");
    let codex = report.harnesses.get("codex").expect("codex counters");
    assert_eq!(codex.written_new, 1, "dry-run previews the user-scope write");
}

#[test]
#[serial]
fn generate_default_dry_run_records_planned_yaml_without_writing_file() {
    let fixture = ImportFixture::new();
    let report_path = fixture.temp.path().join("report.json");
    let yaml_path = fixture.non_git_cwd.join(".memory-project.yaml");

    let output = run_import([
        "import",
        "--harness",
        "codex",
        "--from-codex",
        path_arg(&fixture.codex_root),
        "--repo",
        path_arg(&fixture.repo),
        "--report",
        path_arg(&report_path),
        "--dry-run",
        "--non-git-cwd-default",
        "generate",
        "--quiet",
    ]);

    assert_success(&output);
    assert!(!yaml_path.exists(), "dry-run must not write .memory-project.yaml");

    let report = read_report(&report_path);
    let disposition = only_cwd_disposition(&report);
    assert_eq!(disposition.resolution, "prompted_new_project");
    assert!(disposition.canonical_namespace_id.as_deref().is_some_and(|id| id.starts_with("proj_")));
    let project_yaml = disposition.project_yaml.as_ref().expect("planned project yaml entry");
    assert_eq!(project_yaml.path, yaml_path);
    assert_eq!(project_yaml.action, "planned_write");
    assert!(report.project_yaml_writes.is_empty(), "dry-run records no actual yaml writes");
}

#[tokio::test]
#[serial]
async fn generate_default_execute_writes_yaml_and_binds_project_scope() {
    let fixture = ImportFixture::new();
    let yaml_path = fixture.non_git_cwd.join(".memory-project.yaml");

    let mut prompts = FixedDispositionBackend::new(PromptedDisposition::GenerateProjectYaml);
    let mut client = RecordingClient::default();
    let result = run_import_session(
        &fixture.repo,
        ImportOptions {
            from_claude: None,
            from_codex: Some(fixture.codex_root.clone()),
            harness_filter: Some(HarnessFilter::Codex),
            state: None,
            plan_only: false,
        },
        &mut prompts,
        &mut client,
        ExecuteOptions { dry_run: false, verbose_progress: false },
    )
    .await
    .expect("run import session");

    assert!(yaml_path.exists(), "execute mode writes .memory-project.yaml");
    let report = result.report;
    let disposition = only_cwd_disposition(&report);
    let canonical_id = disposition.canonical_namespace_id.clone().expect("project canonical id");
    let project_yaml = disposition.project_yaml.as_ref().expect("project yaml entry");
    assert_eq!(project_yaml.path, yaml_path);
    assert_eq!(project_yaml.action, "written");
    assert!(report.project_yaml_writes.contains(&yaml_path));

    assert_eq!(client.write_calls.len(), 1, "execute issues one daemon write");
    let meta = &client.write_calls[0].meta;
    assert_eq!(meta["namespace"], "project");
    assert_eq!(meta["canonical_namespace_id"], canonical_id);
    let expected_source_ref = fixture.codex_root.join("MEMORY.md").display().to_string();
    assert_eq!(meta["source_ref"].as_str(), Some(expected_source_ref.as_str()));
}

#[test]
fn cli_import_delegates_lock_and_state_to_shared_runner() {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli/import.rs"))
        .expect("read cli import source");
    assert!(source.contains("run_import_session"), "cli import should call the shared runner");
    assert!(!source.contains("ImportLockGuard"), "cli import must not acquire the import lock directly");
    assert!(!source.contains("ImportState::load"), "cli import must not load import state directly");
}

struct ImportFixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
    codex_root: PathBuf,
    non_git_cwd: PathBuf,
}

impl ImportFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("memorum");
        let codex_root = temp.path().join("codex");
        let non_git_cwd = temp.path().join("non-git-project");
        std::fs::create_dir_all(&codex_root).expect("codex root");
        std::fs::create_dir_all(&non_git_cwd).expect("non-git cwd");
        std::fs::write(
            codex_root.join("MEMORY.md"),
            format!(
                "# Task Group: Non-git import fixture\n\n\
scope: cwd-scoped fixture memory\n\
applies_to: cwd={}; reuse_rule=cwd-scoped\n\n\
## Task 1: safe fixture\n\
The importer should place this safe fixture memory according to the non-git cwd disposition.\n\n\
### keywords\n\
- import-nongit, fixture\n",
                non_git_cwd.display()
            ),
        )
        .expect("write MEMORY.md");
        Self { temp, repo, codex_root, non_git_cwd }
    }
}

fn run_import<const N: usize>(args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_memoryd")).args(args).output().expect("run memoryd import")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout utf8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr utf8")
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("test paths are utf8")
}

fn read_report(path: &Path) -> ImportReport {
    let raw = std::fs::read_to_string(path).expect("read report");
    serde_json::from_str(&raw).expect("parse report")
}

fn only_cwd_disposition(report: &ImportReport) -> &memoryd::import::report::CwdDispositionEntry {
    assert_eq!(report.cwd_dispositions.len(), 1, "report dispositions: {:?}", report.cwd_dispositions);
    &report.cwd_dispositions[0]
}

#[derive(Default)]
struct RecordingClient {
    write_calls: Vec<WriteMemoryRequest>,
}

impl DaemonClient for RecordingClient {
    async fn write_memory(&mut self, request: WriteMemoryRequest) -> memoryd::import::ImportResult<WriteMemoryOutcome> {
        self.write_calls.push(request);
        Ok(WriteMemoryOutcome {
            status: GovernanceStatus::Promoted,
            id: Some("mem_20260601_a1b2c3d4e5f60718_000001".to_string()),
            existing_id: None,
            next_actions: Vec::new(),
            reason: None,
        })
    }

    async fn supersede(&mut self, _request: SupersedeRequest) -> memoryd::import::ImportResult<SupersedeOutcome> {
        panic!("test fixture should not supersede")
    }
}
