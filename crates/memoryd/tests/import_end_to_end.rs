//! T08 importer integration test against fixture corpora.
//!
//! This file exercises the planning and execution paths together — discovery
//! → parse → project-map → plan → topologically-sort → execute — against
//! on-disk fixture corpora that mirror the real shape of Claude Code and
//! Codex CLI memory directories. The execute phase runs against an in-memory
//! `MockDaemonClient` so the full pipeline can be validated without spinning
//! up a real `memoryd serve` (the DaemonScaffold-backed end-to-end smoke
//! belongs in a separate, slower test fixture).
//!
//! The locked T08 acceptance signals:
//! - First-run import on a combined Claude + Codex fixture set succeeds and
//!   records every source in the state file.
//! - Empty-corpus import exits successfully with a zero-write report.
//! - Re-running on the same fixtures with the previous state file produces
//!   zero new socket writes (idempotency).

use std::path::{Path, PathBuf};

use memoryd::import::pipeline::{
    DaemonClient, ExecuteOptions, ImportEngine, ImportOptions, PlanAction, SupersedeOutcome, SupersedeRequest,
    WriteMemoryOutcome, WriteMemoryRequest,
};
use memoryd::import::project_map::{PromptBackend, PromptResult, PromptedDisposition};
use memoryd::import::state::ImportState;
use memoryd::protocol::GovernanceStatus;
use serial_test::serial;

#[derive(Default)]
struct AlwaysPromote {
    next_id: usize,
    write_calls: usize,
}

impl DaemonClient for AlwaysPromote {
    async fn write_memory(
        &mut self,
        _request: WriteMemoryRequest,
    ) -> memoryd::import::ImportResult<WriteMemoryOutcome> {
        self.write_calls += 1;
        self.next_id += 1;
        Ok(WriteMemoryOutcome {
            status: GovernanceStatus::Promoted,
            id: Some(format!("mem_20260527_a1b2c3d4e5f60718_{:06}", self.next_id)),
            existing_id: None,
            next_actions: Vec::new(),
            reason: None,
        })
    }

    async fn supersede(&mut self, _request: SupersedeRequest) -> memoryd::import::ImportResult<SupersedeOutcome> {
        self.next_id += 1;
        Ok(SupersedeOutcome {
            status: GovernanceStatus::Promoted,
            new_id: Some(format!("mem_20260527_a1b2c3d4e5f60718_{:06}", self.next_id)),
            reason: None,
        })
    }

    async fn get_superseded_by_chain(&mut self, _id: &str) -> memoryd::import::ImportResult<Vec<String>> {
        Ok(Vec::new())
    }

    async fn get_memory(
        &mut self,
        _id: &str,
        _full_body: bool,
    ) -> memoryd::import::ImportResult<Option<memoryd::protocol::GetResponse>> {
        Err(memoryd::import::ImportError::Parse {
            source_key: _id.to_string(),
            reason: "not configured in test double".to_string(),
        })
    }
}

struct DropToMePrompts;
impl PromptBackend for DropToMePrompts {
    fn prompt_non_git_cwd(&mut self, _cwd: &Path, _synced_dir: Option<&'static str>) -> PromptResult {
        PromptResult { disposition: PromptedDisposition::DropToMe, synced_dir_confirmed: None }
    }
}

fn write_file(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(&path, body).expect("write");
    path
}

fn seed_fixture_corpus(claude_root: &Path, codex_root: &Path) {
    // Claude corpus: one single-fact, one user_profile, one MEMORY.md (skipped),
    // one wiki-linked file.
    write_file(
        claude_root,
        "proj/memory/build_commands.md",
        b"---\nname: Build commands\n---\nUse `cargo build --release` for prod builds.\n",
    );
    write_file(
        claude_root,
        "proj/memory/user_profile.md",
        b"---\nname: User profile\n---\nPrefers rust-analyzer over RLS.\n",
    );
    write_file(claude_root, "proj/memory/MEMORY.md", b"# Index\n- build: ./build_commands.md\n");
    write_file(
        claude_root,
        "proj/memory/related.md",
        b"---\nname: Related\n---\nSee [[Build commands]] for the toolchain notes.\n",
    );

    // Codex corpus: two Task Groups plus one ad-hoc note.
    write_file(
        codex_root,
        "MEMORY.md",
        b"\
# Task Group: Atlas onboarding

scope: how new contributors get started on AtlasOS
applies_to: cwd=/Users/u/Code/atlasos; reuse_rule=cwd-scoped

## Task 1: react-doctor flake
react-doctor flakes on cold start; rerun fixes it.

### rollout_summary_files
- cwd=/Users/u/Code/atlasos, rollout_path=/r/x.md, updated_at=2026-05-20T10:00:00Z, thread_id=t-1, outcome=success

### keywords
- atlasos, onboarding, react-doctor

# Task Group: workflow notes

scope: cross-project workflow preferences
applies_to: cwd=unknown; reuse_rule=workflow-scoped

## Task 1: PR template
Use the PR template for non-trivial changes.

### keywords
- workflow, pr-template
",
    );
    write_file(codex_root, "extensions/ad_hoc/notes/preference.md", b"Prefer rustls over openssl for TLS.\n");
}

#[tokio::test]
#[serial]
async fn first_run_imports_all_fixtures_and_records_them_in_state() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = tmp.path().join("memorum");
    let claude_root = tmp.path().join("claude");
    let codex_root = tmp.path().join("codex");
    seed_fixture_corpus(&claude_root, &codex_root);

    let engine = ImportEngine::new(&repo);
    let mut prompts = DropToMePrompts;
    let plan = engine
        .plan(
            ImportOptions {
                from_claude: vec![claude_root.clone()],
                quiet: true,
                from_codex: Some(codex_root.clone()),
                harness_filter: None,
                state: ImportState::default(),
            },
            &mut prompts,
        )
        .await
        .expect("plan");

    // 3 Claude topic files (MEMORY.md skipped) + 2 Codex Task Groups + 1
    // ad-hoc note = 6 candidates total.
    assert_eq!(plan.actions.len(), 6, "actions: {:?}", plan.actions.iter().map(|a| &a.source_key).collect::<Vec<_>>());
    assert!(plan.actions.iter().all(|a| matches!(a.action, PlanAction::WriteNew)));

    let mut client = AlwaysPromote::default();
    let result = engine.execute(plan, ExecuteOptions::default(), &mut client).await.expect("execute");
    assert_eq!(client.write_calls, 6);
    assert_eq!(result.state.imports.len(), 6, "every source recorded in state");

    // State persists on disk for the second-run test.
    let state_path = engine.state_path.clone();
    assert!(state_path.exists(), "canonical state file written");
    let claude = result.report.harnesses.get("claude-code").expect("claude bucket");
    assert_eq!(claude.written_new, 3);
    let codex = result.report.harnesses.get("codex").expect("codex bucket");
    assert_eq!(codex.written_new, 3);
}

#[tokio::test]
#[serial]
async fn empty_corpus_import_produces_zero_writes_and_clean_report() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = tmp.path().join("memorum");
    let claude_root = tmp.path().join("claude_empty");
    let codex_root = tmp.path().join("codex_empty");
    std::fs::create_dir_all(&claude_root).expect("mkdir claude");
    std::fs::create_dir_all(&codex_root).expect("mkdir codex");

    let engine = ImportEngine::new(&repo);
    let mut prompts = DropToMePrompts;
    let plan = engine
        .plan(
            ImportOptions {
                from_claude: vec![claude_root],
                quiet: true,
                from_codex: Some(codex_root),
                harness_filter: None,
                state: ImportState::default(),
            },
            &mut prompts,
        )
        .await
        .expect("plan");
    assert!(plan.actions.is_empty());

    let mut client = AlwaysPromote::default();
    let result = engine.execute(plan, ExecuteOptions::default(), &mut client).await.expect("execute");
    assert_eq!(client.write_calls, 0);
    assert_eq!(result.state.imports.len(), 0);
}

#[tokio::test]
#[serial]
async fn re_run_on_unchanged_fixtures_produces_zero_socket_writes() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = tmp.path().join("memorum");
    let claude_root = tmp.path().join("claude");
    let codex_root = tmp.path().join("codex");
    seed_fixture_corpus(&claude_root, &codex_root);

    let engine = ImportEngine::new(&repo);
    let mut prompts = DropToMePrompts;

    // First run: writes everything.
    let plan = engine
        .plan(
            ImportOptions {
                from_claude: vec![claude_root.clone()],
                quiet: true,
                from_codex: Some(codex_root.clone()),
                harness_filter: None,
                state: ImportState::default(),
            },
            &mut prompts,
        )
        .await
        .expect("first plan");
    let mut client = AlwaysPromote::default();
    let first = engine.execute(plan, ExecuteOptions::default(), &mut client).await.expect("first execute");
    assert_eq!(client.write_calls, 6);

    // Second run: state file loaded from disk; all sources unchanged → all
    // actions are SkipUnchanged → zero socket calls.
    let state_on_disk = ImportState::load(&engine.state_path).expect("load state");
    assert_eq!(state_on_disk.imports.len(), 6, "first run persisted to disk");
    let plan2 = engine
        .plan(
            ImportOptions {
                from_claude: vec![claude_root.clone()],
                quiet: true,
                from_codex: Some(codex_root.clone()),
                harness_filter: None,
                state: state_on_disk,
            },
            &mut prompts,
        )
        .await
        .expect("second plan");
    assert!(plan2.actions.iter().all(|a| matches!(a.action, PlanAction::SkipUnchanged { .. })));

    let mut client2 = AlwaysPromote::default();
    let _result2 = engine.execute(plan2, ExecuteOptions::default(), &mut client2).await.expect("second execute");
    assert_eq!(client2.write_calls, 0, "idempotent re-run never re-writes");
    let _ = first;
}

#[tokio::test]
#[serial]
async fn re_run_with_changed_source_supersedes_prior_memory() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = tmp.path().join("memorum");
    let claude_root = tmp.path().join("claude");
    let codex_root = tmp.path().join("codex");
    seed_fixture_corpus(&claude_root, &codex_root);

    let engine = ImportEngine::new(&repo);
    let mut prompts = DropToMePrompts;

    // First run.
    let plan = engine
        .plan(
            ImportOptions {
                from_claude: vec![claude_root.clone()],
                quiet: true,
                from_codex: Some(codex_root.clone()),
                harness_filter: None,
                state: ImportState::default(),
            },
            &mut prompts,
        )
        .await
        .expect("first plan");
    let mut client = AlwaysPromote::default();
    let _first = engine.execute(plan, ExecuteOptions::default(), &mut client).await.expect("first execute");

    // Mutate one source file.
    std::fs::write(
        claude_root.join("proj/memory/build_commands.md"),
        b"---\nname: Build commands\n---\nUSE `cargo build --release --locked` FOR PROD.\n",
    )
    .expect("mutate source");

    let state_on_disk = ImportState::load(&engine.state_path).expect("load state");
    let plan2 = engine
        .plan(
            ImportOptions {
                from_claude: vec![claude_root.clone()],
                quiet: true,
                from_codex: Some(codex_root.clone()),
                harness_filter: None,
                state: state_on_disk,
            },
            &mut prompts,
        )
        .await
        .expect("second plan");
    let supersede_count = plan2.actions.iter().filter(|a| matches!(a.action, PlanAction::Supersede { .. })).count();
    let skip_count = plan2.actions.iter().filter(|a| matches!(a.action, PlanAction::SkipUnchanged { .. })).count();
    assert_eq!(supersede_count, 1, "one source mutated → one supersede");
    assert_eq!(skip_count, 5, "the other five sources stay unchanged");
}
