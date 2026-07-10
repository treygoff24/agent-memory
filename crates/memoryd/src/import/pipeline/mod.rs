//! Importer pipeline — planning (T05) and execution (T06).
//!
//! This module hosts both phases. T05 implements [`ImportEngine::plan`]: source
//! discovery → parse → per-cwd prompts → state-file dedup → topological sort by
//! wiki-link dependency. T06 extends it with [`ImportEngine::execute`] that
//! walks the topo-ordered actions through the daemon socket.
//!
//! The implementation is split across focused submodules — `model` (plain data
//! shapes), `daemon_client` (socket transport), `plan` (planning phase),
//! `execute` (execution phase), and `report_build` (report construction) — but
//! the public surface is re-exported here verbatim, so `import::pipeline::Foo`
//! paths remain valid.

mod daemon_client;
mod execute;
mod model;
mod plan;
mod report_build;

use std::path::{Path, PathBuf};

use crate::import::project_map::PromptBackend;
use crate::import::state::{ImportLockGuard, ImportState};
use crate::import::ImportResult;

pub use daemon_client::{
    DaemonClient, SocketDaemonClient, SupersedeOutcome, SupersedeRequest, WriteMemoryOutcome, WriteMemoryRequest,
};
pub use execute::{ExecuteOptions, ExecuteResult};
pub use model::{
    DiscoverySummary, HarnessFilter, ImportOptions, ImportPlan, PlanAction, PlannedWrite, WikiLinkBackEdge,
};

/// Top-level importer engine. Owns the in-memory plan state across the
/// `plan()` + `execute()` calls.
pub struct ImportEngine {
    /// State file path on disk. Persisted between runs at
    /// `$MEMORUM_REPO/.memorum/import-state.json`.
    pub state_path: PathBuf,
}

impl ImportEngine {
    /// Build an engine pointed at the conventional state-file path.
    pub fn new(repo_root: &Path) -> Self {
        Self { state_path: repo_root.join(".memorum").join("import-state.json") }
    }
}

/// Run a complete disk-backed import session behind the importer invariants:
/// acquire the import lock, load state, plan, execute, and let execution perform
/// crash-safe state persistence. CLI and setup-engine callers should use this
/// runner instead of hand-rolling lock/state plumbing.
#[expect(clippy::too_many_arguments, reason = "task contract keeps session dependencies explicit")]
pub async fn run_import_session<C: DaemonClient>(
    repo_root: &Path,
    mut options: ImportOptions,
    prompts: &mut dyn PromptBackend,
    client: &mut C,
    execute_options: ExecuteOptions,
) -> ImportResult<ExecuteResult> {
    let engine = ImportEngine::new(repo_root);
    // A dry-run plans and counts without issuing daemon writes or persisting
    // state, so it must not mutate disk. `ImportLockGuard::acquire` creates
    // `.memorum/` plus the lock/pid files *before* `execute` consults `dry_run`,
    // so only take the lock for a real run. Loading state is read-only (missing
    // file → default), so it is safe either way and keeps the dry-run plan
    // idempotent against already-imported sources.
    let _lock = if execute_options.dry_run { None } else { Some(ImportLockGuard::acquire(&engine.state_path)?) };
    options.state = ImportState::load(&engine.state_path)?;
    // Planning never writes `.memory-project.yaml`; `execute` is what gates the
    // materialization on `!dry_run`, so the same plan is correct for both modes.
    let plan = engine.plan(options, prompts).await?;
    engine.execute(plan, execute_options, client).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::candidate::Harness;
    use crate::import::project_map::{PromptResult, PromptedDisposition};
    use crate::import::state::ImportRecord;
    use chrono::Utc;

    // Cross-submodule items the tests reach through `super::*`. These are
    // private to the pipeline module (test-only or planner/execute internals),
    // so import them explicitly rather than re-exporting them publicly.
    use super::daemon_client::{daemon_protocol_error, unexpected_daemon_payload};
    use super::execute::{bucket_repair_action, plan_action_for_record};
    use super::plan::topo_sort;

    use std::collections::BTreeMap;

    use serde_json::Value;

    use crate::import::project_map::{ResolutionKind, ScopeBinding};
    use crate::protocol::{GetResponse, GovernanceRefusalReason, GovernanceStatus, MemoryStatus, ProtocolError, ResponsePayload};

    use crate::import::candidate::ParsedMemory;
    use crate::import::report::ImportReport;
    use crate::import::ImportError;

    fn empty_plan() -> ImportPlan {
        ImportPlan {
            actions: Vec::new(),
            source_discovery_summary: DiscoverySummary::default(),
            unresolved_back_edges: Vec::new(),
            parse_errors: Vec::new(),
            frontmatter_recovered: Vec::new(),
            claude_roots_used: Vec::new(),
            state: ImportState::default(),
        }
    }

    #[test]
    fn from_plan_initialises_zero_counters_per_harness() {
        let report = ImportReport::from_plan(&empty_plan());
        assert_eq!(report.harnesses.get("claude-code").map(|c| c.parsed), Some(0));
        assert_eq!(report.harnesses.get("codex").map(|c| c.parsed), Some(0));
    }

    struct NoPrompts;
    impl PromptBackend for NoPrompts {
        fn prompt_non_git_cwd(&mut self, _cwd: &Path, _synced_dir: Option<&'static str>) -> PromptResult {
            PromptResult { disposition: PromptedDisposition::DropToMe, synced_dir_confirmed: None }
        }
    }

    fn make_candidate(source_key: &str, body: &str, wiki_links: Vec<String>) -> ParsedMemory {
        let mut hint = BTreeMap::new();
        hint.insert("name".to_string(), serde_json::Value::String(source_key.to_string()));
        let content_hash = ParsedMemory::compute_content_hash(&hint, body);
        ParsedMemory {
            source_key: source_key.to_string(),
            source_path: PathBuf::from(format!("/fixture/{source_key}")),
            content_hash,
            harness: Harness::ClaudeCode,
            frontmatter_hint: hint,
            body: body.to_string(),
            wiki_links,
            cwd: None,
            title: Some(source_key.to_string()),
            section_disambiguation: None,
        }
    }

    fn make_planned(source_key: &str, body: &str, wiki_links: Vec<String>, action: PlanAction) -> PlannedWrite {
        PlannedWrite {
            source_key: source_key.to_string(),
            candidate: make_candidate(source_key, body, wiki_links),
            scope: ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                namespace_alias: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            },
            action,
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        }
    }

    fn import_record(
        memory_id: &str,
        content_hash: &str,
        namespace: Option<&str>,
        canonical_namespace_id: Option<&str>,
    ) -> ImportRecord {
        ImportRecord {
            source_identity: String::new(),
            source_key: String::new(),
            source_memory_id: None,
            memory_id: memory_id.to_string(),
            content_hash: content_hash.to_string(),
            imported_at: Utc::now(),
            harness: "claude-code".to_string(),
            source_path_at_import: PathBuf::from("ignored"),
            namespace: namespace.map(str::to_string),
            canonical_namespace_id: canonical_namespace_id.map(str::to_string),
            aliases: Vec::new(),
            supersession_chain: Vec::new(),
        }
    }

    fn project_scope(namespace_alias: &str, canonical_namespace_id: &str) -> ScopeBinding {
        ScopeBinding {
            scope: memory_substrate::Scope::Project,
            namespace: Some("project".to_string()),
            namespace_alias: Some(namespace_alias.to_string()),
            canonical_namespace_id: Some(canonical_namespace_id.to_string()),
            resolution: ResolutionKind::YamlOverride,
            project_yaml: None,
        }
    }

    #[test]
    fn unchanged_project_import_with_matching_bucket_skips_idempotently() {
        let record = import_record("mem_existing", "sha256:same", Some("policy"), Some("proj_policy-c6698817853503be"));
        let action =
            plan_action_for_record(&record, "record_key", "sha256:same", &project_scope("policy", "proj_policy-c6698817853503be"));
        assert!(
            matches!(action, PlanAction::SkipUnchanged { existing_memory_id, .. } if existing_memory_id == "mem_existing")
        );
    }

    #[test]
    fn unchanged_project_import_with_different_bucket_repairs_instead_of_skipping() {
        let record = import_record("mem_wrong_bucket", "sha256:same", Some("agent-memory"), Some("proj_agent-memory"));
        let action =
            plan_action_for_record(&record, "record_key", "sha256:same", &project_scope("policy", "proj_policy-c6698817853503be"));
        assert!(
            matches!(action, PlanAction::RepairBucket { prior_memory_id, prior_content_hash, .. } if prior_memory_id == "mem_wrong_bucket" && prior_content_hash == "sha256:same")
        );
    }

    #[test]
    fn daemon_protocol_error_keeps_code_message_and_retryability() {
        let error = daemon_protocol_error(
            "WriteMemory",
            ProtocolError {
                code: "invalid_request".to_string(),
                message: "title is required".to_string(),
                retryable: false,
            },
        );

        let ImportError::Parse { source_key, reason } = error else {
            panic!("daemon errors should stay reportable parse-style failures");
        };
        assert_eq!(source_key, "<daemon>");
        assert!(reason.contains("WriteMemory"));
        assert!(reason.contains("invalid_request"));
        assert!(reason.contains("title is required"));
        assert!(reason.contains("retryable=false"));
    }

    #[test]
    fn unexpected_daemon_payload_names_operation_and_payload() {
        let error = unexpected_daemon_payload(
            "Supersede",
            &ResponsePayload::Status(crate::protocol::StatusResponse::default()),
        );

        let ImportError::Parse { reason, .. } = error else {
            panic!("unexpected payloads should stay reportable parse-style failures");
        };
        assert!(reason.contains("Supersede"));
        assert!(reason.contains("Status"));
    }

    #[test]
    fn unexpected_daemon_payload_omits_payload_body_content() {
        let error = unexpected_daemon_payload(
            "WriteMemory",
            &ResponsePayload::Get(crate::protocol::GetResponse {
                id: "mem-secret".to_string(),
                summary: "private summary".to_string(),
                body: "SECRET MEMORY BODY".to_string(),
                truncated: false,
                provenance: None,
                sensitivity: None,
                status: None,
                guidance: "private guidance".to_string(),
            }),
        );

        let ImportError::Parse { reason, .. } = error else {
            panic!("unexpected payloads should stay reportable parse-style failures");
        };
        assert!(reason.contains("WriteMemory"));
        assert!(reason.contains("Get"));
        assert!(!reason.contains("SECRET MEMORY BODY"));
        assert!(!reason.contains("private summary"));
        assert!(!reason.contains("private guidance"));
    }

    #[test]
    fn topo_sort_orders_a_to_b_to_c_as_c_b_a() {
        // A links to B, B links to C; so we should write C first, then B, then A.
        let actions = vec![
            make_planned("a", "see [[b]]", vec!["b".to_string()], PlanAction::WriteNew),
            make_planned("b", "see [[c]]", vec!["c".to_string()], PlanAction::WriteNew),
            make_planned("c", "leaf", Vec::new(), PlanAction::WriteNew),
        ];
        let (sorted, back_edges) = topo_sort(actions);
        let order: Vec<&str> = sorted.iter().map(|w| w.source_key.as_str()).collect();
        assert_eq!(order, vec!["c", "b", "a"]);
        assert!(back_edges.is_empty(), "no back-edges in a tree");
        let a = sorted.iter().find(|w| w.source_key == "a").unwrap();
        assert_eq!(a.wiki_link_targets_resolvable, vec!["b".to_string()]);
    }

    #[test]
    fn topo_sort_breaks_cycle_at_higher_source_key_outgoing_edge() {
        // A→B→A cycle. The deterministic break rule (in topo_sort) cuts the
        // outgoing edge from the higher source-key node (`b → a`), leaving
        // `a → b` as the only remaining forward edge. So `b` writes first
        // (leaf in the DAG after the cut), `a` writes second carrying
        // `related: [b]`, and `b`'s body keeps `[[a]]` as inert text.
        let actions = vec![
            make_planned("a", "see [[b]]", vec!["b".to_string()], PlanAction::WriteNew),
            make_planned("b", "see [[a]]", vec!["a".to_string()], PlanAction::WriteNew),
        ];
        let (sorted, back_edges) = topo_sort(actions);
        let order: Vec<&str> = sorted.iter().map(|w| w.source_key.as_str()).collect();
        assert_eq!(order, vec!["b", "a"]);
        assert_eq!(back_edges.len(), 1);
        assert_eq!(back_edges[0].source_key, "b");
        assert_eq!(back_edges[0].alias, "a");
        let a = sorted.iter().find(|w| w.source_key == "a").unwrap();
        assert_eq!(a.wiki_link_targets_resolvable, vec!["b".to_string()]);
        let b = sorted.iter().find(|w| w.source_key == "b").unwrap();
        assert_eq!(b.wiki_link_targets_back_edge, vec!["a".to_string()]);
    }

    #[test]
    fn topo_sort_unresolvable_alias_stays_inert_not_back_edge() {
        let actions = vec![make_planned("a", "see [[missing]]", vec!["missing".to_string()], PlanAction::WriteNew)];
        let (sorted, back_edges) = topo_sort(actions);
        assert_eq!(sorted.len(), 1);
        assert!(sorted[0].wiki_link_targets_resolvable.is_empty(), "unresolved alias is not a forward link");
        assert!(sorted[0].wiki_link_targets_back_edge.is_empty(), "unresolved alias is not a back-edge");
        assert!(back_edges.is_empty(), "unresolved aliases are inert body text");
    }

    #[tokio::test]
    async fn first_run_produces_all_write_new_actions() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        std::fs::write(claude_root.join("proj/memory/a.md"), b"---\nname: A\n---\nbody a\n").expect("write");
        std::fs::write(claude_root.join("proj/memory/b.md"), b"---\nname: B\n---\nbody b\n").expect("write");

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: vec![claude_root],
                    from_codex: Some(PathBuf::from("/does/not/exist")),
                    harness_filter: None,
                    quiet: true,
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("plan ok");
        assert_eq!(plan.actions.len(), 2);
        for action in &plan.actions {
            assert!(matches!(action.action, PlanAction::WriteNew));
        }
    }

    #[tokio::test]
    async fn second_run_with_unchanged_content_produces_skip_unchanged() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        let body = b"---\nname: A\n---\nbody a\n";
        std::fs::write(claude_root.join("proj/memory/a.md"), body).expect("write");

        // Construct a state record with the matching content hash.
        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let first_plan = engine
            .plan(
                ImportOptions {
                    from_claude: vec![claude_root.clone()],
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: None,
                    quiet: true,
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("first plan");
        let source_key = first_plan.actions[0].source_key.clone();
        let content_hash = first_plan.actions[0].candidate.content_hash.clone();

        let mut state = ImportState::default();
        state.imports.insert(
            source_key.clone(),
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_existing".to_string(),
                content_hash,
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("ignored"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );

        let mut prompts2 = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: vec![claude_root],
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: None,
                    quiet: true,
                    state,
                },
                &mut prompts2,
            )
            .await
            .expect("second plan");
        assert_eq!(plan.actions.len(), 1);
        assert!(
            matches!(plan.actions[0].action, PlanAction::SkipUnchanged { ref existing_memory_id, .. } if existing_memory_id == "mem_existing")
        );
    }

    #[tokio::test]
    async fn second_run_with_unchanged_content_but_wrong_project_bucket_repairs_bucket() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        let project = tmp.path().join("policy");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        std::fs::write(claude_root.join("proj/memory/a.md"), b"---\nname: A\n---\nbody a\n").expect("write");

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let first_plan = engine
            .plan(
                ImportOptions {
                    from_claude: vec![claude_root.clone()],
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: None,
                    quiet: true,
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("first plan");
        let source_key = first_plan.actions[0].source_key.clone();
        let content_hash = first_plan.actions[0].candidate.content_hash.clone();

        let mut state = ImportState::default();
        state.imports.insert(
            source_key,
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_wrong_bucket".to_string(),
                content_hash,
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("ignored"),
                namespace: Some("agent-memory".to_string()),
                canonical_namespace_id: Some("proj_agent-memory".to_string()),
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );

        let mut candidate = first_plan.actions[0].candidate.clone();
        candidate.cwd = Some(project);
        let action = PlannedWrite {
            source_key: candidate.source_key.clone(),
            candidate,
            scope: ScopeBinding {
                scope: memory_substrate::Scope::Project,
                namespace: Some("project".to_string()),
                namespace_alias: Some("policy".to_string()),
                canonical_namespace_id: Some("proj_policy-c6698817853503be".to_string()),
                resolution: ResolutionKind::YamlOverride,
                project_yaml: None,
            },
            action: bucket_repair_action(
                state.imports.get("claude:proj/memory/a.md").expect("seeded record"),
            ),
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        };

        assert!(
            matches!(action.action, PlanAction::RepairBucket { ref prior_memory_id, .. } if prior_memory_id == "mem_wrong_bucket")
        );

        let mut client = MockDaemonClient::default().push_supersede(SupersedeOutcome {
            status: GovernanceStatus::Promoted,
            new_id: Some("mem_rebucketed".to_string()),
            reason: None,
        });
        let result = engine
            .execute(
                ImportPlan {
                    actions: vec![action],
                    source_discovery_summary: DiscoverySummary::default(),
                    unresolved_back_edges: Vec::new(),
                    parse_errors: Vec::new(),
                    frontmatter_recovered: Vec::new(),
                    claude_roots_used: Vec::new(),
                    state,
                },
                ExecuteOptions::default(),
                &mut client,
            )
            .await
            .expect("repair execute ok");

        assert!(client.write_calls.is_empty(), "bucket repair must not get swallowed by duplicate detection");
        assert_eq!(client.supersede_calls.len(), 1);
        assert_eq!(client.supersede_calls[0].old_id, "mem_wrong_bucket");
        assert_eq!(client.supersede_calls[0].meta.get("namespace_alias").and_then(Value::as_str), Some("policy"));
        assert_eq!(
            client.supersede_calls[0].meta.get("canonical_namespace_id").and_then(Value::as_str),
            Some("proj_policy-c6698817853503be")
        );
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.superseded, 1);
        let record = result.state.imports.values().find(|r| r.memory_id == "mem_rebucketed").expect("state record");
        assert_eq!(record.namespace.as_deref(), Some("policy"));
        assert_eq!(record.canonical_namespace_id.as_deref(), Some("proj_policy-c6698817853503be"));
    }

    #[tokio::test]
    async fn second_run_with_changed_content_produces_supersede() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        std::fs::write(claude_root.join("proj/memory/a.md"), b"---\nname: A\n---\nNEW body\n").expect("write");

        let mut state = ImportState::default();
        // The candidate source key will be "claude:proj/memory/a.md"; we
        // record a stale hash.
        state.imports.insert(
            "claude:proj/memory/a.md".to_string(),
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_old".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("ignored"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: vec![claude_root],
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: None,
                    quiet: true,
                    state,
                },
                &mut prompts,
            )
            .await
            .expect("plan");
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0].action {
            PlanAction::Supersede { prior_memory_id, prior_content_hash } => {
                assert_eq!(prior_memory_id, "mem_old");
                assert_eq!(prior_content_hash, "sha256:STALE");
            }
            other => panic!("expected Supersede, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn plan_matches_codex_memory_after_task_group_renumbering() {
        let tmp = tempfile::tempdir().expect("tmp");
        let codex_root = tmp.path().join("codex");
        std::fs::create_dir_all(&codex_root).expect("mkdir");
        // Old run: "Foo" was the first task group. New run: "Foo" is second.
        std::fs::write(
            codex_root.join("MEMORY.md"),
            b"# Task Group: Atlas\n\nscope: how\n\nbody atlas\n\n# Task Group: Foo\n\nscope: what\n\nbody foo\n",
        )
        .expect("write");

        let parse_output = crate::import::sources::codex::parse(&codex_root).expect("parse");
        assert!(parse_output.errors.is_empty(), "parse errors: {:?}", parse_output.errors);
        let foo = parse_output
            .candidates
            .iter()
            .find(|c| c.source_key == "codex:memories/MEMORY.md#task-group-2-foo")
            .unwrap_or_else(|| panic!("foo candidate not in {:?}", parse_output.candidates.iter().map(|c| &c.source_key).collect::<Vec<_>>()));

        let mut state = ImportState::default();
        state.imports.insert(
            "codex:memories/MEMORY.md#task-group-1-foo".to_string(),
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_foo".to_string(),
                content_hash: foo.content_hash.clone(),
                imported_at: Utc::now(),
                harness: "codex".to_string(),
                source_path_at_import: codex_root.join("MEMORY.md"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: Vec::new(),
                    from_codex: Some(codex_root),
                    harness_filter: None,
                    quiet: true,
                    state,
                },
                &mut prompts,
            )
            .await
            .expect("plan");

        let foo_action = plan
            .actions
            .iter()
            .find(|a| a.source_key == "codex:memories/MEMORY.md#task-group-2-foo")
            .expect("foo action");
        assert!(
            matches!(foo_action.action, PlanAction::SkipUnchanged { ref existing_memory_id, .. } if existing_memory_id == "mem_foo"),
            "renumbered task group should still match by ordinal-free identity: {:?}",
            foo_action.action
        );
    }

    #[tokio::test]
    async fn harness_filter_excludes_other_harness() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        std::fs::write(claude_root.join("proj/memory/a.md"), b"---\nname: A\n---\nbody\n").expect("write");

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: vec![claude_root],
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: Some(HarnessFilter::Codex),
                    quiet: true,
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("plan");
        assert!(plan.actions.is_empty(), "Claude root not scanned under --harness codex");
        assert!(plan.source_discovery_summary.claude_root.is_none());
    }

    // T06: execute-phase tests. The MockDaemonClient lets each test script the
    // daemon's response per request, so we can exercise the branching matrix
    // (status × existing_id × next_actions) and assert the importer's bookkeeping
    // without spinning up a real daemon.

    #[derive(Debug, Default)]
    struct MockDaemonClient {
        write_responses: std::collections::VecDeque<WriteMemoryOutcome>,
        supersede_responses: std::collections::VecDeque<SupersedeOutcome>,
        get_responses: std::collections::HashMap<String, crate::protocol::GetResponse>,
        superseded_by_chains: std::collections::HashMap<String, Vec<String>>,
        write_calls: Vec<MockWriteCall>,
        supersede_calls: Vec<MockSupersedeCall>,
        get_calls: Vec<String>,
        trust_artifact_calls: Vec<String>,
    }

    #[derive(Debug, Clone)]
    struct MockWriteCall {
        #[allow(dead_code)]
        body: String,
        #[allow(dead_code)]
        title: Option<String>,
        meta: Value,
    }

    #[derive(Debug, Clone)]
    struct MockSupersedeCall {
        old_id: String,
        #[allow(dead_code)]
        content: String,
        #[allow(dead_code)]
        meta: Value,
    }

    impl MockDaemonClient {
        fn push_write(mut self, outcome: WriteMemoryOutcome) -> Self {
            self.write_responses.push_back(outcome);
            self
        }
        fn push_supersede(mut self, outcome: SupersedeOutcome) -> Self {
            self.supersede_responses.push_back(outcome);
            self
        }
        fn with_get_response(mut self, id: &str, response: crate::protocol::GetResponse) -> Self {
            self.get_responses.insert(id.to_string(), response);
            self
        }
        fn with_superseded_by_chain(mut self, id: &str, chain: Vec<String>) -> Self {
            self.superseded_by_chains.insert(id.to_string(), chain);
            self
        }
    }

    impl DaemonClient for MockDaemonClient {
        async fn write_memory(&mut self, request: WriteMemoryRequest) -> ImportResult<WriteMemoryOutcome> {
            self.write_calls.push(MockWriteCall {
                body: request.body.clone(),
                title: request.title.clone(),
                meta: request.meta.clone(),
            });
            Ok(self.write_responses.pop_front().unwrap_or(WriteMemoryOutcome {
                status: GovernanceStatus::Promoted,
                id: Some(format!("mem_mock_{:04}", self.write_calls.len())),
                existing_id: None,
                next_actions: Vec::new(),
                reason: None,
            }))
        }

        async fn supersede(&mut self, request: SupersedeRequest) -> ImportResult<SupersedeOutcome> {
            self.supersede_calls.push(MockSupersedeCall {
                old_id: request.old_id,
                content: request.content,
                meta: request.meta,
            });
            Ok(self.supersede_responses.pop_front().unwrap_or(SupersedeOutcome {
                status: GovernanceStatus::Promoted,
                new_id: Some("mem_supersede".to_string()),
                reason: None,
            }))
        }

        async fn get_superseded_by_chain(&mut self, id: &str) -> ImportResult<Vec<String>> {
            // Model the same transitive walk the real SocketDaemonClient performs.
            let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
            let mut chain: Vec<String> = Vec::new();
            queue.push_back(id.to_string());
            visited.insert(id.to_string());

            while let Some(current_id) = queue.pop_front() {
                if chain.len() >= 16 {
                    break;
                }
                self.trust_artifact_calls.push(current_id.clone());
                for next_id in self.superseded_by_chains.get(&current_id).cloned().unwrap_or_default() {
                    if visited.insert(next_id.clone()) {
                        chain.push(next_id.clone());
                        queue.push_back(next_id);
                    }
                }
            }

            Ok(chain)
        }

        async fn get_memory(&mut self, id: &str, full_body: bool) -> ImportResult<crate::protocol::GetResponse> {
            self.get_calls.push(id.to_string());
            let mut response = self.get_responses.get(id).cloned().ok_or_else(|| ImportError::Parse {
                source_key: id.to_string(),
                reason: "mock get response not configured".to_string(),
            })?;
            if !full_body && response.body.chars().count() > 4_096 {
                response.body = response.body.chars().take(4_096).collect();
                response.truncated = true;
            }
            Ok(response)
        }
    }

    fn plan_with_actions(actions: Vec<PlannedWrite>) -> ImportPlan {
        ImportPlan {
            actions,
            source_discovery_summary: DiscoverySummary::default(),
            unresolved_back_edges: Vec::new(),
            parse_errors: Vec::new(),
            frontmatter_recovered: Vec::new(),
            claude_roots_used: Vec::new(),
            state: ImportState::default(),
        }
    }

    /// A dry-run session must not acquire the import lock, so it never creates
    /// the `.memorum/` substrate dir, the `<state>.json.lock`, or `import.pid`.
    #[tokio::test]
    async fn dry_run_session_creates_no_lock_or_substrate() {
        let tmp = tempfile::tempdir().expect("tmp");
        let repo = tmp.path().join("repo");
        let mut client = MockDaemonClient::default();
        let mut prompts = NoPrompts;

        let result = run_import_session(
            &repo,
            ImportOptions {
                from_claude: Vec::new(),
                from_codex: None,
                harness_filter: None,
                quiet: true,
                state: ImportState::default(),
            },
            &mut prompts,
            &mut client,
            ExecuteOptions { dry_run: true, verbose_progress: false },
        )
        .await
        .expect("dry-run session succeeds");

        assert!(result.report.refusals.is_empty(), "no corpus, no refusals");
        assert!(client.write_calls.is_empty(), "dry-run issues no daemon writes");
        assert!(!repo.join(".memorum").exists(), "dry-run must not create the .memorum substrate dir");
        let state_path = ImportEngine::new(&repo).state_path;
        assert!(!state_path.with_extension("json.lock").exists(), "dry-run must not create the lock file");
        assert!(!repo.join(".memorum").join("import.pid").exists(), "dry-run must not create the pid file");
    }

    #[tokio::test]
    async fn execute_records_promoted_id_and_increments_written_new() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Promoted,
            id: Some("mem_abc".to_string()),
            existing_id: None,
            next_actions: Vec::new(),
            reason: None,
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("claude bucket");
        assert_eq!(claude.written_new, 1);
        assert_eq!(claude.refused_other, 0);
        assert_eq!(result.state.imports.len(), 1);
        assert_eq!(result.state.imports.values().next().map(|r| r.memory_id.clone()), Some("mem_abc".to_string()));
        assert_eq!(client.write_calls.len(), 1, "single socket call");
    }

    #[tokio::test]
    async fn execute_promoted_with_existing_id_counts_as_dedup_not_new() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Promoted,
            id: None,
            existing_id: Some("mem_existing".to_string()),
            next_actions: Vec::new(),
            reason: None,
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.dedup_existing, 1);
        assert_eq!(claude.written_new, 0);
        assert_eq!(result.report.dedups.len(), 1);
        let record = result.state.imports.values().find(|r| r.memory_id == "mem_existing").expect("state record");
        assert_eq!(record.source_key, "a");
    }

    #[tokio::test]
    async fn execute_candidate_with_supersede_next_action_issues_followup_supersede() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default()
            .push_write(WriteMemoryOutcome {
                status: GovernanceStatus::Candidate,
                id: None,
                existing_id: Some("mem_prior".to_string()),
                next_actions: vec!["memory_supersede".to_string()],
                reason: None,
            })
            .push_supersede(SupersedeOutcome {
                status: GovernanceStatus::Promoted,
                new_id: Some("mem_new".to_string()),
                reason: None,
            });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        assert_eq!(client.supersede_calls.len(), 1);
        assert_eq!(client.supersede_calls[0].old_id, "mem_prior");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.superseded, 1);
        let record = result.state.imports.values().find(|r| r.memory_id == "mem_new").expect("state record");
        assert_eq!(record.source_key, "a");
        assert_eq!(record.supersession_chain.len(), 0, "supersession chain only carries prior state-file records");
    }

    #[tokio::test]
    async fn execute_supersede_chain_adopts_existing_replacement_without_duplicate_write() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        // Candidate content is "replacement"; the daemon's supersession chain
        // already has a memory with that same body.
        let candidate = make_candidate("a", "replacement", Vec::new());
        let mut state = ImportState::default();
        state.imports.insert(
            "a".to_string(),
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_prior".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("/fixture/a"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );
        let action = PlannedWrite {
            source_key: candidate.source_key.clone(),
            candidate,
            scope: ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                namespace_alias: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            },
            action: PlanAction::Supersede {
                prior_memory_id: "mem_prior".to_string(),
                prior_content_hash: "sha256:STALE".to_string(),
            },
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        };

        let mut client = MockDaemonClient::default()
            .with_superseded_by_chain("mem_prior", vec!["mem_new".to_string()])
            .with_get_response(
                "mem_new",
                GetResponse {
                    id: "mem_new".to_string(),
                    summary: "replacement".to_string(),
                    body: "replacement".to_string(),
                    truncated: false,
                    provenance: None,
                    status: Some(MemoryStatus::Active),
                    guidance: String::new(),
                },
            );

        let result = engine
            .execute(
                ImportPlan {
                    actions: vec![action],
                    source_discovery_summary: DiscoverySummary::default(),
                    unresolved_back_edges: Vec::new(),
                    parse_errors: Vec::new(),
                    frontmatter_recovered: Vec::new(),
                    claude_roots_used: Vec::new(),
                    state,
                },
                ExecuteOptions::default(),
                &mut client,
            )
            .await
            .expect("execute ok");

        assert!(client.supersede_calls.is_empty(), "crash-mitigation must not re-issue supersede");
        assert!(client.get_calls.contains(&"mem_new".to_string()));
        assert!(client.trust_artifact_calls.contains(&"mem_prior".to_string()));
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.superseded, 1);
        let record = result.state.imports.values().find(|r| r.memory_id == "mem_new").expect("state record");
        assert_eq!(record.source_key, "a");
        assert!(record.supersession_chain.iter().any(|link| link.memory_id == "mem_prior"));
    }

    #[tokio::test]
    async fn execute_refusal_appends_to_report_and_does_not_record_state() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Refused,
            id: None,
            existing_id: None,
            next_actions: Vec::new(),
            reason: Some(GovernanceRefusalReason::Privacy),
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.refused_privacy, 1);
        assert_eq!(result.report.refusals.len(), 1);
        assert_eq!(result.report.refusals[0].reason, "privacy");
        assert!(!result.state.imports.contains_key("a"), "refused writes never land in state file");
    }

    #[tokio::test]
    async fn execute_dry_run_performs_zero_socket_calls_and_zero_state_writes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default();
        let opts = ExecuteOptions { dry_run: true, verbose_progress: false };
        let result = engine.execute(plan_with_actions(actions), opts, &mut client).await.expect("dry-run ok");
        assert_eq!(client.write_calls.len(), 0);
        assert_eq!(client.supersede_calls.len(), 0);
        // Dry-run still counts the intended write so the report previews the scope.
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.written_new, 1);
        // No state file written to disk.
        assert!(!engine.state_path.exists());
    }

    #[tokio::test]
    async fn execute_resolves_wiki_links_against_in_flight_alias_map() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        // Topo-sorted: b first (leaf), a second (depends on b).
        let mut actions = vec![
            make_planned("a", "see [[b]]", vec!["b".to_string()], PlanAction::WriteNew),
            make_planned("b", "leaf", Vec::new(), PlanAction::WriteNew),
        ];
        let (sorted, _back_edges) = topo_sort(actions.clone());
        actions = sorted;
        let mut client = MockDaemonClient::default()
            .push_write(WriteMemoryOutcome {
                status: GovernanceStatus::Promoted,
                id: Some("mem_b".to_string()),
                existing_id: None,
                next_actions: Vec::new(),
                reason: None,
            })
            .push_write(WriteMemoryOutcome {
                status: GovernanceStatus::Promoted,
                id: Some("mem_a".to_string()),
                existing_id: None,
                next_actions: Vec::new(),
                reason: None,
            });
        let _result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        // The second write (a) must have carried related=[mem_b] in its meta.
        let a_call = client.write_calls.last().expect("at least one call");
        let related = a_call
            .meta
            .get("related")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        assert_eq!(related, vec!["mem_b"], "wiki-link target resolved against in-flight alias map");
    }

    #[tokio::test]
    async fn execute_candidate_write_populates_report_candidates_list() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        // A Candidate status with an id and no supersede next-action lands the
        // source in the review queue as a candidate.
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Candidate,
            id: Some("mem_cand".to_string()),
            existing_id: None,
            next_actions: Vec::new(),
            reason: None,
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.written_candidate, 1, "counter and list stay one-to-one");
        assert_eq!(result.report.candidates.len(), 1);
        assert_eq!(result.report.candidates[0].source_key, "a");
        assert_eq!(result.report.candidates[0].harness, "claude-code");
        assert_eq!(result.report.candidates[0].memory_id.as_deref(), Some("mem_cand"));
        assert!(result.report.quarantined.is_empty());
    }

    #[tokio::test]
    async fn execute_quarantined_write_populates_report_quarantined_list() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Quarantined,
            id: Some("mem_quar".to_string()),
            existing_id: None,
            next_actions: Vec::new(),
            reason: None,
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.quarantined, 1);
        assert_eq!(result.report.quarantined.len(), 1);
        assert_eq!(result.report.quarantined[0].source_key, "a");
        assert_eq!(result.report.quarantined[0].memory_id.as_deref(), Some("mem_quar"));
        assert!(result.report.candidates.is_empty());
    }

    #[tokio::test]
    async fn plan_threads_malformed_frontmatter_into_recovered_and_report() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        // Malformed YAML frontmatter the lenient line-scan salvages rather than
        // dropping — the source key must surface in `frontmatter_recovered`.
        std::fs::write(
            claude_root.join("proj/memory/bad.md"),
            b"---\n: this is not valid yaml ::\n  :: blip\n---\nBody after broken YAML\n",
        )
        .expect("write");
        std::fs::write(claude_root.join("proj/memory/good.md"), b"---\nname: Good\n---\nA fine memory.\n")
            .expect("write");

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: vec![claude_root],
                    from_codex: Some(PathBuf::from("/does/not/exist")),
                    harness_filter: None,
                    quiet: true,
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("plan ok");
        assert!(
            plan.frontmatter_recovered.iter().any(|k| k.contains("bad.md")),
            "recovered: {:?}",
            plan.frontmatter_recovered
        );
        assert!(!plan.claude_roots_used.is_empty(), "claude roots covered: {:?}", plan.claude_roots_used);

        // from_plan must carry the recovered keys and roots onto the report.
        let report = ImportReport::from_plan(&plan);
        assert!(report.frontmatter_recovered.iter().any(|k| k.contains("bad.md")));
        assert_eq!(report.claude_roots_used, plan.claude_roots_used);
    }

    #[tokio::test]
    async fn mock_get_memory_truncates_when_full_body_unset() {
        let mut client = MockDaemonClient::default().with_get_response(
            "mem_large",
            GetResponse {
                id: "mem_large".to_string(),
                summary: "big memory".to_string(),
                body: "x".repeat(5_000),
                truncated: false,
                provenance: None,
                status: Some(MemoryStatus::Active),
                guidance: String::new(),
            },
        );

        let full = client.get_memory("mem_large", true).await.expect("get full");
        assert_eq!(full.body.chars().count(), 5_000);
        assert!(!full.truncated);

        let preview = client.get_memory("mem_large", false).await.expect("get preview");
        assert_eq!(preview.body.chars().count(), 4_096);
        assert!(preview.truncated);
    }

    #[tokio::test]
    async fn execute_supersede_chain_walks_multiple_hops() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let candidate = make_candidate("a", "replacement", Vec::new());
        let new_identity = candidate.import_identity(None);
        let mut state = ImportState::default();
        state.imports.insert(
            "a".to_string(),
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_A".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("/fixture/a"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );
        let action = PlannedWrite {
            source_key: candidate.source_key.clone(),
            candidate,
            scope: ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                namespace_alias: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            },
            action: PlanAction::Supersede {
                prior_memory_id: "mem_A".to_string(),
                prior_content_hash: "sha256:STALE".to_string(),
            },
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        };

        let mut client = MockDaemonClient::default()
            .with_superseded_by_chain("mem_A", vec!["mem_B".to_string()])
            .with_superseded_by_chain("mem_B", vec!["mem_C".to_string()])
            .with_get_response(
                "mem_B",
                GetResponse {
                    id: "mem_B".to_string(),
                    summary: "not yet".to_string(),
                    body: "different body".to_string(),
                    truncated: false,
                    provenance: None,
                    status: Some(MemoryStatus::Active),
                    guidance: String::new(),
                },
            )
            .with_get_response(
                "mem_C",
                GetResponse {
                    id: "mem_C".to_string(),
                    summary: "replacement".to_string(),
                    body: "replacement".to_string(),
                    truncated: false,
                    provenance: None,
                    status: Some(MemoryStatus::Active),
                    guidance: String::new(),
                },
            );

        let result = engine
            .execute(
                ImportPlan {
                    actions: vec![action],
                    source_discovery_summary: DiscoverySummary::default(),
                    unresolved_back_edges: Vec::new(),
                    parse_errors: Vec::new(),
                    frontmatter_recovered: Vec::new(),
                    claude_roots_used: Vec::new(),
                    state,
                },
                ExecuteOptions::default(),
                &mut client,
            )
            .await
            .expect("execute ok");

        assert!(client.supersede_calls.is_empty(), "multi-hop adoption must not re-issue supersede");
        assert!(client.trust_artifact_calls.contains(&"mem_A".to_string()));
        assert!(client.trust_artifact_calls.contains(&"mem_B".to_string()));
        assert!(client.get_calls.contains(&"mem_B".to_string()));
        assert!(client.get_calls.contains(&"mem_C".to_string()));
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.superseded, 1);
        let adopted = result.state.imports.get(&new_identity).expect("adopted C is recorded in state");
        assert_eq!(adopted.memory_id, "mem_C");
    }

    #[tokio::test]
    async fn execute_supersede_chain_rejects_tombstoned_adoption() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let candidate = make_candidate("a", "replacement", Vec::new());
        let mut state = ImportState::default();
        state.imports.insert(
            "a".to_string(),
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_prior".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("/fixture/a"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );
        let new_identity = candidate.import_identity(None);
        let action = PlannedWrite {
            source_key: candidate.source_key.clone(),
            candidate,
            scope: ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                namespace_alias: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            },
            action: PlanAction::Supersede {
                prior_memory_id: "mem_prior".to_string(),
                prior_content_hash: "sha256:STALE".to_string(),
            },
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        };

        let mut client = MockDaemonClient::default()
            .with_superseded_by_chain("mem_prior", vec!["mem_new".to_string()])
            .with_get_response(
                "mem_new",
                GetResponse {
                    id: "mem_new".to_string(),
                    summary: "replacement".to_string(),
                    body: "replacement".to_string(),
                    truncated: false,
                    provenance: None,
                    status: Some(MemoryStatus::Tombstoned),
                    guidance: String::new(),
                },
            )
            .push_supersede(SupersedeOutcome {
                status: GovernanceStatus::Promoted,
                new_id: Some("mem_fresh".to_string()),
                reason: None,
            });

        let result = engine
            .execute(
                ImportPlan {
                    actions: vec![action],
                    source_discovery_summary: DiscoverySummary::default(),
                    unresolved_back_edges: Vec::new(),
                    parse_errors: Vec::new(),
                    frontmatter_recovered: Vec::new(),
                    claude_roots_used: Vec::new(),
                    state,
                },
                ExecuteOptions::default(),
                &mut client,
            )
            .await
            .expect("execute ok");

        assert_eq!(client.supersede_calls.len(), 1, "tombstoned replacement must fall through to supersede");
        let fresh_record = result.state.imports.get(&new_identity).expect("fresh supersede is recorded");
        assert_eq!(fresh_record.memory_id, "mem_fresh");
    }

    #[tokio::test]
    async fn execute_supersede_chain_errors_on_get_failure() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let candidate = make_candidate("a", "replacement", Vec::new());
        let mut state = ImportState::default();
        state.imports.insert(
            "a".to_string(),
            ImportRecord {
                source_identity: String::new(),
                source_key: String::new(),
                source_memory_id: None,
                memory_id: "mem_prior".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("/fixture/a"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );
        let action = PlannedWrite {
            source_key: candidate.source_key.clone(),
            candidate,
            scope: ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                namespace_alias: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            },
            action: PlanAction::Supersede {
                prior_memory_id: "mem_prior".to_string(),
                prior_content_hash: "sha256:STALE".to_string(),
            },
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        };

        // get_memory not configured for mem_new -> Err, which must fail closed.
        let mut client = MockDaemonClient::default()
            .with_superseded_by_chain("mem_prior", vec!["mem_new".to_string()]);

        let result = engine
            .execute(
                ImportPlan {
                    actions: vec![action],
                    source_discovery_summary: DiscoverySummary::default(),
                    unresolved_back_edges: Vec::new(),
                    parse_errors: Vec::new(),
                    frontmatter_recovered: Vec::new(),
                    claude_roots_used: Vec::new(),
                    state,
                },
                ExecuteOptions::default(),
                &mut client,
            )
            .await;

        assert!(result.is_err(), "lookup errors must fail closed");
    }

    #[tokio::test]
    async fn execute_alias_map_ignores_tuple_identity_keys() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let mut planned = make_planned("src/foo", "body", vec!["tuple:codex:/ignored:me:foo:bar".to_string()], PlanAction::WriteNew);
        // Simulate the planning resolution: this alias is the target of the wiki link.
        planned.wiki_link_targets_resolvable = vec!["tuple:codex:/ignored:me:foo:bar".to_string()];

        let mut state = ImportState::default();
        state.imports.insert(
            "tuple:codex:/ignored:me:foo:bar".to_string(),
            ImportRecord {
                source_identity: "tuple:codex:/ignored:me:foo:bar".to_string(),
                source_key: "src/foo".to_string(),
                source_memory_id: None,
                memory_id: "mem_existing".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("/fixture/src/foo"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );

        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Promoted,
            id: Some("mem_new".to_string()),
            existing_id: None,
            next_actions: Vec::new(),
            reason: None,
        });

        engine
            .execute(
                ImportPlan {
                    actions: vec![planned],
                    source_discovery_summary: DiscoverySummary::default(),
                    unresolved_back_edges: Vec::new(),
                    parse_errors: Vec::new(),
                    frontmatter_recovered: Vec::new(),
                    claude_roots_used: Vec::new(),
                    state,
                },
                ExecuteOptions::default(),
                &mut client,
            )
            .await
            .expect("execute ok");

        let write_call = client.write_calls.first().expect("write called");
        assert!(
            write_call.meta.get("related").is_none(),
            "tuple identity key must not be seeded as a wiki-link alias"
        );
        // source_key is still a valid alias.
        assert!(write_call.meta.get("aliases").is_some());
    }

    #[tokio::test]
    async fn execute_supersede_removes_legacy_source_key() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let candidate = make_candidate("src/foo", "new body", Vec::new());
        let mut state = ImportState::default();
        let legacy_key = "claude:src/foo.md".to_string();
        let new_identity = candidate.import_identity(None);
        state.imports.insert(
            legacy_key.clone(),
            ImportRecord {
                source_identity: String::new(),
                source_key: legacy_key.clone(),
                source_memory_id: None,
                memory_id: "mem_old".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("/fixture/src/foo"),
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                aliases: Vec::new(),
                supersession_chain: Vec::new(),
            },
        );
        let action = PlannedWrite {
            source_key: candidate.source_key.clone(),
            candidate,
            scope: ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                namespace_alias: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            },
            action: PlanAction::Supersede {
                prior_memory_id: "mem_old".to_string(),
                prior_content_hash: "sha256:STALE".to_string(),
            },
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        };

        let mut client = MockDaemonClient::default()
            .with_superseded_by_chain("mem_old", Vec::new())
            .push_supersede(SupersedeOutcome {
                status: GovernanceStatus::Promoted,
                new_id: Some("mem_new".to_string()),
                reason: None,
            });

        let result = engine
            .execute(
                ImportPlan {
                    actions: vec![action],
                    source_discovery_summary: DiscoverySummary::default(),
                    unresolved_back_edges: Vec::new(),
                    parse_errors: Vec::new(),
                    frontmatter_recovered: Vec::new(),
                    claude_roots_used: Vec::new(),
                    state,
                },
                ExecuteOptions::default(),
                &mut client,
            )
            .await
            .expect("execute ok");

        assert!(!result.state.imports.contains_key(&legacy_key), "legacy source key must be removed");
        assert!(result.state.imports.contains_key(&new_identity), "new source_identity must be present");
    }
}
