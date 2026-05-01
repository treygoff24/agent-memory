use std::{sync::Arc, time::Duration};

use chrono::NaiveDate;
use memory_privacy::{PrivacyLabel, PrivacySpan};
use memoryd::dream::{
    harness::EchoCli,
    run::{DreamActiveMemoryInput, DreamRunOptions, DreamRunner, DreamSubstrateFragmentInput, NoopCandidateWriter},
    scope::DreamScope,
    types::{ActiveMemory, SubstrateFragment},
};

#[tokio::test]
async fn pass_1_with_echo_cli_writes_masked_journal_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview prompt");
    let harness =
        Arc::new(EchoCli::from_prompt_outputs([(pass_1_prompt.as_str(), "# Why\nPerson_A keeps seeing auth drift.")]));
    let runner = DreamRunner::new(options.with_harness(harness), NoopCandidateWriter);

    let report = runner.run().await.expect("dream run completes");

    assert_eq!(report.pass_1.status, memoryd::protocol::PassStatus::Success);
    let journal_path = temp.path().join("dreams/journal/project/proj_abc/2026-04-30.md");
    let journal = std::fs::read_to_string(journal_path).expect("journal file");
    assert!(journal.contains("Person_A"));
    assert!(!journal.contains("Alice"), "Pass 1 output must stay masked: {journal}");
}

fn base_options(repo_root: &std::path::Path) -> DreamRunOptions {
    DreamRunOptions {
        repo_root: repo_root.to_path_buf(),
        scope: DreamScope::parse("project:proj_abc").expect("scope"),
        run_date: NaiveDate::from_ymd_opt(2026, 4, 30).expect("date"),
        run_id: "run_test_01".to_string(),
        harness: Arc::new(EchoCli::default()),
        pass_timeout: Duration::from_secs(1),
        pass_2_max_candidates: 8,
        substrate_fragments: vec![DreamSubstrateFragmentInput {
            fragment: SubstrateFragment {
                id: "sub_01".to_string(),
                kind: "pattern".to_string(),
                ts: "2026-04-30T12:00:00Z".to_string(),
                entities: vec!["ent_auth_flow".to_string()],
                text: "Alice saw JWT rotation fail three times.".to_string(),
            },
            text_spans: vec![PrivacySpan::new(PrivacyLabel::PrivatePerson, 0, 5, 0.95)],
        }],
        active_memories: vec![DreamActiveMemoryInput {
            memory: ActiveMemory {
                id: "mem_01".to_string(),
                namespace: "project:proj_abc".to_string(),
                kind: "decision".to_string(),
                entities: vec!["ent_auth_flow".to_string()],
                summary: "JWT verification belongs behind one seam.".to_string(),
            },
            summary_spans: Vec::new(),
        }],
        previous_questions: Vec::new(),
    }
}

#[tokio::test]
async fn pass_2_receives_evidence_catalog_and_accepts_valid_refs_into_candidate_queue_under_dreaming_strict() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    assert!(pass_2_prompt.contains("evidence_catalog"));
    assert!(pass_2_prompt.contains("sub_01"));

    let pass_2_output = r#"[
      {
        "claim": "Person_A should centralize JWT verification.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [{"kind": "substrate_fragment", "ref": "sub_01", "excerpt": "Person_A saw JWT rotation fail three times."}],
        "confidence": 0.86,
        "rationale": "Person_A saw the pattern repeatedly."
      }
    ]"#;
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), pass_2_output),
    ]));
    let writer = RecordingCandidateWriter::default();
    let writes = writer.writes.clone();
    let runner = DreamRunner::new(options.with_harness(harness), writer);

    let report = runner.run().await.expect("dream run completes");

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Success);
    assert_eq!(report.pass_2.error_code, None);
    assert_eq!(report.pass_2.candidate_results.len(), 1);
    assert!(report.pass_2.candidate_results[0].accepted);
    assert_eq!(report.pass_2.candidate_results[0].source_ref_count, 1);

    let writes = writes.lock().expect("writes lock");
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].policy, "dreaming-strict");
    assert!(writes[0].grounding_rehydration_required);
    assert_eq!(writes[0].evidence[0].reference, "sub_01");
    assert_eq!(writes[0].evidence[0].excerpt.as_deref(), Some("Alice saw JWT rotation fail three times."));
    assert!(
        writes[0].claim.contains("Alice"),
        "Pass 2 fields must be restored before candidate write: {:?}",
        writes[0]
    );
}

#[derive(Clone, Default)]
struct RecordingCandidateWriter {
    writes: Arc<std::sync::Mutex<Vec<memoryd::dream::run::CandidateWriteRequest>>>,
}

impl memoryd::dream::run::CandidateWriter for RecordingCandidateWriter {
    fn write_candidate<'a>(
        &'a self,
        request: memoryd::dream::run::CandidateWriteRequest,
    ) -> memoryd::dream::harness::HarnessFuture<'a, memoryd::protocol::CandidateWriteResult> {
        Box::pin(async move {
            let source_ref_count = request.evidence.len();
            self.writes.lock().expect("writes lock").push(request);
            memoryd::protocol::CandidateWriteResult {
                id: Some(format!("cand_{source_ref_count}")),
                accepted: true,
                reason: None,
                source_ref_count,
            }
        })
    }
}

async fn run_pass_2_fixture(
    options: DreamRunOptions,
    pass_2_output: &str,
    writer: RecordingCandidateWriter,
) -> (memoryd::protocol::DreamRunReport, Arc<std::sync::Mutex<Vec<memoryd::dream::run::CandidateWriteRequest>>>) {
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    let pass_3_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(&options, pass_1_output).expect("preview pass 3");
    let pass_3_output = "{\"entities\":[\"ent_auth_flow\"],\"question\":\"What did Person_A miss?\"}\n";
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), pass_2_output),
        (pass_3_prompt.as_str(), pass_3_output),
    ]));
    let writes = writer.writes.clone();
    let runner = DreamRunner::new(options.with_harness(harness), writer);
    let report = runner.run().await.expect("dream run completes");
    (report, writes)
}

#[tokio::test]
async fn pass_2_rejects_empty_evidence_before_governance_and_marks_all_refused_skipped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pass_2_output = r#"[
      {
        "claim": "Person_A should centralize JWT verification.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [],
        "confidence": 0.86,
        "rationale": "The model cited nothing."
      }
    ]"#;

    let (report, writes) =
        run_pass_2_fixture(base_options(temp.path()), pass_2_output, RecordingCandidateWriter::default()).await;

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Skipped);
    assert_eq!(report.pass_2.error_code.as_deref(), Some("no_candidates_accepted"));
    assert_eq!(report.pass_2.candidate_results.len(), 1);
    assert!(!report.pass_2.candidate_results[0].accepted);
    assert_eq!(report.pass_2.candidate_results[0].reason.as_deref(), Some("missing_evidence_ref"));
    assert_eq!(report.pass_2.candidate_results[0].source_ref_count, 0);
    assert!(writes.lock().expect("writes lock").is_empty());
}

#[tokio::test]
async fn pass_2_rejects_out_of_scope_namespace_invalid_confidence_and_invalid_kind() {
    let cases = [
        (
            "out-of-scope namespace",
            r#"{
              "claim": "Person_A should centralize JWT verification.",
              "namespace": "me",
              "kind": "decision",
              "evidence": [{"kind": "substrate_fragment", "ref": "sub_01"}],
              "confidence": 0.86,
              "rationale": "Wrong scope."
            }"#,
            "out_of_scope_namespace",
            1,
        ),
        (
            "negative confidence",
            r#"{
              "claim": "Person_A should centralize JWT verification.",
              "namespace": "project:proj_abc",
              "kind": "decision",
              "evidence": [{"kind": "substrate_fragment", "ref": "sub_01"}],
              "confidence": -0.01,
              "rationale": "Bad confidence."
            }"#,
            "invalid_confidence",
            1,
        ),
        (
            "confidence above one",
            r#"{
              "claim": "Person_A should centralize JWT verification.",
              "namespace": "project:proj_abc",
              "kind": "decision",
              "evidence": [{"kind": "substrate_fragment", "ref": "sub_01"}],
              "confidence": 1.01,
              "rationale": "Bad confidence."
            }"#,
            "invalid_confidence",
            1,
        ),
        (
            "invalid kind",
            r#"{
              "claim": "Person_A should centralize JWT verification.",
              "namespace": "project:proj_abc",
              "kind": "made_up_kind",
              "evidence": [{"kind": "substrate_fragment", "ref": "sub_01"}],
              "confidence": 0.86,
              "rationale": "Unknown kind."
            }"#,
            "invalid_candidate_kind",
            1,
        ),
    ];

    for (label, candidate, expected_reason, expected_ref_count) in cases {
        let temp = tempfile::tempdir().expect("tempdir");
        let pass_2_output = format!("[{candidate}]");
        let (report, writes) =
            run_pass_2_fixture(base_options(temp.path()), &pass_2_output, RecordingCandidateWriter::default()).await;

        assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Skipped, "{label}");
        assert_eq!(report.pass_2.error_code.as_deref(), Some("no_candidates_accepted"), "{label}");
        assert_eq!(report.pass_2.candidate_results.len(), 1, "{label}");
        assert!(!report.pass_2.candidate_results[0].accepted, "{label}");
        assert_eq!(report.pass_2.candidate_results[0].reason.as_deref(), Some(expected_reason), "{label}");
        assert_eq!(report.pass_2.candidate_results[0].source_ref_count, expected_ref_count, "{label}");
        assert!(writes.lock().expect("writes lock").is_empty(), "{label}");
    }
}

#[tokio::test]
async fn pass_2_rejects_candidate_arrays_over_configured_cap_before_governance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut options = base_options(temp.path());
    options.pass_2_max_candidates = 1;
    let pass_2_output = r#"[
      {
        "claim": "Person_A should centralize JWT verification.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [{"kind": "substrate_fragment", "ref": "sub_01"}],
        "confidence": 0.86,
        "rationale": "First candidate."
      },
      {
        "claim": "Person_A should centralize JWT verification again.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [{"kind": "memory", "ref": "mem_01"}],
        "confidence": 0.72,
        "rationale": "Second candidate."
      }
    ]"#;

    let (report, writes) = run_pass_2_fixture(options, pass_2_output, RecordingCandidateWriter::default()).await;

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Skipped);
    assert_eq!(report.pass_2.error_code.as_deref(), Some("no_candidates_accepted"));
    assert_eq!(report.pass_2.candidate_results.len(), 2);
    assert!(report.pass_2.candidate_results.iter().all(|result| !result.accepted));
    assert!(report
        .pass_2
        .candidate_results
        .iter()
        .all(|result| result.reason.as_deref() == Some("too_many_candidates")));
    assert!(writes.lock().expect("writes lock").is_empty());
}

#[tokio::test]
async fn pass_2_empty_candidate_array_is_skipped_with_stable_error_code() {
    let temp = tempfile::tempdir().expect("tempdir");

    let (report, writes) =
        run_pass_2_fixture(base_options(temp.path()), "[]", RecordingCandidateWriter::default()).await;

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Skipped);
    assert_eq!(report.pass_2.error_code.as_deref(), Some("no_candidates_accepted"));
    assert!(report.pass_2.candidate_results.is_empty());
    assert!(writes.lock().expect("writes lock").is_empty());
}

#[tokio::test]
async fn pass_2_mixed_valid_and_invalid_candidates_succeeds_and_preserves_results() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pass_2_output = r#"[
      {
        "claim": "Person_A should centralize JWT verification.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [{"kind": "substrate_fragment", "ref": "sub_missing"}],
        "confidence": 0.86,
        "rationale": "The model invented a ref."
      },
      {
        "claim": "Person_A should centralize JWT verification.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [{"kind": "substrate_fragment", "ref": "sub_01", "excerpt": "Person_A saw JWT rotation fail three times."}],
        "confidence": 0.86,
        "rationale": "Person_A saw the pattern repeatedly."
      }
    ]"#;

    let (report, writes) =
        run_pass_2_fixture(base_options(temp.path()), pass_2_output, RecordingCandidateWriter::default()).await;

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Success);
    assert_eq!(report.pass_2.error_code, None);
    assert_eq!(report.pass_2.candidate_results.len(), 2);
    assert!(!report.pass_2.candidate_results[0].accepted);
    assert_eq!(
        report.pass_2.candidate_results[0].reason.as_deref(),
        Some("hallucinated_evidence_ref:substrate_fragment:sub_missing")
    );
    assert!(report.pass_2.candidate_results[1].accepted);

    let writes = writes.lock().expect("writes lock");
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].evidence[0].excerpt.as_deref(), Some("Alice saw JWT rotation fail three times."));
}

#[tokio::test]
async fn hallucinated_evidence_refs_are_rejected_before_governance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    let pass_2_output = r#"[
      {
        "claim": "Person_A should centralize JWT verification.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [{"kind": "substrate_fragment", "ref": "sub_missing"}],
        "confidence": 0.86,
        "rationale": "The model invented a ref."
      }
    ]"#;
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), pass_2_output),
    ]));
    let writer = RecordingCandidateWriter::default();
    let writes = writer.writes.clone();
    let runner = DreamRunner::new(options.with_harness(harness), writer);

    let report = runner.run().await.expect("dream run completes");

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Skipped);
    assert_eq!(report.pass_2.error_code.as_deref(), Some("no_candidates_accepted"));
    assert_eq!(report.pass_2.candidate_results.len(), 1);
    assert!(!report.pass_2.candidate_results[0].accepted);
    assert_eq!(
        report.pass_2.candidate_results[0].reason.as_deref(),
        Some("hallucinated_evidence_ref:substrate_fragment:sub_missing")
    );
    assert!(
        writes.lock().expect("writes lock").is_empty(),
        "invalid refs must be rejected before candidate writer/governance"
    );
}

#[tokio::test]
async fn pass_2_malformed_json_retries_once_then_fails_pass_2_while_pass_3_still_runs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    let pass_3_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(&options, pass_1_output).expect("preview pass 3");
    let pass_3_output = r#"{"entities":["ent_auth_flow"],"question":"What pattern is Person_A under-testing?"}
"#;
    let pass_2_attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let harness = Arc::new(CountingMalformedPass2Harness {
        pass_1_prompt,
        pass_1_output: pass_1_output.to_string(),
        pass_2_prompt,
        pass_3_prompt,
        pass_3_output: pass_3_output.to_string(),
        pass_2_attempts: pass_2_attempts.clone(),
    });
    let writer = RecordingCandidateWriter::default();
    let writes = writer.writes.clone();
    let runner = DreamRunner::new(options.with_harness(harness), writer);

    let report = runner.run().await.expect("dream run completes with partial failure");

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Failed);
    assert_eq!(report.pass_2.error_code.as_deref(), Some("malformed_pass_2_json"));
    assert_eq!(pass_2_attempts.load(std::sync::atomic::Ordering::SeqCst), 2, "Pass 2 should retry malformed JSON once");
    assert!(report.pass_2.candidate_results.is_empty());
    assert!(writes.lock().expect("writes lock").is_empty());
    assert_eq!(report.pass_3.status, memoryd::protocol::PassStatus::Success);
    let questions_path = temp.path().join("dreams/questions/project/proj_abc/2026-04-30.jsonl");
    let questions = std::fs::read_to_string(questions_path).expect("questions file");
    assert!(questions.contains("Person_A"));
    assert!(!questions.contains("Alice"));
}

struct CountingMalformedPass2Harness {
    pass_1_prompt: String,
    pass_1_output: String,
    pass_2_prompt: String,
    pass_3_prompt: String,
    pass_3_output: String,
    pass_2_attempts: Arc<std::sync::atomic::AtomicUsize>,
}

impl memoryd::dream::harness::HarnessCli for CountingMalformedPass2Harness {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn prompt_transport(&self) -> memoryd::protocol::PromptTransport {
        memoryd::protocol::PromptTransport::Stdin
    }

    fn is_installed(&self) -> bool {
        true
    }

    fn is_authenticated(
        &self,
    ) -> memoryd::dream::harness::HarnessFuture<'_, Result<bool, memoryd::dream::error::HarnessCliError>> {
        Box::pin(async { Ok(true) })
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a str,
        _expect_json: bool,
        _timeout: Duration,
    ) -> memoryd::dream::harness::HarnessFuture<'a, Result<String, memoryd::dream::error::HarnessCliError>> {
        Box::pin(async move {
            if prompt == self.pass_1_prompt {
                return Ok(self.pass_1_output.clone());
            }
            if prompt == self.pass_2_prompt {
                self.pass_2_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return Err(memoryd::dream::error::HarnessCliError::MalformedJson {
                    stage: memoryd::dream::error::JsonStage::Parse,
                    raw: "{not valid json".to_string(),
                });
            }
            if prompt == self.pass_3_prompt {
                return Ok(self.pass_3_output.clone());
            }
            Err(memoryd::dream::error::HarnessCliError::SubprocessExit {
                code: Some(1),
                stderr_tail: "unexpected prompt".to_string(),
            })
        })
    }
}

#[tokio::test]
async fn pass_3_jsonl_writes_only_records_with_non_empty_valid_entity_ids_and_masked_questions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    let pass_3_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(&options, pass_1_output).expect("preview pass 3");
    let pass_3_output = concat!(
        "{\"entities\":[\"ent_auth_flow\"],\"question\":\"What did Person_A miss about JWT drift?\"}\n",
        "{\"entities\":[],\"question\":\"Empty entities should not persist.\"}\n",
        "{\"entities\":[\"ent_hallucinated\"],\"question\":\"Hallucinated entity should not persist.\"}\n",
        "{\"entities\":[\"ent_auth_flow\"],\"question\":\"\"}\n",
        "{\"entities\":[\"ent_auth_flow\"],\"question\":\"What did Alice leak?\"}\n"
    );
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), "{not valid json"),
        (pass_3_prompt.as_str(), pass_3_output),
    ]));
    let counters = memoryd::dream::run::DreamQuestionCounters::default();
    let runner =
        DreamRunner::new(options.with_harness(harness), NoopCandidateWriter).with_question_counters(counters.clone());

    let report = runner.run().await.expect("dream run completes");

    assert_eq!(report.pass_3.status, memoryd::protocol::PassStatus::Success);
    let questions_path = temp.path().join("dreams/questions/project/proj_abc/2026-04-30.jsonl");
    let questions = std::fs::read_to_string(questions_path).expect("questions file");
    let lines = questions.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1, "only the valid masked record should persist: {questions}");
    assert!(lines[0].contains("ent_auth_flow"));
    assert!(lines[0].contains("Person_A"));
    assert!(!questions.contains("Alice"));
    assert!(!questions.contains("ent_hallucinated"));
    assert_eq!(counters.omitted("malformed_record"), 3);
    assert_eq!(counters.omitted("unsafe_fragment"), 1);
}

#[tokio::test]
async fn pass_3_hallucinated_entity_ids_are_discarded_by_unknown_entity_validation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    let pass_3_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(&options, pass_1_output).expect("preview pass 3");
    let pass_3_output = "{\"entities\":[\"ent_hallucinated\"],\"question\":\"Should not persist.\"}\n";
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), "{not valid json"),
        (pass_3_prompt.as_str(), pass_3_output),
    ]));
    let counters = memoryd::dream::run::DreamQuestionCounters::default();
    let runner =
        DreamRunner::new(options.with_harness(harness), NoopCandidateWriter).with_question_counters(counters.clone());

    let report = runner.run().await.expect("dream run completes");

    assert_eq!(report.pass_3.status, memoryd::protocol::PassStatus::Success);
    assert_eq!(counters.omitted("malformed_record"), 1);
    assert_eq!(counters.omitted("unsafe_fragment"), 0);
    let questions_path = temp.path().join("dreams/questions/project/proj_abc/2026-04-30.jsonl");
    let questions = std::fs::read_to_string(questions_path).expect("questions file");
    assert!(questions.trim().is_empty(), "hallucinated entity record must be discarded: {questions}");
}

#[tokio::test]
async fn pass_3_original_private_values_are_unsafe_omissions_not_malformed_records() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    let pass_3_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(&options, pass_1_output).expect("preview pass 3");
    let pass_3_output = "{\"entities\":[\"ent_auth_flow\"],\"question\":\"What did Alice leak?\"}\n";
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), "[]"),
        (pass_3_prompt.as_str(), pass_3_output),
    ]));
    let counters = memoryd::dream::run::DreamQuestionCounters::default();
    let runner =
        DreamRunner::new(options.with_harness(harness), NoopCandidateWriter).with_question_counters(counters.clone());

    let report = runner.run().await.expect("dream run completes");

    assert_eq!(report.pass_3.status, memoryd::protocol::PassStatus::Success);
    assert_eq!(counters.omitted("unsafe_fragment"), 1);
    assert_eq!(counters.omitted("malformed_record"), 0);
    let questions_path = temp.path().join("dreams/questions/project/proj_abc/2026-04-30.jsonl");
    let questions = std::fs::read_to_string(questions_path).expect("questions file");
    assert!(questions.trim().is_empty(), "original private value must not persist: {questions}");
}

#[tokio::test]
async fn unsafe_pass_1_output_fails_before_journal_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let unsafe_output = "# Journal\nContact alice@example.com with sk_live_1234567890abcdef.\n";
    let harness = Arc::new(EchoCli::from_prompt_outputs([(pass_1_prompt.as_str(), unsafe_output)]));
    let runner = DreamRunner::new(options.with_harness(harness), NoopCandidateWriter);

    let report = runner.run().await.expect("unsafe pass 1 is reported");

    assert_eq!(report.pass_1.status, memoryd::protocol::PassStatus::Failed);
    assert_eq!(report.pass_1.error_code.as_deref(), Some("unsafe_pass_1_output"));
    assert!(
        !temp.path().join("dreams/journal/project/proj_abc/2026-04-30.md").exists(),
        "unsafe Pass 1 output must not be persisted"
    );
}

#[tokio::test]
async fn unsafe_pass_3_questions_are_omitted_before_jsonl_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let pass_2_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&options, pass_1_output).expect("preview pass 2");
    let pass_3_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(&options, pass_1_output).expect("preview pass 3");
    let pass_3_output = "{\"entities\":[\"ent_auth_flow\"],\"question\":\"Should we email alice@example.com?\"}\n";
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), "[]"),
        (pass_3_prompt.as_str(), pass_3_output),
    ]));
    let counters = memoryd::dream::run::DreamQuestionCounters::default();
    let runner =
        DreamRunner::new(options.with_harness(harness), NoopCandidateWriter).with_question_counters(counters.clone());

    let report = runner.run().await.expect("dream run completes");

    assert_eq!(report.pass_3.status, memoryd::protocol::PassStatus::Success);
    assert_eq!(counters.omitted("unsafe_fragment"), 1);
    let questions_path = temp.path().join("dreams/questions/project/proj_abc/2026-04-30.jsonl");
    let questions = std::fs::read_to_string(questions_path).expect("questions file");
    assert!(questions.trim().is_empty(), "unsafe question must not persist: {questions}");
}

#[tokio::test]
async fn empty_pass_1_output_aborts_candidate_writing_and_drops_masking_session() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = base_options(temp.path());
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&options).expect("preview pass 1");
    let harness = Arc::new(EchoCli::from_prompt_outputs([(pass_1_prompt.as_str(), "  \n\t  ")]));
    let writer = RecordingCandidateWriter::default();
    let writes = writer.writes.clone();
    let observer = memoryd::dream::masking::MaskingDropObserver::default();
    let runner = DreamRunner::new(options.with_harness(harness), writer).with_masking_drop_observer(observer.clone());

    let report = runner.run().await.expect("dream run completes with empty pass 1 failure");

    assert_eq!(report.pass_1.status, memoryd::protocol::PassStatus::Failed);
    assert_eq!(report.pass_1.error_code.as_deref(), Some("empty_pass_1_output"));
    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Skipped);
    assert!(writes.lock().expect("writes lock").is_empty());
    assert_eq!(observer.drops(), 1, "MaskingSession wrapper must drop even on failure");
    assert!(!temp.path().join("dreams/journal/project/proj_abc/2026-04-30.md").exists());
}

#[tokio::test]
async fn masking_session_restore_restores_pass_2_fields_and_drop_runs_on_success_and_failure() {
    let success_temp = tempfile::tempdir().expect("success tempdir");
    let success_options = base_options(success_temp.path());
    let pass_1_output = "# Why\nPerson_A keeps seeing auth drift.";
    let pass_1_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&success_options).expect("preview pass 1");
    let pass_2_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(&success_options, pass_1_output)
        .expect("preview pass 2");
    let pass_3_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(&success_options, pass_1_output)
        .expect("preview pass 3");
    let pass_2_output = r#"[
      {
        "claim": "Person_A should centralize JWT verification.",
        "namespace": "project:proj_abc",
        "kind": "decision",
        "evidence": [{"kind": "substrate_fragment", "ref": "sub_01"}],
        "confidence": 0.86,
        "rationale": "Person_A saw the pattern repeatedly."
      }
    ]"#;
    let pass_3_output = "{\"entities\":[\"ent_auth_flow\"],\"question\":\"What did Person_A miss?\"}\n";
    let harness = Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), pass_2_output),
        (pass_3_prompt.as_str(), pass_3_output),
    ]));
    let writer = RecordingCandidateWriter::default();
    let writes = writer.writes.clone();
    let success_observer = memoryd::dream::masking::MaskingDropObserver::default();
    let runner = DreamRunner::new(success_options.with_harness(harness), writer)
        .with_masking_drop_observer(success_observer.clone());

    let report = runner.run().await.expect("successful dream run");

    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Success);
    assert_eq!(success_observer.drops(), 1, "MaskingSession wrapper must drop after success");
    {
        let writes = writes.lock().expect("writes lock");
        assert_eq!(writes.len(), 1);
        assert!(writes[0].claim.contains("Alice"));
        assert!(writes[0].rationale.contains("Alice"));
        assert!(!writes[0].claim.contains("Person_A"));
    }

    let failure_temp = tempfile::tempdir().expect("failure tempdir");
    let failure_options = base_options(failure_temp.path());
    let failure_prompt =
        DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(&failure_options).expect("preview pass 1");
    let failure_harness = Arc::new(EchoCli::from_prompt_outputs([(failure_prompt.as_str(), "")]));
    let failure_observer = memoryd::dream::masking::MaskingDropObserver::default();
    let failure_runner = DreamRunner::new(failure_options.with_harness(failure_harness), NoopCandidateWriter)
        .with_masking_drop_observer(failure_observer.clone());

    let failure_report = failure_runner.run().await.expect("failure path still reports");

    assert_eq!(failure_report.pass_1.status, memoryd::protocol::PassStatus::Failed);
    assert_eq!(failure_observer.drops(), 1, "MaskingSession wrapper must drop after failure");
}
