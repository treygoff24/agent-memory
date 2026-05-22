use chrono::{DateTime, Utc};
use memory_substrate::{
    events::{read_events, EventKind},
    Author, AuthorKind, ClassificationOutcome, EncryptedWriteRequest, EventContext, Frontmatter, InitOptions, Memory,
    MemoryContent, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source,
    SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request, handle_request_with_state, HandlerState};
use memoryd::protocol::{
    RealityCheckAction, RealityCheckCompletion, RealityCheckRequest, RealityCheckResponse, RequestEnvelope,
    RequestPayload, RespondRefusalKind, ResponsePayload, ResponseResult,
};
use memoryd::state::{DaemonState, RcSessionStore};
use tempfile::TempDir;

#[tokio::test]
async fn test_list_uses_scoring_and_returns_component_scores() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("List scoring item", "List scoring body", Scope::User).await;

    let response = fixture.reality_check(RealityCheckRequest::List { namespace: None, limit: Some(1) }).await;

    let RealityCheckResponse::Pending { session_id, items, total_scored, .. } = response else {
        panic!("expected pending response");
    };
    assert_eq!(session_id, None);
    assert_eq!(total_scored, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].memory_id, id);
    assert!(items[0].component_scores.days_since_observed_norm.is_finite());
}

#[tokio::test]
async fn test_run_creates_session_state_file_and_resumes_existing_session() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Run scoring item", "Run scoring body", Scope::User).await;

    let first = fixture
        .reality_check(RealityCheckRequest::Run {
            session_id: Some("rcs_existing".to_owned()),
            namespace: None,
            limit: None,
        })
        .await;
    let second =
        fixture.reality_check(RealityCheckRequest::Run { session_id: None, namespace: None, limit: None }).await;

    let RealityCheckResponse::Pending { session_id, items, .. } = first else {
        panic!("expected first pending");
    };
    assert_eq!(session_id.as_deref(), Some("rcs_existing"));
    assert_eq!(items[0].memory_id, id);
    let RealityCheckResponse::Pending { session_id, items, .. } = second else {
        panic!("expected resumed pending");
    };
    assert_eq!(session_id.as_deref(), Some("rcs_existing"));
    assert_eq!(items[0].memory_id, id);
    assert!(fixture.runtime.path().join("state/reality-check-session.json").exists());
}

#[tokio::test]
async fn test_confirm_updates_observed_at_and_bumps_confidence() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Confirm item", "Confirm body", Scope::User).await;
    let before = fixture.substrate.read_memory(&id).await.expect("memory before");
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::Confirm,
        })
        .await;

    assert!(matches!(
        response,
        RealityCheckResponse::RespondAccepted { completion: RealityCheckCompletion::Complete { .. }, .. }
    ));
    let after = fixture.substrate.read_memory(&id).await.expect("memory after");
    let observed_at = after.frontmatter.observed_at.expect("confirm persists observed_at");
    assert!(observed_at > before.frontmatter.updated_at);
    assert!(after.frontmatter.updated_at >= before.frontmatter.updated_at);
    assert!((after.frontmatter.confidence - 0.82).abs() < 0.000_001);
    assert!(fixture.events().iter().any(|event| {
        matches!(&event.kind, EventKind::RealityCheckConfirmed { id: event_id, session_id: event_session }
            if event_id == &id && event_session == &session_id)
    }));
}

#[tokio::test]
async fn test_metadata_update_does_not_reset_observed_at() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Observed item", "Observed body", Scope::User).await;
    let session_id = fixture.start_session().await;

    fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id,
            memory_id: id.clone(),
            action: RealityCheckAction::Confirm,
        })
        .await;
    let confirmed = fixture.substrate.read_memory(&id).await.expect("confirmed memory");
    let observed_at = confirmed.frontmatter.observed_at;

    let session_id =
        fixture.reality_check(RealityCheckRequest::Run { session_id: None, namespace: None, limit: None }).await;
    let RealityCheckResponse::Pending { session_id: Some(session_id), .. } = session_id else {
        panic!("expected new session");
    };
    fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id,
            memory_id: id.clone(),
            action: RealityCheckAction::NotRelevant,
        })
        .await;

    let updated = fixture.substrate.read_memory(&id).await.expect("updated memory");
    assert_eq!(updated.frontmatter.observed_at, observed_at);
}

#[tokio::test]
async fn test_not_relevant_sets_passive_recall_false() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Not relevant item", "Not relevant body", Scope::User).await;
    let session_id = match fixture
        .reality_check(RealityCheckRequest::Run { session_id: None, namespace: None, limit: Some(5) })
        .await
    {
        RealityCheckResponse::Pending { session_id: Some(session_id), .. } => session_id,
        other => panic!("expected session id, got {other:?}"),
    };

    fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id,
            memory_id: id.clone(),
            action: RealityCheckAction::NotRelevant,
        })
        .await;

    let memory = fixture.substrate.read_memory(&id).await.expect("memory after");
    assert!(!memory.frontmatter.retrieval_policy.passive_recall);
    assert!(memory.frontmatter.tags.iter().any(|tag| tag == "reality_check_not_relevant"));
}

#[tokio::test]
async fn test_not_relevant_does_not_tombstone() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Still active item", "Still active body", Scope::User).await;
    let session_id = fixture.start_session().await;

    fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id,
            memory_id: id.clone(),
            action: RealityCheckAction::NotRelevant,
        })
        .await;

    let memory = fixture.substrate.read_memory(&id).await.expect("memory after");
    assert_eq!(memory.frontmatter.status, MemoryStatus::Active);
    assert!(fixture
        .events()
        .iter()
        .all(|event| !matches!(&event.kind, EventKind::TombstoneCommitted { id: event_id } if event_id == &id)));
}

#[tokio::test]
async fn test_forget_requires_reason_minimum_length() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Forget invalid item", "Forget invalid body", Scope::User).await;
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::Forget { reason: "no".to_owned() },
        })
        .await;

    assert_eq!(
        response,
        RealityCheckResponse::RespondRefused {
            session_id,
            memory_id: id.clone(),
            reason: "reason too short".to_owned(),
            kind: RespondRefusalKind::InvalidAction,
        }
    );
    assert_eq!(
        fixture.substrate.read_memory(&id).await.expect("memory after").frontmatter.status,
        MemoryStatus::Active
    );
}

#[tokio::test]
async fn test_forget_with_valid_reason_tombstones() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Forget valid item", "Forget valid body", Scope::User).await;
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::Forget { reason: "obsolete".to_owned() },
        })
        .await;

    assert!(matches!(
        response,
        RealityCheckResponse::RespondAccepted { completion: RealityCheckCompletion::Complete { .. }, .. }
    ));
    assert_eq!(
        fixture.substrate.read_memory(&id).await.expect("memory after").frontmatter.status,
        MemoryStatus::Tombstoned
    );
    assert!(fixture.events().iter().any(|event| {
        matches!(&event.kind, EventKind::RealityCheckForgotten { id: event_id, session_id: event_session, reason }
            if event_id == &id && event_session == &session_id && reason == "obsolete")
    }));
}

#[tokio::test]
async fn test_forget_reason_redacts_pii_and_secret_like_text_before_persistence() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Forget private reason", "Forget private body", Scope::User).await;
    let session_id = fixture.start_session().await;
    let unsafe_reason = "contains phone 312-555-0199 email trey@example.com and key sk-test-secret";

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id,
            memory_id: id.clone(),
            action: RealityCheckAction::Forget { reason: unsafe_reason.to_owned() },
        })
        .await;

    assert!(matches!(response, RealityCheckResponse::RespondAccepted { .. }));
    let repo_text = collect_repo_text(fixture._repo.path());
    assert!(!repo_text.contains("312-555-0199"));
    assert!(!repo_text.contains("trey@example.com"));
    assert!(!repo_text.contains("sk-test-secret"));
    assert!(fixture.events().iter().any(|event| {
        matches!(&event.kind, EventKind::RealityCheckForgotten { id: event_id, reason, .. }
            if event_id == &id && reason == "[redacted]")
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_concurrent_responses_are_serialized_for_same_session() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Concurrent item", "Concurrent body", Scope::User).await;
    let state = HandlerState::new();
    let session_id = fixture.start_session_with_state(&state).await;

    let forget = fixture.reality_check_with_state(
        &state,
        RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::Forget { reason: "obsolete".to_owned() },
        },
    );
    let not_relevant = fixture.reality_check_with_state(
        &state,
        RealityCheckRequest::Respond { session_id, memory_id: id.clone(), action: RealityCheckAction::NotRelevant },
    );

    let (first, second) = tokio::join!(forget, not_relevant);
    let accepted = [&first, &second]
        .into_iter()
        .filter(|response| matches!(response, RealityCheckResponse::RespondAccepted { .. }))
        .count();
    let refused = [&first, &second]
        .into_iter()
        .filter(|response| {
            matches!(response, RealityCheckResponse::RespondRefused { kind: RespondRefusalKind::SessionExpired, .. })
        })
        .count();

    assert_eq!(accepted, 1, "exactly one response should mutate session state: {first:?} {second:?}");
    assert_eq!(refused, 1, "stale concurrent response should be refused: {first:?} {second:?}");
    assert_eq!(
        fixture.substrate.read_memory(&id).await.expect("memory after").frontmatter.status,
        MemoryStatus::Tombstoned
    );
}

#[tokio::test]
async fn test_list_includes_encrypted_metadata_only_rows_without_title() {
    let fixture = Fixture::new().await;
    let id = fixture.write_encrypted_memory("Encrypted item", Scope::User).await;

    let response = fixture.reality_check(RealityCheckRequest::List { namespace: None, limit: Some(1) }).await;

    let RealityCheckResponse::Pending { items, total_scored, .. } = response else {
        panic!("expected pending response");
    };
    assert_eq!(total_scored, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].memory_id, id);
    assert!(items[0].encrypted);
    assert_eq!(items[0].title, "");
}

#[tokio::test]
async fn test_confirm_updates_encrypted_metadata_without_plaintext_body() {
    let fixture = Fixture::new().await;
    let id = fixture.write_encrypted_memory("Encrypted confirm item", Scope::User).await;
    let before = fixture.substrate.read_memory_envelope(&id).await.expect("encrypted before");
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::Confirm,
        })
        .await;

    assert!(matches!(response, RealityCheckResponse::RespondAccepted { .. }));
    let after = fixture.substrate.read_memory_envelope(&id).await.expect("encrypted after");
    assert_ciphertext_preserved(&before.content, &after.content);
    let observed_at = after.metadata.frontmatter.observed_at.expect("encrypted confirm persists observed_at");
    assert!(observed_at > before.metadata.frontmatter.updated_at);
    assert!((after.metadata.frontmatter.confidence - 0.82).abs() < 0.000_001);
    assert!(fixture.events().iter().any(|event| {
        matches!(&event.kind, EventKind::RealityCheckConfirmed { id: event_id, session_id: event_session }
            if event_id == &id && event_session == &session_id)
    }));
}

#[tokio::test]
async fn test_not_relevant_updates_encrypted_metadata_without_tombstone() {
    let fixture = Fixture::new().await;
    let id = fixture.write_encrypted_memory("Encrypted not relevant item", Scope::User).await;
    let before = fixture.substrate.read_memory_envelope(&id).await.expect("encrypted before");
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::NotRelevant,
        })
        .await;

    assert!(matches!(response, RealityCheckResponse::RespondAccepted { .. }));
    let after = fixture.substrate.read_memory_envelope(&id).await.expect("encrypted after");
    assert_ciphertext_preserved(&before.content, &after.content);
    assert_eq!(after.metadata.frontmatter.status, MemoryStatus::Active);
    assert!(!after.metadata.frontmatter.retrieval_policy.passive_recall);
    assert!(after.metadata.frontmatter.tags.iter().any(|tag| tag == "reality_check_not_relevant"));
    assert!(fixture.events().iter().any(|event| {
        matches!(&event.kind, EventKind::RealityCheckNotRelevant { id: event_id, session_id: event_session }
            if event_id == &id && event_session == &session_id)
    }));
    assert!(fixture
        .events()
        .iter()
        .all(|event| !matches!(&event.kind, EventKind::TombstoneCommitted { id: event_id } if event_id == &id)));
}

#[tokio::test]
async fn test_correct_issues_supersession() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Deployment target", "The old deployment target is staging.", Scope::Project).await;
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id,
            memory_id: id.clone(),
            action: RealityCheckAction::Correct { new_body: "The deployment target is production.".to_owned() },
        })
        .await;

    assert!(matches!(response, RealityCheckResponse::RespondAccepted { .. }));
    let old = fixture.substrate.read_memory(&id).await.expect("old memory");
    assert_eq!(old.frontmatter.status, MemoryStatus::Superseded);
    assert_eq!(old.frontmatter.superseded_by.len(), 1);
}

#[tokio::test]
async fn test_correct_governance_refusal_does_not_advance_session() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Correction refused", "The launch target is staging.", Scope::Project).await;
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::Correct {
                new_body: "The launch target is production and contact is ops@example.com.".to_owned(),
            },
        })
        .await;

    assert!(matches!(
        response,
        RealityCheckResponse::RespondRefused { kind: RespondRefusalKind::GovernanceRefused, .. }
    ));
    let session = RcSessionStore::new(fixture.runtime.path())
        .load_if_recent(Utc::now())
        .expect("session loads")
        .expect("session remains");
    assert!(session.items_remaining.iter().any(|remaining| remaining == id.as_str()));
}

#[tokio::test]
async fn test_skip_this_week_defers_without_frontmatter_mutation() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Skip item", "Skip body", Scope::User).await;
    let before = fixture.substrate.read_memory(&id).await.expect("memory before");
    let session_id = fixture.start_session().await;

    let response = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id,
            memory_id: id.clone(),
            action: RealityCheckAction::SkipThisWeek,
        })
        .await;

    assert!(matches!(
        response,
        RealityCheckResponse::RespondAccepted {
            completion: RealityCheckCompletion::Complete { deferred: 1, reviewed: 0, .. },
            ..
        }
    ));
    let after = fixture.substrate.read_memory(&id).await.expect("memory after");
    assert_eq!(after.frontmatter.confidence, before.frontmatter.confidence);
    assert_eq!(after.frontmatter.tags, before.frontmatter.tags);
    assert_eq!(after.frontmatter.status, before.frontmatter.status);
}

#[tokio::test]
async fn test_session_complete_updates_state_json() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("Complete item", "Complete body", Scope::User).await;
    let session_id = fixture.start_session().await;

    fixture
        .reality_check(RealityCheckRequest::Respond { session_id, memory_id: id, action: RealityCheckAction::Confirm })
        .await;

    let state = DaemonState::load(fixture.runtime.path());
    assert!(state.reality_check.last_completed_at.is_some());
    assert!(!fixture.runtime.path().join("state/reality-check-session.json").exists());
}

#[tokio::test]
async fn test_completed_session_is_persisted_to_history_with_action_counts() {
    let fixture = Fixture::new().await;
    let confirmed = fixture.write_memory("Confirmed item", "Confirmed body", Scope::User).await;
    let not_relevant = fixture.write_memory("Not relevant history item", "Not relevant body", Scope::User).await;
    let skipped = fixture.write_memory("Skipped history item", "Skipped body", Scope::User).await;
    let session_id = fixture.start_session().await;

    fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: confirmed,
            action: RealityCheckAction::Confirm,
        })
        .await;
    fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: not_relevant,
            action: RealityCheckAction::NotRelevant,
        })
        .await;
    let completed = fixture
        .reality_check(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: skipped,
            action: RealityCheckAction::SkipThisWeek,
        })
        .await;

    assert!(matches!(
        completed,
        RealityCheckResponse::RespondAccepted {
            completion: RealityCheckCompletion::Complete { reviewed: 2, deferred: 1, .. },
            ..
        }
    ));

    let history = fixture.reality_check(RealityCheckRequest::History { limit: Some(1) }).await;
    let RealityCheckResponse::History { sessions } = history else {
        panic!("expected history response");
    };
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    assert_eq!(session.session_id, session_id);
    assert_eq!(session.items_total, 3);
    assert_eq!(session.reviewed, 2);
    assert_eq!(session.confirmed, 1);
    assert_eq!(session.corrected, 0);
    assert_eq!(session.forgotten, 0);
    assert_eq!(session.not_relevant, 1);
    assert_eq!(session.skipped, 1);
    assert_eq!(session.deferred, 1);
    assert_eq!(session.remaining, 0);
}

#[tokio::test]
async fn test_completion_surfaces_history_persistence_failure() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("History persistence item", "History body", Scope::User).await;
    let session_id = fixture.start_session().await;
    let history_path = fixture.runtime.path().join("state/reality-check-history.json");
    std::fs::create_dir_all(&history_path).expect("history path replaced by directory");

    let response = fixture
        .reality_check_envelope(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id: id.clone(),
            action: RealityCheckAction::Confirm,
        })
        .await;

    assert!(
        matches!(response.result, ResponseResult::Error(_)),
        "history persistence errors must be surfaced to the caller"
    );
    let state = DaemonState::load(fixture.runtime.path());
    assert!(
        state.reality_check.last_completed_at.is_none(),
        "daemon state must not mark the session completed when history append fails"
    );
    let session = RcSessionStore::new(fixture.runtime.path())
        .load_if_recent(chrono::Utc::now())
        .expect("session load")
        .expect("session retained for operator recovery");
    assert!(
        !session.items_remaining.iter().any(|remaining| remaining == id.as_str()),
        "accepted action should not remain pending after history append failure"
    );
    assert!(session.items_confirmed.iter().any(|confirmed| confirmed == id.as_str()));

    std::fs::remove_dir(&history_path).expect("remove blocking history directory");
    let response = fixture
        .reality_check_envelope(RealityCheckRequest::Respond {
            session_id,
            memory_id: id,
            action: RealityCheckAction::Confirm,
        })
        .await;
    assert!(
        matches!(
            response.result,
            ResponseResult::Success(ResponsePayload::RealityCheck(RealityCheckResponse::RespondAccepted {
                completion: RealityCheckCompletion::Complete { .. },
                ..
            }))
        ),
        "same response should finalize a previously actioned completed session after storage recovers"
    );
    assert!(DaemonState::load(fixture.runtime.path()).reality_check.last_completed_at.is_some());
    assert!(!fixture.runtime.path().join("state/reality-check-session.json").exists());
}

struct Fixture {
    _repo: TempDir,
    runtime: TempDir,
    substrate: Substrate,
}

impl Fixture {
    async fn new() -> Self {
        let repo = tempfile::tempdir().expect("repo tempdir");
        let runtime = tempfile::tempdir().expect("runtime tempdir");
        let substrate = Substrate::init(
            Roots::new(repo.path(), runtime.path()),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_responses".to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _repo: repo, runtime, substrate }
    }

    async fn write_memory(&self, summary: &str, body: &str, scope: Scope) -> MemoryId {
        let id = self.substrate.next_memory_id().await.expect("id");
        let memory = sample_memory(id.clone(), summary, body, scope);
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("memory writes");
        id
    }

    async fn write_encrypted_memory(&self, summary: &str, scope: Scope) -> MemoryId {
        let id = self.substrate.next_memory_id().await.expect("id");
        let mut memory = sample_memory(id.clone(), summary, "", scope);
        memory.frontmatter.sensitivity = Sensitivity::Confidential;
        memory.frontmatter.retrieval_policy.index_body = false;
        memory.frontmatter.retrieval_policy.index_embeddings = false;
        memory.path = None;
        self.substrate
            .write_encrypted(EncryptedWriteRequest {
                operation_id: None,
                metadata_memory: memory,
                ciphertext: b"encrypted bytes".to_vec(),
                safe_index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::RequiresEncryption,
            })
            .await
            .expect("encrypted memory writes");
        id
    }

    async fn start_session(&self) -> String {
        match self.reality_check(RealityCheckRequest::Run { session_id: None, namespace: None, limit: None }).await {
            RealityCheckResponse::Pending { session_id: Some(session_id), .. } => session_id,
            other => panic!("expected session id, got {other:?}"),
        }
    }

    async fn start_session_with_state(&self, state: &HandlerState) -> String {
        match self
            .reality_check_with_state(
                state,
                RealityCheckRequest::Run { session_id: None, namespace: None, limit: None },
            )
            .await
        {
            RealityCheckResponse::Pending { session_id: Some(session_id), .. } => session_id,
            other => panic!("expected session id, got {other:?}"),
        }
    }

    async fn reality_check(&self, request: RealityCheckRequest) -> RealityCheckResponse {
        let response = self.reality_check_envelope(request).await;
        let ResponseResult::Success(ResponsePayload::RealityCheck(response)) = response.result else {
            panic!("expected reality check response, got {:?}", response.result);
        };
        response
    }

    async fn reality_check_envelope(&self, request: RealityCheckRequest) -> memoryd::protocol::ResponseEnvelope {
        handle_request(&self.substrate, RequestEnvelope::new("reality-check", RequestPayload::RealityCheck(request)))
            .await
    }

    async fn reality_check_with_state(
        &self,
        state: &HandlerState,
        request: RealityCheckRequest,
    ) -> RealityCheckResponse {
        let response = handle_request_with_state(
            &self.substrate,
            state,
            RequestEnvelope::new("reality-check", RequestPayload::RealityCheck(request)),
        )
        .await;
        let ResponseResult::Success(ResponsePayload::RealityCheck(response)) = response.result else {
            panic!("expected reality check response, got {:?}", response.result);
        };
        response
    }

    fn events(&self) -> Vec<memory_substrate::events::Event> {
        read_events(&self._repo.path().join("events/dev_responses.jsonl")).expect("events read")
    }
}

fn collect_repo_text(path: &std::path::Path) -> String {
    let mut out = String::new();
    collect_repo_text_inner(path, &mut out);
    out
}

fn collect_repo_text_inner(path: &std::path::Path, out: &mut String) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_repo_text_inner(&path, out);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            out.push_str(&text);
            out.push('\n');
        }
    }
}

fn assert_ciphertext_preserved(before: &MemoryContent, after: &MemoryContent) {
    let MemoryContent::Ciphertext { bytes: before_bytes, .. } = before else {
        panic!("expected encrypted ciphertext before mutation, got {before:?}");
    };
    let MemoryContent::Ciphertext { bytes: after_bytes, .. } = after else {
        panic!("expected encrypted ciphertext after mutation, got {after:?}");
    };
    assert_eq!(after_bytes, before_bytes);
}

fn sample_memory(id: MemoryId, summary: &str, body: &str, scope: Scope) -> Memory {
    let now = instant("2026-04-01T12:00:00Z");
    let path = match scope {
        Scope::User => RepoPath::new(format!("me/knowledge/{}.md", id.as_str())),
        Scope::Project => RepoPath::new(format!("projects/agent-memory/decisions/{}.md", id.as_str())),
        Scope::Agent | Scope::Subagent | Scope::Org => RepoPath::new(format!("agent/patterns/{}.md", id.as_str())),
    };
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id,
            memory_type: if scope == Scope::Project { MemoryType::Project } else { MemoryType::Pattern },
            scope,
            summary: summary.to_owned(),
            confidence: 0.8,
            original_confidence: Some(0.8),
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::User,
                user_handle: Some("memoryd-test".to_owned()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: None,
            },
            namespace: (scope == Scope::Project).then(|| "agent-memory".to_owned()),
            canonical_namespace_id: (scope == Scope::Project).then(|| "agent-memory".to_owned()),
            tags: vec!["reality-check-test".to_owned()],
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::User,
                reference: Some("responses-test".to_owned()),
                harness: None,
                harness_version: None,
                session_id: None,
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
                max_scope: scope,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "responses-test".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: body.to_owned(),
        path: Some(path),
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).unwrap().with_timezone(&Utc)
}
