use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use serde::Deserialize;

use crate::protocol::{CandidateWriteResult, PassOutcome, PassStatus};
use memory_substrate::config::PromptVersion;

use super::{
    error::HarnessCliError,
    harness::HarnessCli,
    masking::DreamMaskingSession,
    prompts::{render_prompt, DreamPromptInput},
    run::{CandidateEvidenceRef, CandidateWriteRequest, CandidateWriter},
    types::{DreamError, DreamPass, EvidenceCatalogEntry},
};

pub struct Pass2RunContext<'a, W> {
    pub cli: &'a dyn HarnessCli,
    pub writer: &'a W,
    pub masking: &'a DreamMaskingSession,
    pub input: &'a DreamPromptInput,
    pub prompt_version: PromptVersion,
    pub timeout: Duration,
    pub candidate_cap: usize,
}

pub async fn run_pass_2<W>(context: Pass2RunContext<'_, W>) -> Result<PassOutcome, DreamError>
where
    W: CandidateWriter,
{
    let Pass2RunContext { cli, writer, masking, input, prompt_version, timeout, candidate_cap } = context;
    let started_at = Instant::now();
    let prompt = render_prompt(DreamPass::Pass2, input, prompt_version)?;
    let proposals = match complete_and_parse_with_retry(cli, &prompt, timeout).await? {
        Pass2Parse::Candidates(candidates) => candidates,
        Pass2Parse::MalformedAfterRetry => {
            return Ok(PassOutcome {
                status: PassStatus::Failed,
                output_path: None,
                candidate_results: Vec::<CandidateWriteResult>::new(),
                error_code: Some("malformed_pass_2_json".to_string()),
                duration_ms: started_at.elapsed().as_millis() as u64,
            });
        }
    };
    let catalog = EvidenceCatalog::new(&input.evidence_catalog);
    let mut candidate_results = Vec::new();

    let expected_namespace = input.scope.as_str();
    for proposal in proposals.into_iter().take(candidate_cap) {
        if let Some(reason) = validate_candidate(&proposal, &expected_namespace, &catalog) {
            candidate_results.push(refused_candidate(&proposal, &reason));
            continue;
        }

        let request = CandidateWriteRequest {
            claim: masking.restore(&proposal.claim)?,
            namespace: proposal.namespace,
            kind: proposal.kind,
            evidence: restore_evidence(masking, proposal.evidence)?,
            confidence: proposal.confidence,
            rationale: masking.restore(&proposal.rationale)?,
            policy: "dreaming-strict".to_string(),
            grounding_rehydration_required: true,
        };
        candidate_results.push(writer.write_candidate(request).await);
    }

    Ok(pass_2_outcome(started_at, candidate_results))
}

enum Pass2Parse {
    Candidates(Vec<Pass2Candidate>),
    MalformedAfterRetry,
}

const PASS2_RETRY_PREAMBLE: &str =
    "\n\nYour previous response was not valid JSON. Please return only a JSON array conforming to the schema above.";

async fn complete_and_parse_with_retry(
    cli: &dyn HarnessCli,
    prompt: &str,
    timeout: Duration,
) -> Result<Pass2Parse, DreamError> {
    for attempt in 0..=1 {
        let effective_prompt;
        let prompt_for_attempt = if attempt == 0 {
            prompt
        } else {
            effective_prompt = format!("{prompt}{PASS2_RETRY_PREAMBLE}");
            &effective_prompt
        };
        match cli.complete(prompt_for_attempt, true, timeout).await {
            Ok(output) => match parse_candidates(&output) {
                Ok(candidates) => return Ok(Pass2Parse::Candidates(candidates)),
                Err(_) if attempt == 0 => continue,
                Err(_) => return Ok(Pass2Parse::MalformedAfterRetry),
            },
            Err(HarnessCliError::MalformedJson { .. }) if attempt == 0 => continue,
            Err(HarnessCliError::MalformedJson { .. }) => return Ok(Pass2Parse::MalformedAfterRetry),
            Err(error) => return Err(DreamError::invalid_request(format!("pass 2 harness failed: {error}"))),
        }
    }
    Ok(Pass2Parse::MalformedAfterRetry)
}

fn parse_candidates(output: &str) -> Result<Vec<Pass2Candidate>, DreamError> {
    serde_json::from_str(output)
        .map_err(|error| DreamError::invalid_request(format!("pass 2 returned malformed candidate JSON: {error}")))
}

fn validate_candidate(
    proposal: &Pass2Candidate,
    expected_namespace: &str,
    catalog: &EvidenceCatalog,
) -> Option<String> {
    if proposal.namespace != expected_namespace {
        return Some("out_of_scope_namespace".to_string());
    }

    if !is_supported_candidate_kind(&proposal.kind) {
        return Some("invalid_candidate_kind".to_string());
    }

    if !proposal.confidence.is_finite() || !(0.0..=1.0).contains(&proposal.confidence) {
        return Some("invalid_confidence".to_string());
    }

    if proposal.evidence.is_empty() {
        return Some("missing_evidence_ref".to_string());
    }

    catalog.first_invalid_ref(&proposal.evidence).map(|invalid_ref| format!("hallucinated_evidence_ref:{invalid_ref}"))
}

fn is_supported_candidate_kind(kind: &str) -> bool {
    matches!(kind, "project" | "claim" | "decision" | "pattern" | "playbook" | "procedure" | "artifact")
}

fn restore_evidence(
    masking: &DreamMaskingSession,
    evidence: Vec<CandidateEvidenceRef>,
) -> Result<Vec<CandidateEvidenceRef>, DreamError> {
    evidence
        .into_iter()
        .map(|source| {
            Ok(CandidateEvidenceRef {
                kind: source.kind,
                reference: source.reference,
                excerpt: source.excerpt.map(|excerpt| masking.restore(&excerpt)).transpose()?,
            })
        })
        .collect()
}

fn refused_candidate(proposal: &Pass2Candidate, reason: &str) -> CandidateWriteResult {
    CandidateWriteResult {
        id: None,
        accepted: false,
        reason: Some(reason.to_string()),
        source_ref_count: proposal.evidence.len(),
    }
}

fn pass_2_outcome(started_at: Instant, candidate_results: Vec<CandidateWriteResult>) -> PassOutcome {
    let accepted_any = candidate_results.iter().any(|result| result.accepted);
    PassOutcome {
        status: if accepted_any { PassStatus::Success } else { PassStatus::Skipped },
        output_path: None,
        candidate_results,
        error_code: (!accepted_any).then(|| "no_candidates_accepted".to_string()),
        duration_ms: started_at.elapsed().as_millis() as u64,
    }
}

#[derive(Debug, Deserialize)]
struct Pass2Candidate {
    claim: String,
    namespace: String,
    kind: String,
    evidence: Vec<CandidateEvidenceRef>,
    confidence: f64,
    rationale: String,
}

struct EvidenceCatalog {
    refs: BTreeSet<(String, String)>,
}

impl EvidenceCatalog {
    fn new(entries: &[EvidenceCatalogEntry]) -> Self {
        Self { refs: entries.iter().map(|entry| (entry.kind.clone(), entry.reference.clone())).collect() }
    }

    fn first_invalid_ref(&self, evidence: &[CandidateEvidenceRef]) -> Option<String> {
        evidence.iter().find_map(|source| {
            (!self.refs.contains(&(source.kind.clone(), source.reference.clone())))
                .then(|| format!("{}:{}", source.kind, source.reference))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{
        dream::harness::{AuthProbeResult, HarnessFuture},
        protocol::PromptTransport,
    };

    /// A test harness that plays back a fixed sequence of outputs and records
    /// each prompt it receives, so tests can assert on what was sent per attempt.
    struct SequentialCli {
        outputs: Mutex<Vec<Result<String, HarnessCliError>>>,
        captured_prompts: Arc<Mutex<Vec<String>>>,
    }

    impl SequentialCli {
        fn new(outputs: Vec<Result<String, HarnessCliError>>, captured_prompts: Arc<Mutex<Vec<String>>>) -> Self {
            Self { outputs: Mutex::new(outputs), captured_prompts }
        }
    }

    impl HarnessCli for SequentialCli {
        fn name(&self) -> &'static str {
            "sequential-test"
        }

        fn prompt_transport(&self) -> PromptTransport {
            PromptTransport::Stdin
        }

        fn is_installed(&self) -> bool {
            true
        }

        fn auth_probe(&self) -> HarnessFuture<'_, AuthProbeResult> {
            Box::pin(async { AuthProbeResult::Ok })
        }

        fn complete<'a>(
            &'a self,
            prompt: &'a str,
            _expect_json: bool,
            _timeout: Duration,
        ) -> HarnessFuture<'a, Result<String, HarnessCliError>> {
            self.captured_prompts.lock().expect("captured_prompts lock").push(prompt.to_owned());
            let output = self.outputs.lock().expect("outputs lock").remove(0);
            Box::pin(async move { output })
        }
    }

    #[tokio::test]
    async fn retry_appends_corrective_preamble_to_second_attempt_prompt() {
        let captured_prompts = Arc::new(Mutex::new(Vec::<String>::new()));
        // First attempt returns bad JSON; second attempt returns valid JSON.
        let cli = SequentialCli::new(
            vec![Ok("not valid json at all".to_string()), Ok("[]".to_string())],
            Arc::clone(&captured_prompts),
        );

        let base_prompt = "Generate candidates now.";
        let result =
            complete_and_parse_with_retry(&cli, base_prompt, Duration::from_secs(1)).await.expect("no fatal error");

        assert!(
            matches!(result, Pass2Parse::Candidates(ref v) if v.is_empty()),
            "second attempt should yield empty candidate list"
        );

        let prompts = captured_prompts.lock().expect("prompts lock");
        assert_eq!(prompts.len(), 2, "exactly two attempts should be made");
        assert_eq!(prompts[0], base_prompt, "first attempt must use the prompt unchanged");
        assert_ne!(prompts[1], base_prompt, "second attempt prompt must differ from the first");
        assert!(
            prompts[1].ends_with(PASS2_RETRY_PREAMBLE),
            "second attempt must append the corrective preamble; got: {:?}",
            prompts[1]
        );
    }

    #[tokio::test]
    async fn malformed_on_both_attempts_returns_malformed_after_retry() {
        let captured_prompts = Arc::new(Mutex::new(Vec::<String>::new()));
        let cli = SequentialCli::new(
            vec![Ok("not valid json".to_string()), Ok("still not valid json".to_string())],
            Arc::clone(&captured_prompts),
        );

        let result =
            complete_and_parse_with_retry(&cli, "prompt", Duration::from_secs(1)).await.expect("no fatal error");

        assert!(matches!(result, Pass2Parse::MalformedAfterRetry));
        let prompts = captured_prompts.lock().expect("prompts lock");
        assert_ne!(prompts[0], prompts[1], "second attempt prompt must differ from first");
    }
}
