use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use serde::Deserialize;

use crate::protocol::{CandidateWriteResult, PassOutcome, PassStatus};

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
    pub timeout: Duration,
    pub candidate_cap: usize,
}

pub async fn run_pass_2<W>(context: Pass2RunContext<'_, W>) -> Result<PassOutcome, DreamError>
where
    W: CandidateWriter,
{
    let Pass2RunContext { cli, writer, masking, input, timeout, candidate_cap } = context;
    let started_at = Instant::now();
    let prompt = render_prompt(DreamPass::Pass2, input)?;
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

async fn complete_and_parse_with_retry(
    cli: &dyn HarnessCli,
    prompt: &str,
    timeout: Duration,
) -> Result<Pass2Parse, DreamError> {
    for attempt in 0..=1 {
        match cli.complete(prompt, true, timeout).await {
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
