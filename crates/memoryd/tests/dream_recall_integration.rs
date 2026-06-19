use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, Entity, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::{estimated_tokens, StartupRequest};
use serde_json::json;

#[tokio::test]
async fn matching_entity_ids_surface_in_pending_attention() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000001", &["ent_jwt"]).await;
    fixture.write_questions("me", "2026-04-30", &[question(&["ent_jwt"], "What JWT assumption are we avoiding?")]);

    let startup = fixture.startup().await;

    assert!(startup.recall_block.contains("<pending-attention>"));
    assert!(startup.recall_block.contains("- [me] What JWT assumption are we avoiding?"));
}

#[tokio::test]
async fn recently_surfaced_question_hashes_are_suppressed_within_novelty_window() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000008", &["ent_jwt"]).await;
    fixture.write_questions("me", "2026-04-30", &[question(&["ent_jwt"], "What JWT assumption are we avoiding?")]);

    let first = fixture.startup().await;

    assert_eq!(pending_attention_lines(&first.recall_block), vec!["- [me] What JWT assumption are we avoiding?"]);

    fixture.write_questions(
        "me",
        "2026-04-30",
        &[
            question(&["ent_jwt"], "What JWT assumption are we avoiding?"),
            question(&["ent_jwt"], "What JWT rotation risk remains?"),
        ],
    );
    let second = fixture.startup().await;

    assert_eq!(pending_attention_lines(&second.recall_block), vec!["- [me] What JWT rotation risk remains?"]);
    assert!(fixture.status().await.recall.dream_question_omitted_total.is_empty());
}

#[tokio::test]
async fn empty_entities_records_never_surface() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000002", &["ent_jwt"]).await;
    fixture.write_questions("me", "2026-04-30", &[question(&[], "This has no entity gate.")]);

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("This has no entity gate."));
    assert_counter_absent(&fixture.status().await, "no_entity_match");
}

#[tokio::test]
async fn non_matching_entities_increment_no_entity_match_counter() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000003", &["ent_jwt"]).await;
    fixture.write_questions("me", "2026-04-30", &[question(&["ent_billing"], "What billing risk is hidden?")]);

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("What billing risk is hidden?"));
    assert_counter(&fixture.status().await, "no_entity_match", 1);
}

#[tokio::test]
async fn per_scope_and_total_caps_apply_and_increment_cap_counters() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000004", &["ent_a", "ent_b", "ent_c"]).await;
    fixture.write_questions(
        "me",
        "2026-04-30",
        &[
            question(&["ent_a", "ent_b", "ent_c"], "me high 1"),
            question(&["ent_a", "ent_b", "ent_c"], "me high 2"),
            question(&["ent_a", "ent_b", "ent_c"], "me high 3"),
            question(&["ent_a", "ent_b", "ent_c"], "me high 4"),
        ],
    );
    fixture.write_questions(
        "project:proj_dream",
        "2026-04-30",
        &[
            question(&["ent_a", "ent_b"], "project medium 1"),
            question(&["ent_a", "ent_b"], "project medium 2"),
            question(&["ent_a", "ent_b"], "project medium 3"),
            question(&["ent_a", "ent_b"], "project medium 4"),
        ],
    );
    fixture.write_questions(
        "agent",
        "2026-04-30",
        &[
            question(&["ent_a"], "agent low 1"),
            question(&["ent_a"], "agent low 2"),
            question(&["ent_a"], "agent low 3"),
            question(&["ent_a"], "agent low 4"),
        ],
    );

    let startup = fixture.startup().await;
    let pending_lines = pending_attention_lines(&startup.recall_block);

    assert_eq!(pending_lines.len(), 6);
    assert_eq!(pending_lines.iter().filter(|line| line.contains("[me]")).count(), 2);
    assert_eq!(pending_lines.iter().filter(|line| line.contains("[project:proj_dream]")).count(), 2);
    assert_eq!(pending_lines.iter().filter(|line| line.contains("[agent]")).count(), 2);
    assert_counter(&fixture.status().await, "cap_section", 4);
    assert_counter(&fixture.status().await, "cap_total", 2);
}

#[tokio::test]
async fn pending_attention_order_is_deterministic_by_strength_recency_hash_then_lex() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000005", &["ent_a", "ent_b", "ent_c"]).await;
    fixture.write_questions("me", "2026-04-29", &[question(&["ent_a", "ent_b", "ent_c"], "older stronger")]);
    fixture.write_questions("project:proj_dream", "2026-04-30", &[question(&["ent_a", "ent_b"], "newer medium")]);
    fixture.write_questions(
        "agent",
        "2026-04-30",
        &[question(&["ent_a"], "weaker beta"), question(&["ent_a"], "weaker alpha")],
    );

    let first = fixture.startup().await;
    let second_fixture = DreamRecallFixture::new().await;
    second_fixture.write_seed_memory("mem_20260430_1111111111111111_000005", &["ent_a", "ent_b", "ent_c"]).await;
    second_fixture.write_questions("me", "2026-04-29", &[question(&["ent_a", "ent_b", "ent_c"], "older stronger")]);
    second_fixture.write_questions(
        "project:proj_dream",
        "2026-04-30",
        &[question(&["ent_a", "ent_b"], "newer medium")],
    );
    second_fixture.write_questions(
        "agent",
        "2026-04-30",
        &[question(&["ent_a"], "weaker beta"), question(&["ent_a"], "weaker alpha")],
    );
    let second = second_fixture.startup().await;
    let first_lines = pending_attention_lines(&first.recall_block);
    let second_lines = pending_attention_lines(&second.recall_block);

    assert_eq!(first_lines, second_lines);
    assert_eq!(first_lines[0], "- [me] older stronger");
    assert_eq!(first_lines[1], "- [project:proj_dream] newer medium");
    assert!(first_lines[2].starts_with("- [agent] weaker "));
    assert!(first_lines[3].starts_with("- [agent] weaker "));
}

#[tokio::test]
async fn unsafe_questions_are_omitted_and_counted() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000006", &["ent_secret"]).await;
    fixture.write_questions(
        "me",
        "2026-04-30",
        &[question(&["ent_secret"], "Why are we ignoring card 4111111111111111?")],
    );

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("4111111111111111"));
    assert_counter(&fixture.status().await, "unsafe_fragment", 1);
}

#[tokio::test]
async fn malformed_question_records_are_omitted_and_counted() {
    let fixture = DreamRecallFixture::new().await;
    fixture.write_seed_memory("mem_20260430_1111111111111111_000007", &["ent_jwt"]).await;
    fixture.write_question_lines("me", "2026-04-30", &["not-json", r#"{"entities":["ent_jwt"],"question":""}"#]);

    let startup = fixture.startup().await;

    assert!(pending_attention_lines(&startup.recall_block).is_empty());
    assert_counter(&fixture.status().await, "malformed_record", 2);
}

#[tokio::test]
async fn startup_recall_is_byte_identical_to_stream_e_baseline_without_dream_questions() {
    let fixture = DreamRecallFixture::new().await;

    let startup = fixture.startup().await;

    assert_eq!(startup.recall_block, stream_e_empty_baseline(&startup.session_binding.cwd));
}

struct DreamRecallFixture {
    _temp: tempfile::TempDir,
    substrate: Substrate,
    state: HandlerState,
    repo: std::path::PathBuf,
}

impl DreamRecallFixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&repo).expect("repo dir");
        std::fs::write(repo.join(".memory-project.yaml"), "canonical_id: proj_dream\nalias: dream-project\n")
            .expect("project binding");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_dreamrecall".to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, substrate, state: HandlerState::new(), repo }
    }

    async fn write_seed_memory(&self, id: &str, entities: &[&str]) {
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: self.memory(id, entities),
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("seed memory write");
    }

    fn memory(&self, id: &str, entities: &[&str]) -> Memory {
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new(id),
                memory_type: MemoryType::Project,
                scope: Scope::User,
                summary: format!("seed memory for {}", entities.join(",")),
                confidence: 0.9,
                original_confidence: None,
                trust_level: TrustLevel::Trusted,
                sensitivity: Sensitivity::Internal,
                status: MemoryStatus::Active,
                created_at: instant("2026-04-30T12:00:00Z"),
                updated_at: instant("2026-04-30T12:00:00Z"),
                observed_at: None,
                author: Author {
                    kind: AuthorKind::Agent,
                    user_handle: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_dream".to_owned()),
                    subagent_id: None,
                    phase: None,
                    component: None,
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: Vec::new(),
                entities: entities
                    .iter()
                    .map(|entity| Entity { id: (*entity).to_owned(), label: (*entity).to_owned(), aliases: Vec::new() })
                    .collect(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::AgentPrimary,
                    reference: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_dream".to_owned()),
                    subagent_id: None,
                    device: None,
                },
                evidence: Vec::new(),
                requires_user_confirmation: false,
                review_state: None,
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
                    human_review_required: false,
                    policy_applied: "dream-recall-test".to_owned(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                extras: BTreeMap::new(),
            },
            body: "seed body".to_owned(),
            path: Some(RepoPath::new(format!("me/{id}.md"))),
        }
    }

    fn write_questions(&self, scope: &str, date: &str, records: &[serde_json::Value]) {
        let lines = records.iter().map(serde_json::Value::to_string).collect::<Vec<_>>();
        let refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
        self.write_question_lines(scope, date, &refs);
    }

    fn write_question_lines(&self, scope: &str, date: &str, lines: &[&str]) {
        let path = question_file_path(&self.repo, scope, date);
        std::fs::create_dir_all(path.parent().expect("question parent")).expect("question dir");
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

fn startup_request(repo: &Path) -> StartupRequest {
    StartupRequest {
        cwd: repo.to_string_lossy().into_owned(),
        session_id: "sess_dream".to_owned(),
        harness: "codex".to_owned(),
        harness_version: None,
        include_recent: true,
        since_event_id: None,
        budget_tokens: Some(512),
        passive: false,
    }
}

fn question(entities: &[&str], question: &str) -> serde_json::Value {
    json!({ "entities": entities, "question": question })
}

fn question_file_path(repo: &Path, scope: &str, date: &str) -> std::path::PathBuf {
    match scope {
        "me" | "agent" => repo.join("dreams/questions").join(scope).join(format!("{date}.jsonl")),
        scoped if scoped.starts_with("project:") => repo
            .join("dreams/questions/project")
            .join(scoped.trim_start_matches("project:"))
            .join(format!("{date}.jsonl")),
        scoped if scoped.starts_with("org:") => {
            repo.join("dreams/questions/org").join(scoped.trim_start_matches("org:")).join(format!("{date}.jsonl"))
        }
        other => panic!("unsupported fixture scope {other}"),
    }
}

fn pending_attention_lines(recall_block: &str) -> Vec<String> {
    recall_block
        .split("<pending-attention>")
        .nth(1)
        .and_then(|tail| tail.split("</pending-attention>").next())
        .expect("pending-attention section")
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("- ["))
        .map(str::to_owned)
        .collect()
}

fn assert_counter(status: &memoryd::protocol::StatusResponse, reason: &str, expected: u64) {
    assert_eq!(status.recall.dream_question_omitted_total.get(reason), Some(&expected), "reason {reason}");
}

fn assert_counter_absent(status: &memoryd::protocol::StatusResponse, reason: &str) {
    assert!(!status.recall.dream_question_omitted_total.contains_key(reason), "reason {reason}");
}

fn stream_e_empty_baseline(cwd: &str) -> String {
    let template = [
        "<memory-recall version=\"stream-e-v0.6\" harness=\"codex\" session=\"sess_dream\">".to_owned(),
        "  <identity>".to_owned(),
        "    - harness: codex".to_owned(),
        "    - session: sess_dream".to_owned(),
        format!("    - cwd: {cwd}"),
        "  </identity>".to_owned(),
        "  <project-state project=\"dream-project\" resolved-via=\"yaml_override\">".to_owned(),
        "    - project: dream-project".to_owned(),
        "    - namespace: project:proj_dream".to_owned(),
        "  </project-state>".to_owned(),
        "  <entity-recall entities=\"dream-project,proj_dream\">".to_owned(),
        "  </entity-recall>".to_owned(),
        "  <recent-memory>".to_owned(),
        "  </recent-memory>".to_owned(),
        "  <pending-attention>".to_owned(),
        "  </pending-attention>".to_owned(),
        "  <recall-explanation policy=\"stream-e-v0.6\" budget-tokens=\"512\" used-tokens=\"{used}\">".to_owned(),
        "    Deterministic passive recall from Memorum index rows.".to_owned(),
        "  </recall-explanation>".to_owned(),
        "</memory-recall>".to_owned(),
        String::new(),
    ]
    .join("\n");
    let mut block = template.replace("{used}", "0");

    for _ in 0..4 {
        let measured = estimated_tokens(&block);
        let next = template.replace("{used}", &measured.to_string());
        if next == block {
            return block;
        }
        block = next;
    }
    block
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
