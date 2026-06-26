mod common;

use chrono::{Duration, Utc};
use common::trust_for_status;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, Entity, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::StartupRequest;
use memoryd::state::{DaemonState, RealityCheckState};
use serde_json::json;

const REALITY_CHECK_DUE_ITEM: &str = "<item kind=\"reality_check_due\" count=\"1\">Weekly Reality Check is ready — run `memoryd reality-check run` or open TUI panel 8.</item>";

#[tokio::test]
async fn test_reality_check_due_item_appears_when_due() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(8)),
        snooze_until: None,
    });

    let startup = fixture.startup().await;

    assert!(pending_attention_block(&startup.recall_block).contains(REALITY_CHECK_DUE_ITEM));
}

#[tokio::test]
async fn test_reality_check_due_suppressed_when_not_due() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(5)),
        snooze_until: None,
    });

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("reality_check_due"));
}

#[tokio::test]
async fn test_reality_check_due_suppressed_when_state_missing() {
    let fixture = PendingAttentionFixture::new().await;

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("reality_check_due"));
}

#[tokio::test]
async fn test_reality_check_due_suppressed_when_snoozed() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(8)),
        snooze_until: Some(Utc::now() + Duration::days(1)),
    });

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("reality_check_due"));
}

#[tokio::test]
async fn test_reality_check_due_item_appears_at_most_once_per_7_day_window() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(8)),
        snooze_until: None,
    });

    let first = fixture.startup().await;
    let second = fixture.startup().await;

    assert!(first.recall_block.contains(REALITY_CHECK_DUE_ITEM));
    assert!(!second.recall_block.contains("reality_check_due"));
}

#[tokio::test]
async fn test_reality_check_item_text_contains_no_memory_content() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(8)),
        snooze_until: None,
    });
    fixture
        .write_memory(FixtureMemory {
            id: "mem_20260501_1111111111111111_000001",
            summary: "private-title-never-in-rc-item",
            body: "private-body-never-in-rc-item",
            status: MemoryStatus::Candidate,
            entities: &["private-entity-never-in-rc-item"],
        })
        .await;

    let startup = fixture.startup().await;
    let item = reality_check_item(&startup.recall_block);

    assert_eq!(item, REALITY_CHECK_DUE_ITEM);
    assert!(!item.contains("private-title-never-in-rc-item"));
    assert!(!item.contains("private-body-never-in-rc-item"));
    assert!(!item.contains("private-entity-never-in-rc-item"));
    assert!(!item.contains("mem_20260501"));
}

#[tokio::test]
async fn test_reality_check_item_dropped_when_6_total_cap_full() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(8)),
        snooze_until: None,
    });
    fixture.write_memory_with_entities(&["ent_a", "ent_b", "ent_c"]).await;
    fixture.write_questions("me", &["me cap 1", "me cap 2"]);
    fixture.write_questions("project:proj_rc", &["project cap 1", "project cap 2"]);
    fixture.write_questions("agent", &["agent cap 1", "agent cap 2"]);

    let startup = fixture.startup().await;
    let status = fixture.status().await;

    assert_eq!(pending_attention_items(&startup.recall_block).len(), 6);
    assert!(!startup.recall_block.contains("reality_check_due"));
    assert_eq!(status.recall.dream_question_omitted_total.get("cap_total"), Some(&1));
}

#[tokio::test]
async fn test_reality_check_item_counts_against_cap() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(8)),
        snooze_until: None,
    });
    fixture.write_memory_with_entities(&["ent_a", "ent_b", "ent_c"]).await;
    fixture.write_questions("me", &["me cap 1", "me cap 2"]);
    fixture.write_questions("project:proj_rc", &["project cap 1", "project cap 2"]);
    fixture.write_questions("agent", &["agent cap 1"]);

    let startup = fixture.startup().await;

    assert_eq!(pending_attention_items(&startup.recall_block).len(), 6);
    assert!(startup.recall_block.contains(REALITY_CHECK_DUE_ITEM));
}

#[tokio::test]
async fn test_startup_xml_version_string_unchanged() {
    let fixture = PendingAttentionFixture::new().await;
    fixture.write_reality_check_state(RealityCheckState {
        last_completed_at: Some(Utc::now() - Duration::days(8)),
        snooze_until: None,
    });

    let startup = fixture.startup().await;

    assert!(startup.recall_block.starts_with("<memory-recall version=\"stream-e-v0.6\""));
    assert_eq!(startup.recall_explanation.policy, "stream-e-v0.6");
}

struct PendingAttentionFixture {
    _temp: tempfile::TempDir,
    substrate: Substrate,
    state: HandlerState,
    repo: std::path::PathBuf,
    runtime: std::path::PathBuf,
}

impl PendingAttentionFixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&repo).expect("repo dir");
        std::fs::write(repo.join(".memory-project.yaml"), "canonical_id: proj_rc\nalias: rc-project\n")
            .expect("project binding");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, substrate, state: HandlerState::new(), repo, runtime }
    }

    fn write_reality_check_state(&self, reality_check: RealityCheckState) {
        DaemonState { reality_check, ..Default::default() }.save(&self.runtime).expect("daemon state writes");
    }

    async fn write_memory_with_entities(&self, entities: &[&str]) {
        self.write_memory(FixtureMemory {
            id: "mem_20260501_1111111111111111_000002",
            summary: "seed memory",
            body: "seed body",
            status: MemoryStatus::Active,
            entities,
        })
        .await;
    }

    async fn write_memory(&self, fixture: FixtureMemory<'_>) {
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: self.memory(fixture),
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("memory write");
    }

    fn memory(&self, fixture: FixtureMemory<'_>) -> Memory {
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new(fixture.id),
                memory_type: MemoryType::Project,
                scope: Scope::User,
                summary: fixture.summary.to_owned(),
                confidence: 0.9,
                original_confidence: None,
                trust_level: trust_for_status(fixture.status),
                sensitivity: Sensitivity::Internal,
                status: fixture.status,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                observed_at: None,
                author: Author {
                    kind: AuthorKind::Agent,
                    user_handle: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_reality_check".to_owned()),
                    subagent_id: None,
                    phase: None,
                    component: None,
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: Vec::new(),
                entities: fixture
                    .entities
                    .iter()
                    .map(|entity| Entity { id: (*entity).to_owned(), label: (*entity).to_owned(), aliases: Vec::new() })
                    .collect(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::AgentPrimary,
                    reference: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_reality_check".to_owned()),
                    subagent_id: None,
                    device: None,
                },
                evidence: Vec::new(),
                requires_user_confirmation: matches!(
                    fixture.status,
                    MemoryStatus::Candidate | MemoryStatus::Quarantined
                ),
                review_state: matches!(fixture.status, MemoryStatus::Candidate | MemoryStatus::Quarantined)
                    .then(|| "pending".to_owned()),
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: true,
                    max_scope: Scope::User,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: true,
                },
                write_policy: WritePolicy {
                    human_review_required: matches!(
                        fixture.status,
                        MemoryStatus::Candidate | MemoryStatus::Quarantined
                    ),
                    policy_applied: "reality-check-pending-test".to_owned(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                extras: Default::default(),
            },
            body: fixture.body.to_owned(),
            path: Some(RepoPath::new(format!("me/{}.md", fixture.id))),
        }
    }

    fn write_questions(&self, scope: &str, questions: &[&str]) {
        let path = question_file_path(&self.repo, scope);
        std::fs::create_dir_all(path.parent().expect("question parent")).expect("question dir");
        let lines = questions
            .iter()
            .map(|question| json!({ "entities": ["ent_a", "ent_b", "ent_c"], "question": question }).to_string())
            .collect::<Vec<_>>();
        std::fs::write(path, format!("{}\n", lines.join("\n"))).expect("question file");
    }

    async fn startup(&self) -> Box<memoryd::recall::StartupResponse> {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new("req-startup", RequestPayload::Startup(startup_request(&self.repo))),
        )
        .await;
        match response.result {
            ResponseResult::Success(ResponsePayload::Startup(startup)) => startup,
            other => panic!("expected startup success, got {other:?}"),
        }
    }

    async fn status(&self) -> memoryd::protocol::StatusResponse {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new("req-status", RequestPayload::Status),
        )
        .await;
        match response.result {
            ResponseResult::Success(ResponsePayload::Status(status)) => status,
            other => panic!("expected status success, got {other:?}"),
        }
    }
}

struct FixtureMemory<'a> {
    id: &'a str,
    summary: &'a str,
    body: &'a str,
    status: MemoryStatus,
    entities: &'a [&'a str],
}

fn startup_request(repo: &std::path::Path) -> StartupRequest {
    StartupRequest {
        cwd: repo.to_string_lossy().into_owned(),
        session_id: "sess_reality_check".to_owned(),
        harness: "codex".to_owned(),
        harness_version: None,
        include_recent: true,
        since_event_id: None,
        budget_tokens: Some(512),
        passive: false,
    }
}

fn pending_attention_block(recall_block: &str) -> &str {
    recall_block
        .split("<pending-attention>")
        .nth(1)
        .and_then(|tail| tail.split("</pending-attention>").next())
        .expect("pending-attention section")
}

fn pending_attention_items(recall_block: &str) -> Vec<String> {
    pending_attention_block(recall_block)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn reality_check_item(recall_block: &str) -> String {
    pending_attention_items(recall_block)
        .into_iter()
        .find(|line| line.contains("reality_check_due"))
        .expect("reality_check_due item")
}

fn question_file_path(repo: &std::path::Path, scope: &str) -> std::path::PathBuf {
    match scope {
        "me" | "agent" => repo.join("dreams/questions").join(scope).join("2026-05-01.jsonl"),
        scoped if scoped.starts_with("project:") => {
            repo.join("dreams/questions/project").join(scoped.trim_start_matches("project:")).join("2026-05-01.jsonl")
        }
        other => panic!("unsupported fixture scope {other}"),
    }
}
