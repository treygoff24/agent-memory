use chrono::{TimeZone, Utc};
use memory_substrate::{AuthorKind, EventId, MemoryId, MemoryStatus, Sensitivity, SourceKind};
use memoryd::protocol::{
    CandidateWriteResult, CaptureSourceResponse, CaptureStatus, ComponentScores, DashboardRoiResponse, DreamRunReport,
    DreamStatusCounters, DreamStatusReport, DreamingRoiSummary, GetProvenance, GetResponse, GovernanceForgetResponse,
    GovernanceStatus, GovernanceSupersedeResponse, GovernanceWriteResponse, HarnessCliStatus, LeaseRecord, ObserveKind,
    ObserveResponse, ObserveTarget, PassOutcome, PassStatus, PromptTransport, RealityCheckAction,
    RealityCheckAdherenceSummary, RealityCheckHistorySession, RealityCheckItem, RealityCheckRequest,
    RealityCheckResponse, RecallHitSummary, RecallHitsResponse, RequestEnvelope, RequestPayload, ResponseEnvelope,
    ResponsePayload, ResponseResult, RevealResponse, ScopeRunSummary, SearchHit, SearchResponse, SourceCapturePayload,
    WriteNoteResponse,
};
use memoryd::recall::StartupRequest;

#[test]
fn protocol_contract_round_trips_request_variants_as_snake_case_json() {
    let requests = [
        RequestEnvelope::new("req-status", RequestPayload::Status),
        RequestEnvelope::new("req-doctor", RequestPayload::Doctor),
        RequestEnvelope::new(
            "req-search",
            RequestPayload::Search { query: "daemon socket protocol".to_owned(), limit: Some(5), include_body: false },
        ),
        RequestEnvelope::new(
            "req-get",
            RequestPayload::Get { id: "mem_20260428_0123456789abcdef_000001".to_owned(), include_provenance: true },
        ),
        RequestEnvelope::new(
            "req-trust-artifact",
            RequestPayload::TrustArtifact { id: "mem_20260428_0123456789abcdef_000001".to_owned() },
        ),
        RequestEnvelope::new(
            "req-capture-source",
            RequestPayload::CaptureSource(SourceCapturePayload {
                source: "https://example.com/report".to_owned(),
                mode: Default::default(),
                excerpts: vec!["exact quote".to_owned()],
                note: Some("operator note".to_owned()),
                local_path: None,
            }),
        ),
        RequestEnvelope::new(
            "req-recall-hits",
            RequestPayload::RecallHits {
                since: Some(Utc.with_ymd_and_hms(2026, 5, 2, 0, 0, 0).unwrap()),
                limit: Some(5),
            },
        ),
        RequestEnvelope::new(
            "req-reveal",
            RequestPayload::Reveal {
                id: "mem_20260428_0123456789abcdef_000001".to_owned(),
                reason: "user asked for encrypted contact".to_owned(),
            },
        ),
        RequestEnvelope::new(
            "req-write-note",
            RequestPayload::WriteNote { text: "observed a useful pattern".to_owned() },
        ),
        RequestEnvelope::new(
            "req-write-memory",
            RequestPayload::WriteMemory {
                body: "governed body".to_owned(),
                title: Some("Governed body".to_owned()),
                tags: vec!["governed".to_owned()],
                meta: serde_json::json!({ "namespace": "project" }),
            },
        ),
        RequestEnvelope::new(
            "req-supersede",
            RequestPayload::Supersede {
                old_id: "mem_20260428_0123456789abcdef_000001".to_owned(),
                content: "replacement body".to_owned(),
                reason: "stale".to_owned(),
                meta: serde_json::Value::Null,
            },
        ),
        RequestEnvelope::new(
            "req-forget",
            RequestPayload::Forget {
                id: "mem_20260428_0123456789abcdef_000001".to_owned(),
                reason: "user requested removal".to_owned(),
            },
        ),
        RequestEnvelope::new(
            "req-startup",
            RequestPayload::Startup(StartupRequest {
                cwd: "/tmp/agent-memory".to_owned(),
                session_id: "sess_protocol".to_owned(),
                harness: "codex".to_owned(),
                harness_version: Some("0.0.0".to_owned()),
                include_recent: true,
                since_event_id: None,
                budget_tokens: Some(3_600),
                passive: false,
            }),
        ),
    ];

    for request in requests {
        let line = request.to_json_line().expect("request serializes");
        assert!(line.ends_with('\n'), "protocol frames are newline-delimited");
        assert!(!line[..line.len() - 1].contains('\n'), "one JSON value per line");

        let value: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert!(value.get("request").is_some(), "request envelope contains request field");
        assert!(!line.contains("WriteNote"), "variant names are snake_case");

        let decoded = RequestEnvelope::from_json_line(&line).expect("request deserializes");
        assert_eq!(decoded, request);
    }
}

#[test]
fn protocol_contract_capture_source_response_round_trips() {
    let captured_at = Utc.with_ymd_and_hms(2026, 5, 5, 18, 0, 0).unwrap();
    let response = ResponseEnvelope::success(
        "req-capture-source",
        ResponsePayload::CaptureSource(CaptureSourceResponse {
            artifact_id: "src_01J0Z7Y8Q9R0ABCDE123456789".to_owned(),
            source_refs: vec!["webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001".to_owned()],
            mode: memoryd::protocol::CaptureSourceMode::HttpStatic,
            final_url: "https://example.com/report".to_owned(),
            captured_at,
            capture_status: CaptureStatus::CompleteTextOnly,
            warnings: vec!["raw_omitted_privacy".to_owned()],
        }),
    );

    let line = response.to_json_line().expect("capture source response serializes");
    let decoded = ResponseEnvelope::from_json_line(&line).expect("capture source response deserializes");

    assert_eq!(decoded, response);
    assert!(line.contains("\"capture_source\""));
    assert!(line.contains("webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001"));
}

#[test]
fn protocol_contract_recall_hits_response_round_trips_daemon_dto() {
    let response = ResponseEnvelope::success(
        "req-recall-hits",
        ResponsePayload::RecallHits(RecallHitsResponse {
            since: None,
            limit: 1,
            hits: vec![RecallHitSummary {
                event_id: "evt_recall_hit".to_owned(),
                device: "dev_protocol".to_owned(),
                seq: 7,
                memory_id: MemoryId::new("mem_20260502_0123456789abcdef_000001"),
                recalled_at: Utc.with_ymd_and_hms(2026, 5, 2, 12, 0, 0).unwrap(),
                summary: Some("Protocol recall-hit fixture".to_owned()),
            }],
        }),
    );

    let line = response.to_json_line().expect("recall hits response serializes");
    let decoded = ResponseEnvelope::from_json_line(&line).expect("recall hits response deserializes");

    assert_eq!(decoded, response);
    assert!(line.contains("\"recall_hits\""));
    assert!(line.contains("Protocol recall-hit fixture"));
}

#[test]
fn protocol_contract_dashboard_roi_request_and_response_round_trip() {
    let request = RequestPayload::DashboardRoi { window_days: 90 };

    let decoded = round_trip(&request);

    assert_eq!(decoded, request);
    let request_json = serde_json::to_string(&request).expect("dashboard ROI request serializes");
    assert!(request_json.contains("dashboard_roi"));
    assert!(request_json.contains("window_days"));

    let response = ResponseEnvelope::success(
        "req-dashboard-roi",
        ResponsePayload::DashboardRoi(DashboardRoiResponse {
            window_days: 90,
            promotion_rate: 0.5,
            promotion_precision: 1.0,
            refusal_breakdown: std::collections::BTreeMap::from([("grounding".to_owned(), 2)]),
            dreaming: DreamingRoiSummary {
                candidates_generated: 4,
                promoted_silent: 1,
                entered_review_queue: 2,
                dropped: 1,
                review_queue_approval_rate: 0.5,
            },
            reality_check_adherence: RealityCheckAdherenceSummary { weeks_completed: 3, weeks_skipped: 1 },
        }),
    );

    let line = response.to_json_line().expect("dashboard ROI response serializes");
    let decoded = ResponseEnvelope::from_json_line(&line).expect("dashboard ROI response deserializes");

    assert_eq!(decoded, response);
    assert!(line.contains("\"dashboard_roi\""));
    assert!(line.contains("\"promotion_rate\""));
}

#[test]
fn protocol_contract_status_recall_is_json_additive() {
    let legacy = r#"{"state":"ready","guidance":"legacy status"}"#;
    let decoded: memoryd::protocol::StatusResponse = serde_json::from_str(legacy).expect("legacy status decodes");
    assert_eq!(decoded.recall.startup_invoked_total, 0);
    assert!(decoded.recall.startup_failed_total.is_empty());
    assert_eq!(decoded.dreams.dream_runs_invoked_total, 0);
    assert!(decoded.dreams.substrate_fragments_written_total.is_empty());
    assert!(decoded.passive_notifications.is_empty());

    let encoded = serde_json::to_value(decoded).expect("status encodes");
    assert!(encoded.get("recall").is_some(), "new status responses always serialize recall counters");
    assert!(encoded.get("dreams").is_some(), "new status responses always serialize dream counters");
    assert!(encoded.get("passive_notifications").is_some(), "status serializes passive notifications");
}

#[test]
fn protocol_contract_trust_artifact_response_round_trips_daemon_dto() {
    let response = ResponseEnvelope::success(
        "req-trust-artifact",
        ResponsePayload::TrustArtifact(Box::new(sample_trust_artifact())),
    );

    let line = response.to_json_line().expect("trust artifact response serializes");
    let decoded = ResponseEnvelope::from_json_line(&line).expect("trust artifact response deserializes");

    assert_eq!(decoded, response);
    assert!(line.contains("\"trust_artifact\""));
    assert!(line.contains("mem_20260501_0123456789abcdef_000009"));
}

#[test]
fn test_reality_check_request_list_round_trips_serde() {
    let request = RequestPayload::RealityCheck(RealityCheckRequest::List { namespace: None, limit: Some(12) });

    let decoded = round_trip(&request);

    assert_eq!(decoded, request);
}

#[test]
fn test_reality_check_request_history_round_trips_serde() {
    let request = RequestPayload::RealityCheck(RealityCheckRequest::History { limit: Some(4) });

    let decoded = round_trip(&request);

    assert_eq!(decoded, request);
}

#[test]
fn test_reality_check_request_run_round_trips_serde() {
    let request = RequestPayload::RealityCheck(RealityCheckRequest::Run {
        session_id: None,
        namespace: Some("me".to_owned()),
        limit: None,
    });

    let decoded = round_trip(&request);

    assert_eq!(decoded, request);
}

#[test]
fn test_reality_check_request_respond_round_trips_serde() {
    let memory_id = MemoryId::new("mem_20260501_0123456789abcdef_000001");
    let request = RequestPayload::RealityCheck(RealityCheckRequest::Respond {
        session_id: "s1".to_owned(),
        memory_id,
        action: RealityCheckAction::Confirm,
    });

    let decoded = round_trip(&request);

    assert_eq!(decoded, request);
}

#[test]
fn test_web_dashboard_requests_round_trip_serde() {
    for request in [
        RequestPayload::WebEnable { port: 7137, socket_path: "/tmp/memoryd.sock".to_owned() },
        RequestPayload::WebDisable,
        RequestPayload::WebStatus,
    ] {
        let decoded = round_trip(&request);

        assert_eq!(decoded, request);
    }
}

#[test]
fn test_tui_read_only_protocol_requests_round_trip_serde() {
    let requests = [
        RequestPayload::InspectEntities { limit: Some(10), prefix: Some("ent_project".to_owned()) },
        RequestPayload::EventsLogPage { since: Some(EventId::new("evt_cursor")), limit: 25, kind_filter: None },
        RequestPayload::NamespaceTree { root: Some("project:agent-memory".to_owned()), depth: Some(2) },
        RequestPayload::GovernancePolicyDump,
        RequestPayload::ConflictsList { limit: Some(5) },
    ];

    for request in requests {
        let decoded = round_trip(&request);

        assert_eq!(decoded, request);
    }
}

#[tokio::test]
async fn test_tui_read_only_protocol_requests_are_rejected_by_mcp_forwarder() {
    let socket_path = tempfile::tempdir().expect("tempdir").path().join("missing.sock");
    let requests = [
        RequestPayload::InspectEntities { limit: None, prefix: None },
        RequestPayload::EventsLogPage { since: None, limit: 10, kind_filter: None },
        RequestPayload::NamespaceTree { root: None, depth: None },
        RequestPayload::GovernancePolicyDump,
        RequestPayload::ConflictsList { limit: None },
    ];

    for request in requests {
        let response = memoryd::mcp::forward_payload_to_daemon(&socket_path, "req-tui", request)
            .await
            .expect("mcp rejection is local and does not touch socket");
        let ResponseResult::Error(error) = response.result else {
            panic!("expected MCP method_not_allowed error");
        };
        assert_eq!(error.code, "method_not_allowed_on_mcp");
    }
}

#[test]
fn test_reality_check_request_skip_round_trips_serde() {
    let request = RequestPayload::RealityCheck(RealityCheckRequest::Skip);

    let decoded = round_trip(&request);

    assert_eq!(decoded, request);
}

#[test]
fn test_reality_check_request_snooze_until_round_trips_serde() {
    let request = RequestPayload::RealityCheck(RealityCheckRequest::Snooze {
        until: Some(Utc.with_ymd_and_hms(2026, 5, 10, 0, 0, 0).unwrap()),
    });

    let decoded = round_trip(&request);

    assert_eq!(decoded, request);
}

#[test]
fn test_reality_check_response_pending_round_trips_serde() {
    let response = ResponsePayload::RealityCheck(RealityCheckResponse::Pending {
        session_id: None,
        items: vec![],
        total_scored: 5,
        last_completed_at: None,
    });

    let decoded = round_trip(&response);

    assert_eq!(decoded, response);
}

#[test]
fn test_reality_check_response_history_round_trips_serde() {
    let response = ResponsePayload::RealityCheck(RealityCheckResponse::History {
        sessions: vec![RealityCheckHistorySession {
            session_id: "rcs_20260522_120000".to_owned(),
            started_at: Utc.with_ymd_and_hms(2026, 5, 22, 12, 0, 0).unwrap(),
            completed_at: Utc.with_ymd_and_hms(2026, 5, 22, 12, 3, 0).unwrap(),
            items_total: 3,
            reviewed: 2,
            confirmed: 1,
            corrected: 0,
            forgotten: 0,
            not_relevant: 1,
            deferred: 1,
            remaining: 0,
        }],
    });

    let decoded = round_trip(&response);

    assert_eq!(decoded, response);
}

#[test]
fn test_reality_check_item_component_scores_round_trips_serde() {
    let last_observed_at = Utc.with_ymd_and_hms(2026, 5, 1, 9, 0, 0).unwrap();
    let last_recalled_at = Utc.with_ymd_and_hms(2026, 5, 1, 10, 0, 0).unwrap();
    let item = RealityCheckItem {
        memory_id: MemoryId::new("mem_20260501_0123456789abcdef_000001"),
        title: "Contract-driven APIs need stable errors".to_owned(),
        namespace: "project".to_owned(),
        status: MemoryStatus::Active,
        sensitivity: Some(Sensitivity::Internal),
        score: 0.74,
        component_scores: ComponentScores {
            days_since_observed_norm: 0.20,
            recall_frequency_norm: 0.30,
            cross_source_corroboration: 1.0,
            confidence_decay: 0.40,
            sensitivity_weight: 0.50,
        },
        encrypted: false,
        last_observed_at,
        recall_count_30d: 7,
        last_recalled_at: Some(last_recalled_at),
    };

    let json = serde_json::to_value(&item).expect("reality check item serializes");

    assert_eq!(json["component_scores"]["days_since_observed_norm"], 0.20);
    assert_eq!(json["component_scores"]["recall_frequency_norm"], 0.30);
    assert_eq!(json["component_scores"]["cross_source_corroboration"], 1.0);
    assert_eq!(json["component_scores"]["confidence_decay"], 0.40);
    assert_eq!(json["component_scores"]["sensitivity_weight"], 0.50);
    assert_eq!(round_trip(&item), item);
}

#[test]
fn test_existing_protocol_variants_unchanged() {
    let existing_shapes = [
        (serde_json::to_value(RequestPayload::Status).unwrap(), serde_json::json!("status")),
        (
            serde_json::to_value(RequestPayload::Search {
                query: "stable".to_owned(),
                limit: Some(5),
                include_body: false,
            })
            .unwrap(),
            serde_json::json!({"search":{"query":"stable","limit":5,"include_body":false}}),
        ),
        (
            serde_json::to_value(RequestPayload::Startup(StartupRequest {
                cwd: "/tmp/agent-memory".to_owned(),
                session_id: "sess_protocol".to_owned(),
                harness: "codex".to_owned(),
                harness_version: None,
                include_recent: true,
                since_event_id: None,
                budget_tokens: Some(3_600),
                passive: false,
            }))
            .unwrap(),
            serde_json::json!({"startup":{
                "cwd":"/tmp/agent-memory",
                "session_id":"sess_protocol",
                "harness":"codex",
                "harness_version":null,
                "include_recent":true,
                "since_event_id":null,
                "budget_tokens":3600
            }}),
        ),
        (
            serde_json::to_value(RequestPayload::WriteMemory {
                body: "body".to_owned(),
                title: Some("Title".to_owned()),
                tags: vec!["tag".to_owned()],
                meta: serde_json::json!({"namespace":"project"}),
            })
            .unwrap(),
            serde_json::json!({"write_memory":{
                "body":"body",
                "title":"Title",
                "tags":["tag"],
                "meta":{"namespace":"project"}
            }}),
        ),
        (
            serde_json::to_value(RequestPayload::Supersede {
                old_id: "mem_20260428_0123456789abcdef_000001".to_owned(),
                content: "replacement".to_owned(),
                reason: "stale".to_owned(),
                meta: serde_json::Value::Null,
            })
            .unwrap(),
            serde_json::json!({"supersede":{
                "old_id":"mem_20260428_0123456789abcdef_000001",
                "content":"replacement",
                "reason":"stale",
                "meta":null
            }}),
        ),
        (
            serde_json::to_value(RequestPayload::Forget {
                id: "mem_20260428_0123456789abcdef_000001".to_owned(),
                reason: "user requested removal".to_owned(),
            })
            .unwrap(),
            serde_json::json!({"forget":{
                "id":"mem_20260428_0123456789abcdef_000001",
                "reason":"user requested removal"
            }}),
        ),
    ];

    for (actual, expected) in existing_shapes {
        assert_eq!(actual, expected);
    }
}

#[test]
fn protocol_contract_stream_f_dreaming_dtos_round_trip_as_snake_case_json() {
    let acquired_at = Utc.with_ymd_and_hms(2026, 4, 30, 3, 0, 0).unwrap();
    let expires_at = Utc.with_ymd_and_hms(2026, 4, 30, 4, 0, 0).unwrap();

    let accepted_candidate = CandidateWriteResult {
        id: Some("mem_20260430_a1b2c3d4e5f60718_000001".to_owned()),
        accepted: true,
        reason: None,
        source_ref_count: 2,
    };
    let refused_candidate = CandidateWriteResult {
        id: None,
        accepted: false,
        reason: Some("missing_grounding".to_owned()),
        source_ref_count: 0,
    };
    let pass_1 = PassOutcome {
        status: PassStatus::Success,
        output_path: Some("dreams/journal/project/proj_abc/2026-04-30.md".to_owned()),
        candidate_results: Vec::new(),
        error_code: None,
        duration_ms: 125,
    };
    let pass_2 = PassOutcome {
        status: PassStatus::Failed,
        output_path: None,
        candidate_results: vec![accepted_candidate.clone(), refused_candidate.clone()],
        error_code: Some("missing_grounding".to_owned()),
        duration_ms: 250,
    };
    let pass_3 = PassOutcome {
        status: PassStatus::Skipped,
        output_path: Some("dreams/questions/project/proj_abc/2026-04-30.jsonl".to_owned()),
        candidate_results: Vec::new(),
        error_code: None,
        duration_ms: 75,
    };
    let dream_run = DreamRunReport {
        scope: "project:proj_abc".to_owned(),
        cli_used: Some("codex".to_owned()),
        pass_1: pass_1.clone(),
        pass_2: pass_2.clone(),
        pass_2_refusal_counts_by_reason: std::collections::BTreeMap::from([("missing_grounding".to_owned(), 1)]),
        pass_3: pass_3.clone(),
        duration_ms: 450,
    };
    let lease = LeaseRecord {
        device: "dev_local".to_owned(),
        scope: "project:proj_abc".to_owned(),
        acquired_at,
        expires_at,
        run_id: "dream_run_20260430_030000".to_owned(),
    };
    let cli = HarnessCliStatus {
        name: "codex".to_owned(),
        is_installed: true,
        is_authenticated: Some(true),
        prompt_transport: PromptTransport::Stdin,
        last_probe_at: Some(acquired_at),
        last_probe_error: None,
    };
    let scope_summary = ScopeRunSummary {
        scope: "project:proj_abc".to_owned(),
        last_run_at: Some(acquired_at),
        last_run_outcome: Some(PassStatus::Failed),
        last_run_cli: Some("codex".to_owned()),
        consecutive_missed_runs: 1,
    };
    let mut counters = DreamStatusCounters::default();
    counters.substrate_fragments_written_total.insert("observation".to_owned(), 3);
    counters.dream_runs_invoked_total = 1;
    counters.dream_runs_failed_total.insert("missing_grounding".to_owned(), 1);
    counters.pass_failed_total.insert("pass_2:missing_grounding".to_owned(), 1);
    counters.harness_cli_calls_total.insert("codex".to_owned(), 3);
    counters.harness_cli_auth_failures_total.insert("gemini".to_owned(), 1);
    counters.cleanup_runs_invoked_total = 1;
    counters.cleanup_findings_total.insert("stale_candidate".to_owned(), 2);
    let dream_status = DreamStatusReport {
        enabled: true,
        last_runs: vec![scope_summary.clone()],
        active_leases: vec![lease.clone()],
        cli_inventory: vec![cli.clone()],
        counters: counters.clone(),
        privacy_disclosure: "Dreaming sends masked prompts to configured local CLIs.".to_owned(),
    };

    let requests = [
        RequestEnvelope::new(
            "req-observe",
            RequestPayload::Observe {
                text: "raw observation for Stream F".to_owned(),
                kind: ObserveKind::Observation,
                entities: vec!["ent_proj_abc".to_owned()],
                cwd: "/tmp/agent-memory".to_owned(),
                session_id: "sess_protocol".to_owned(),
                harness: "codex".to_owned(),
                harness_version: Some("0.0.0".to_owned()),
            },
        ),
        RequestEnvelope::new(
            "req-dream-now",
            RequestPayload::DreamNow {
                scope: "project:proj_abc".to_owned(),
                force: true,
                cli_override: Some("codex".to_owned()),
            },
        ),
        RequestEnvelope::new("req-dream-status", RequestPayload::DreamStatus {}),
    ];

    for request in requests {
        let line = request.to_json_line().expect("stream f request serializes");
        assert!(!line.contains("DreamNow"), "request variants serialize as snake_case");
        let decoded = RequestEnvelope::from_json_line(&line).expect("stream f request deserializes");
        assert_eq!(decoded, request);
    }

    let responses = [
        ResponseEnvelope::success(
            "req-observe",
            ResponsePayload::Observe(ObserveResponse {
                fragment_id: "sub_01HWPRZK1SPRAWM6EVQ6Y0XS8R".to_owned(),
                target: ObserveTarget::PlaintextSubstrate,
            }),
        ),
        ResponseEnvelope::success("req-dream-now", ResponsePayload::DreamNow(Box::new(dream_run.clone()))),
        ResponseEnvelope::success("req-dream-status", ResponsePayload::DreamStatus(Box::new(dream_status.clone()))),
    ];

    for response in responses {
        let line = response.to_json_line().expect("stream f response serializes");
        assert!(line.contains("dream_now") || line.contains("dream_status") || line.contains("plaintext_substrate"));
        assert!(line.contains("prompt_transport") || line.contains("fragment_id") || line.contains("pass_1"));
        let decoded = ResponseEnvelope::from_json_line(&line).expect("stream f response deserializes");
        assert_eq!(decoded, response);
    }

    assert_eq!(serde_json::to_value(PromptTransport::Stdin).expect("transport serializes"), "stdin");
    assert_eq!(
        serde_json::to_value(ObserveTarget::PlaintextSubstrate).expect("target serializes"),
        "plaintext_substrate"
    );
    assert_eq!(serde_json::to_value(PassStatus::Failed).expect("pass status serializes"), "failed");

    assert_eq!(round_trip(&dream_run), dream_run);
    assert_eq!(round_trip(&pass_1), pass_1);
    assert_eq!(round_trip(&accepted_candidate), accepted_candidate);
    assert_eq!(round_trip(&dream_status), dream_status);
    assert_eq!(round_trip(&scope_summary), scope_summary);
    assert_eq!(round_trip(&cli), cli);
    assert_eq!(round_trip(&PromptTransport::Argv), PromptTransport::Argv);
    assert_eq!(round_trip(&lease), lease);
}

fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    serde_json::from_value(serde_json::to_value(value).expect("value serializes")).expect("value deserializes")
}

#[test]
fn protocol_contract_success_responses_are_bounded_and_guided() {
    let search = ResponseEnvelope::success(
        "req-search",
        ResponsePayload::Search(SearchResponse {
            hits: vec![SearchHit {
                id: "mem_20260428_0123456789abcdef_000001".to_owned(),
                summary: "Protocol DTOs are newline-delimited JSON.".to_owned(),
                snippet: "Protocol DTOs are newline-delimited JSON with bounded snippets.".to_owned(),
                body: None,
                score: 0.87,
            }],
            total: 1,
            guidance: "Bounded snippets only; call memory_get for full body.".to_owned(),
        }),
    );
    let get = ResponseEnvelope::success(
        "req-get",
        ResponsePayload::Get(GetResponse {
            id: "mem_20260428_0123456789abcdef_000001".to_owned(),
            summary: "Protocol DTOs are stable.".to_owned(),
            body: "Short bounded body preview.".to_owned(),
            truncated: true,
            provenance: Some(GetProvenance {
                path: Some("agent/patterns/mem_20260428_0123456789abcdef_000001.md".to_owned()),
                source_kind: SourceKind::Import,
                source_ref: Some("fixture".to_owned()),
                author_kind: AuthorKind::System,
                harness: None,
                session_id: None,
                evidence_refs: Vec::new(),
            }),
            guidance: "Body preview truncated; call memory_get for full body.".to_owned(),
        }),
    );
    let write = ResponseEnvelope::success(
        "req-write-note",
        ResponsePayload::WriteNote(WriteNoteResponse {
            id: "mem_20260428_0123456789abcdef_000002".to_owned(),
            summary: "Note accepted.".to_owned(),
        }),
    );
    let reveal = ResponseEnvelope::success(
        "req-reveal",
        ResponsePayload::Reveal(RevealResponse {
            id: "mem_20260428_0123456789abcdef_000001".to_owned(),
            summary: "Encrypted contact.".to_owned(),
            body: "Bounded decrypted body.".to_owned(),
            truncated: false,
            guidance: "Returned decrypted content through explicit memory_reveal.".to_owned(),
        }),
    );
    let governed_write = ResponseEnvelope::success(
        "req-write-memory",
        ResponsePayload::GovernanceWrite(GovernanceWriteResponse {
            status: GovernanceStatus::Promoted,
            id: Some("mem_20260428_0123456789abcdef_000003".to_owned()),
            namespace: Some("project".to_owned()),
            reason: None,
            next_actions: Vec::new(),
            policy_applied: Some("project-standard@v2".to_owned()),
            policy_source: Some("built_in_fallback".to_owned()),
            existing_id: None,
            similarity_degraded: None,
        }),
    );
    let supersede = ResponseEnvelope::success(
        "req-supersede",
        ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
            status: GovernanceStatus::Promoted,
            new_id: Some("mem_20260428_0123456789abcdef_000004".to_owned()),
            old_id: Some("mem_20260428_0123456789abcdef_000003".to_owned()),
            reason: None,
            chain: Some(serde_json::json!({ "supersedes": ["mem_20260428_0123456789abcdef_000003"] })),
            policy_applied: Some("project-standard@v2".to_owned()),
            policy_source: Some("built_in_fallback".to_owned()),
            warning: None,
        }),
    );
    let forget = ResponseEnvelope::success(
        "req-forget",
        ResponsePayload::GovernanceForget(GovernanceForgetResponse {
            status: GovernanceStatus::Tombstoned,
            id: "mem_20260428_0123456789abcdef_000004".to_owned(),
            tombstone_ref: Some("tombstone:stream-a".to_owned()),
            reason: None,
        }),
    );

    for response in [search, get, reveal, write, governed_write, supersede, forget] {
        let line = response.to_json_line().expect("response serializes");
        let decoded = ResponseEnvelope::from_json_line(&line).expect("response deserializes");
        assert_eq!(decoded, response);

        let json: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(json["result"].as_object().expect("result object").len(), 1);
        assert!(
            line.contains("call memory_get for full body")
                || line.contains("Note accepted")
                || line.contains("memory_reveal")
                || line.contains("promoted")
                || line.contains("tombstoned")
        );
    }
}

#[test]
fn protocol_contract_error_response_preserves_id_and_structured_error() {
    let response = ResponseEnvelope::error("req-bad", "invalid_request", "missing required search query", true);

    let line = response.to_json_line().expect("error response serializes");
    let decoded = ResponseEnvelope::from_json_line(&line).expect("error response deserializes");
    assert_eq!(decoded, response);

    let json: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
    assert_eq!(json["id"], "req-bad");
    assert_eq!(json["result"]["error"]["code"], "invalid_request");
    assert_eq!(json["result"]["error"]["retryable"], true);
}

#[test]
fn observe_request_payload_accepts_spec_shaped_json_without_binding_fields() {
    let payload: RequestPayload = serde_json::from_value(serde_json::json!({
        "observe": {
            "text": "raw observation for Stream F",
            "kind": "observation"
        }
    }))
    .expect("spec-shaped observe payload parses");

    let RequestPayload::Observe { text, kind, entities, cwd, session_id, harness, harness_version } = payload else {
        panic!("expected observe payload");
    };
    assert_eq!(text, "raw observation for Stream F");
    assert_eq!(kind, ObserveKind::Observation);
    assert!(entities.is_empty());
    assert!(!cwd.is_empty());
    assert_eq!(session_id, "synthetic-memory-observe");
    assert_eq!(harness, "unknown");
    assert_eq!(harness_version, None);
}

#[test]
fn protocol_contract_doctor_finding_round_trips_severity() {
    use memoryd::protocol::{DoctorFinding, DoctorSeverity};

    // Findings are computed fresh server-side with an explicit severity (never
    // persisted), so we only prove the field survives the wire round-trip for both
    // variants and serializes as snake_case. `DoctorSeverity::default() == Advisory`
    // is intentionally left as-is (only relevant to legacy JSON without the field).
    for (severity, wire) in [(DoctorSeverity::Fatal, "fatal"), (DoctorSeverity::Advisory, "advisory")] {
        let finding = DoctorFinding {
            code: "sync_blocked".to_owned(),
            message: "Sync is blocked".to_owned(),
            repair: Some("memoryd quarantine list".to_owned()),
            severity,
        };

        let value = serde_json::to_value(&finding).expect("doctor finding serializes");
        assert_eq!(value["severity"], wire, "severity serializes as snake_case");

        let decoded: DoctorFinding = serde_json::from_value(value).expect("doctor finding deserializes");
        assert_eq!(decoded, finding);
        assert_eq!(decoded.severity, severity, "severity survives the round-trip");
    }
}

fn sample_trust_artifact() -> memoryd::trust_artifact::TrustArtifact {
    serde_json::from_value(serde_json::json!({
        "id": "mem_20260501_0123456789abcdef_000009",
        "namespace": "project:atlasos",
        "status": "active",
        "sensitivity": "internal",
        "source": "substrate:projects/atlasos/deploy-target.md",
        "title": {
            "kind": "plaintext",
            "value": "Deploy target is production ECS"
        },
        "body": {
            "kind": "plaintext",
            "value": "Daemon-built trust artifact body."
        },
        "current_confidence": "0.95",
        "original_confidence": "0.90",
        "confidence_reason": "user confirmed",
        "trust_summary": "trusted / project-standard@v2",
        "recall": {
            "total": 1,
            "last_30_days": 1,
            "last_recalled_at": "2026-05-01T11:02:00Z",
            "strength": "0.42"
        },
        "provenance_chain": [],
        "policy_decisions": [],
        "privacy_scan": {
            "labels_detected": ["none"],
            "storage_action": "plaintext"
        },
        "supersedes": [],
        "superseded_by": [],
        "sync_state": {
            "devices": ["macbook"],
            "merge_status": "clean",
            "claim_lock_status": null
        }
    }))
    .expect("sample trust artifact fixture matches daemon DTO")
}
