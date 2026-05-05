use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use memory_privacy::PrivacySpan;

use crate::protocol::{CandidateWriteResult, DreamRunReport, PassOutcome, PassStatus, PromptTransport};

#[cfg(any(test, feature = "dev-fixtures"))]
use super::harness::EchoCli;
use super::{
    evidence::build_evidence_catalog,
    harness::{HarnessCli, HarnessFuture},
    masking::{DreamMaskingSession, MaskingDropObserver},
    pass1::run_pass_1,
    pass2::{run_pass_2, Pass2RunContext},
    pass3::{run_pass_3, Pass3RunContext},
    prompts::{render_prompt, DreamPromptInput},
    scope::DreamScope,
    types::{ActiveMemory, DreamError, DreamPass, HarnessSelection, SubstrateFragment},
};

#[derive(Clone)]
pub struct DreamRunOptions {
    pub repo_root: PathBuf,
    pub scope: DreamScope,
    pub run_date: chrono::NaiveDate,
    pub run_id: String,
    /// Populated by `select_harness` (or `with_harness`) before `DreamRunner::run`.
    /// `build_dream_run` constructs this with a placeholder; live runs always replace
    /// it before execution. See `orchestration::UnselectedHarness`.
    pub harness: Arc<dyn HarnessCli>,
    pub pass_timeout: Duration,
    pub pass_2_max_candidates: usize,
    pub substrate_fragments: Vec<DreamSubstrateFragmentInput>,
    pub active_memories: Vec<DreamActiveMemoryInput>,
    pub previous_questions: Vec<String>,
}

impl DreamRunOptions {
    pub fn with_harness(mut self, harness: Arc<dyn HarnessCli>) -> Self {
        self.harness = harness;
        self
    }
}

#[cfg(any(test, feature = "dev-fixtures"))]
pub fn deterministic_echo_harness(options: &DreamRunOptions) -> Result<Arc<dyn HarnessCli>, DreamError> {
    let pass_1_output = "# Dream Journal\nNo substrate fragments available.";
    let pass_1_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_1_prompt(options)?;
    let pass_2_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_2_prompt(options, pass_1_output)?;
    let pass_3_prompt = DreamRunner::<NoopCandidateWriter>::preview_pass_3_prompt(options, pass_1_output)?;
    let pass_2_output = deterministic_echo_pass_2_output(options)?;

    Ok(Arc::new(EchoCli::from_prompt_outputs([
        (pass_1_prompt.as_str(), pass_1_output),
        (pass_2_prompt.as_str(), pass_2_output.as_str()),
        (pass_3_prompt.as_str(), ""),
    ])))
}

#[cfg(any(test, feature = "dev-fixtures"))]
fn deterministic_echo_pass_2_output(options: &DreamRunOptions) -> Result<String, DreamError> {
    if let Some(input) = options.substrate_fragments.first() {
        let text = deterministic_masked_text(options, &input.fragment.text, &input.text_spans)?;
        return Ok(serde_json::json!([{
            "claim": format!("Echo dream observed {}.", text),
            "namespace": options.scope.as_str(),
            "kind": "decision",
            "evidence": [{
                "kind": "substrate_fragment",
                "ref": input.fragment.id,
                "excerpt": text
            }],
            "confidence": 0.8,
            "rationale": "Deterministic echo harness generated a candidate from substrate evidence."
        }])
        .to_string());
    }
    if let Some(input) = options.active_memories.first() {
        let summary = deterministic_masked_text(options, &input.memory.summary, &input.summary_spans)?;
        return Ok(serde_json::json!([{
            "claim": format!("Echo dream recalled {}.", summary),
            "namespace": options.scope.as_str(),
            "kind": "decision",
            "evidence": [{
                "kind": "memory",
                "ref": input.memory.id,
                "excerpt": summary
            }],
            "confidence": 0.8,
            "rationale": "Deterministic echo harness generated a candidate from active memory evidence."
        }])
        .to_string());
    }
    Ok("[]".to_string())
}

#[cfg(any(test, feature = "dev-fixtures"))]
fn deterministic_masked_text(
    options: &DreamRunOptions,
    text: &str,
    spans: &[PrivacySpan],
) -> Result<String, DreamError> {
    let mut masking = DreamMaskingSession::new(&options.scope.as_str(), &options.run_id);
    masking.mask(text, spans)
}

#[derive(Debug, Clone)]
pub struct DreamSubstrateFragmentInput {
    pub fragment: SubstrateFragment,
    pub text_spans: Vec<PrivacySpan>,
}

#[derive(Debug, Clone)]
pub struct DreamActiveMemoryInput {
    pub memory: ActiveMemory,
    pub summary_spans: Vec<PrivacySpan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateWriteRequest {
    pub claim: String,
    pub namespace: String,
    pub kind: String,
    pub evidence: Vec<CandidateEvidenceRef>,
    pub confidence: f64,
    pub rationale: String,
    pub policy: String,
    pub grounding_rehydration_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct CandidateEvidenceRef {
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

pub trait CandidateWriter: Send + Sync {
    fn write_candidate<'a>(&'a self, request: CandidateWriteRequest) -> HarnessFuture<'a, CandidateWriteResult>;
}

#[derive(Debug, Clone, Copy)]
pub struct NoopCandidateWriter;

impl CandidateWriter for NoopCandidateWriter {
    fn write_candidate<'a>(&'a self, _request: CandidateWriteRequest) -> HarnessFuture<'a, CandidateWriteResult> {
        Box::pin(async {
            CandidateWriteResult {
                id: None,
                accepted: false,
                reason: Some("noop_candidate_writer".to_string()),
                source_ref_count: 0,
            }
        })
    }
}

pub struct DreamRunner<W> {
    options: DreamRunOptions,
    candidate_writer: W,
    drop_observer: Option<MaskingDropObserver>,
    question_counters: DreamQuestionCounters,
}

#[derive(Clone, Debug, Default)]
pub struct DreamQuestionCounters {
    omitted: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl DreamQuestionCounters {
    pub fn increment_omitted(&self, reason: &str) {
        let mut omitted = self.omitted.lock().expect("dream question counter lock poisoned");
        *omitted.entry(reason.to_string()).or_insert(0) += 1;
    }

    pub fn omitted(&self, reason: &str) -> u64 {
        self.omitted.lock().expect("dream question counter lock poisoned").get(reason).copied().unwrap_or(0)
    }
}

impl<W> DreamRunner<W>
where
    W: CandidateWriter,
{
    pub fn new(options: DreamRunOptions, candidate_writer: W) -> Self {
        Self { options, candidate_writer, drop_observer: None, question_counters: DreamQuestionCounters::default() }
    }

    pub fn with_masking_drop_observer(mut self, observer: MaskingDropObserver) -> Self {
        self.drop_observer = Some(observer);
        self
    }

    pub fn with_question_counters(mut self, counters: DreamQuestionCounters) -> Self {
        self.question_counters = counters;
        self
    }

    pub fn preview_pass_1_prompt(options: &DreamRunOptions) -> Result<String, DreamError> {
        let mut masking = DreamMaskingSession::new(&options.scope.as_str(), &options.run_id);
        let input = build_masked_prompt_input(options, &mut masking, None)?;
        render_prompt(DreamPass::Pass1, &input)
    }

    pub fn preview_pass_2_prompt(options: &DreamRunOptions, pass_1_markdown: &str) -> Result<String, DreamError> {
        let mut masking = DreamMaskingSession::new(&options.scope.as_str(), &options.run_id);
        let _ = build_masked_prompt_input(options, &mut masking, None)?;
        let input = build_masked_prompt_input(options, &mut masking, Some(pass_1_markdown.to_string()))?;
        render_prompt(DreamPass::Pass2, &input)
    }

    pub fn preview_pass_3_prompt(options: &DreamRunOptions, pass_1_markdown: &str) -> Result<String, DreamError> {
        let mut masking = DreamMaskingSession::new(&options.scope.as_str(), &options.run_id);
        let _ = build_masked_prompt_input(options, &mut masking, None)?;
        let input = build_masked_prompt_input(options, &mut masking, Some(pass_1_markdown.to_string()))?;
        render_prompt(DreamPass::Pass3, &input)
    }

    pub async fn run(self) -> Result<DreamRunReport, DreamError> {
        let started_at = Instant::now();
        let mut masking = DreamMaskingSession::with_drop_observer(
            &self.options.scope.as_str(),
            &self.options.run_id,
            self.drop_observer.clone(),
        );
        let pass_1_input = build_masked_prompt_input(&self.options, &mut masking, None)?;
        let pass_1 = run_pass_1(
            &self.options.repo_root,
            self.options.harness.as_ref(),
            &pass_1_input,
            self.options.pass_timeout,
        )
        .await?;

        let pass_2 = match &pass_1.markdown {
            Some(markdown) => {
                let pass_2_input = build_masked_prompt_input(&self.options, &mut masking, Some(markdown.clone()))?;
                match run_pass_2(Pass2RunContext {
                    cli: self.options.harness.as_ref(),
                    writer: &self.candidate_writer,
                    masking: &masking,
                    input: &pass_2_input,
                    timeout: self.options.pass_timeout,
                    candidate_cap: self.options.pass_2_max_candidates,
                })
                .await
                {
                    Ok(outcome) => outcome,
                    Err(error) => failed_pass("pass_2_failed", &error),
                }
            }
            None => skipped_pass(),
        };

        let pass_3 = match &pass_1.markdown {
            Some(markdown) => {
                let pass_3_input = build_masked_prompt_input(&self.options, &mut masking, Some(markdown.clone()))?;
                match run_pass_3(Pass3RunContext {
                    repo_root: &self.options.repo_root,
                    cli: self.options.harness.as_ref(),
                    masking: &masking,
                    input: &pass_3_input,
                    timeout: self.options.pass_timeout,
                    counters: &self.question_counters,
                })
                .await
                {
                    Ok(outcome) => outcome,
                    Err(error) => failed_pass("pass_3_failed", &error),
                }
            }
            None => skipped_pass(),
        };

        let report = DreamRunReport {
            scope: self.options.scope.as_str(),
            cli_used: Some(self.options.harness.name().to_string()),
            pass_1: pass_1.outcome,
            pass_2_refusal_counts_by_reason: refusal_counts_by_reason(&pass_2),
            pass_2,
            pass_3,
            duration_ms: started_at.elapsed().as_millis() as u64,
        };
        drop(masking);
        Ok(report)
    }
}

fn build_masked_prompt_input(
    options: &DreamRunOptions,
    masking: &mut DreamMaskingSession,
    pass_1_markdown: Option<String>,
) -> Result<DreamPromptInput, DreamError> {
    let substrate_fragments = options
        .substrate_fragments
        .iter()
        .map(|input| {
            let mut fragment = input.fragment.clone();
            fragment.text = masking.mask(&fragment.text, &input.text_spans)?;
            Ok(fragment)
        })
        .collect::<Result<Vec<_>, DreamError>>()?;

    let active_memories = options
        .active_memories
        .iter()
        .map(|input| {
            let mut memory = input.memory.clone();
            memory.summary = masking.mask(&memory.summary, &input.summary_spans)?;
            Ok(memory)
        })
        .collect::<Result<Vec<_>, DreamError>>()?;

    let evidence_catalog = build_evidence_catalog(&substrate_fragments, &active_memories);

    Ok(DreamPromptInput {
        scope: options.scope.clone(),
        run_date: options.run_date,
        harness: HarnessSelection {
            name: options.harness.name().to_string(),
            prompt_transport: prompt_transport_name(options.harness.prompt_transport()).to_string(),
        },
        masking: masking.context(),
        substrate_fragments,
        active_memories,
        pass_1_markdown,
        previous_questions: options.previous_questions.clone(),
        evidence_catalog,
    })
}

fn prompt_transport_name(transport: PromptTransport) -> &'static str {
    match transport {
        PromptTransport::Stdin => "stdin",
        PromptTransport::Argv => "argv",
    }
}

fn refusal_counts_by_reason(pass: &PassOutcome) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for result in pass.candidate_results.iter().filter(|result| !result.accepted) {
        let reason = result.reason.as_deref().unwrap_or("unspecified");
        *counts.entry(reason.to_owned()).or_insert(0) += 1;
    }
    counts
}

fn skipped_pass() -> PassOutcome {
    PassOutcome {
        status: PassStatus::Skipped,
        output_path: None,
        candidate_results: Vec::<CandidateWriteResult>::new(),
        error_code: None,
        duration_ms: 0,
    }
}

fn failed_pass(code: &str, error: &DreamError) -> PassOutcome {
    PassOutcome {
        status: PassStatus::Failed,
        output_path: None,
        candidate_results: Vec::<CandidateWriteResult>::new(),
        error_code: Some(format!("{code}:{}", error.code())),
        duration_ms: 0,
    }
}
