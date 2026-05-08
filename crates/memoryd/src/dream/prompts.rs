use std::collections::BTreeSet;

use chrono::NaiveDate;
use serde::Serialize;

use super::scope::DreamScope;
use super::types::{
    ActiveMemory, DreamError, DreamPass, EvidenceCatalogEntry, HarnessSelection, MaskingContext, SubstrateFragment,
};

pub use memory_substrate::config::PromptVersion;

const PASS_1_TEMPLATE: &str = include_str!("../../../../prompts/dream-pass-1-v1.md");
const PASS_2_TEMPLATE: &str = include_str!("../../../../prompts/dream-pass-2-v1.md");
const PASS_3_TEMPLATE: &str = include_str!("../../../../prompts/dream-pass-3-v1.md");
const PASS_1_TEMPLATE_V2: &str = include_str!("../../../../prompts/dream-pass-1-v2.md");
const PASS_2_TEMPLATE_V2: &str = include_str!("../../../../prompts/dream-pass-2-v2.md");
const PASS_3_TEMPLATE_V2: &str = include_str!("../../../../prompts/dream-pass-3-v2.md");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamPromptInput {
    pub scope: DreamScope,
    pub run_date: NaiveDate,
    pub harness: HarnessSelection,
    pub masking: MaskingContext,
    pub substrate_fragments: Vec<SubstrateFragment>,
    pub active_memories: Vec<ActiveMemory>,
    pub pass_1_markdown: Option<String>,
    pub previous_questions: Vec<String>,
    pub evidence_catalog: Vec<EvidenceCatalogEntry>,
}

pub fn render_prompt(pass: DreamPass, input: &DreamPromptInput, version: PromptVersion) -> Result<String, DreamError> {
    match (pass, version) {
        (DreamPass::Pass1, PromptVersion::V1) => render_from_template(PASS_1_TEMPLATE, &Pass1Payload::from(input)),
        (DreamPass::Pass2, PromptVersion::V1) => render_from_template(PASS_2_TEMPLATE, &Pass2Payload::from(input)),
        (DreamPass::Pass3, PromptVersion::V1) => render_from_template(PASS_3_TEMPLATE, &Pass3Payload::from(input)),
        (DreamPass::Pass1, PromptVersion::V2) => render_from_template(PASS_1_TEMPLATE_V2, &Pass1Payload::from(input)),
        (DreamPass::Pass2, PromptVersion::V2) => render_from_template(PASS_2_TEMPLATE_V2, &Pass2Payload::from(input)),
        (DreamPass::Pass3, PromptVersion::V2) => render_from_template(PASS_3_TEMPLATE_V2, &Pass3Payload::from(input)),
    }
}

fn render_from_template<T: Serialize>(template: &str, payload: &T) -> Result<String, DreamError> {
    let input_json = serde_json::to_string_pretty(payload)
        .map_err(|error| DreamError::invalid_request(format!("failed to render dream prompt input: {error}")))?;

    Ok(template.replace("{{input_json}}", &input_json))
}

#[derive(Serialize)]
struct Pass1Payload<'a> {
    scope: String,
    scope_path: String,
    run_date: String,
    harness: &'a HarnessSelection,
    masking: &'a MaskingContext,
    substrate_fragments: &'a [SubstrateFragment],
    active_memories: &'a [ActiveMemory],
    allowed_entities: Vec<String>,
}

#[derive(Serialize)]
struct Pass2Payload<'a> {
    scope: String,
    scope_path: String,
    run_date: String,
    harness: &'a HarnessSelection,
    masking: &'a MaskingContext,
    pass_1_markdown: &'a str,
    active_memories: &'a [ActiveMemory],
    evidence_catalog: &'a [EvidenceCatalogEntry],
    candidate_schema: CandidateSchema,
}

#[derive(Serialize)]
struct Pass3Payload<'a> {
    scope: String,
    scope_path: String,
    run_date: String,
    harness: &'a HarnessSelection,
    masking: &'a MaskingContext,
    pass_1_markdown: &'a str,
    active_memories: &'a [ActiveMemory],
    previous_questions: &'a [String],
    allowed_entities: Vec<String>,
}

#[derive(Serialize)]
struct CandidateSchema {
    output: &'static str,
    claim: &'static str,
    namespace: &'static str,
    kind: &'static str,
    evidence: &'static str,
    confidence: &'static str,
    rationale: &'static str,
}

impl CandidateSchema {
    fn dream_pass_2() -> Self {
        Self {
            output: "JSON array of candidate-proposal objects only",
            claim: "masked claim text",
            namespace: "must match the in-scope namespace",
            kind: "canonical memory kind such as decision or fact",
            evidence: "non-empty array; every kind/ref tuple must appear in evidence_catalog",
            confidence: "finite number in [0, 1]",
            rationale: "short masked rationale",
        }
    }
}

impl<'a> From<&'a DreamPromptInput> for Pass1Payload<'a> {
    fn from(input: &'a DreamPromptInput) -> Self {
        Self {
            scope: input.scope.as_str(),
            scope_path: input.scope.storage_path_for_date(input.run_date),
            run_date: input.run_date.to_string(),
            harness: &input.harness,
            masking: &input.masking,
            substrate_fragments: &input.substrate_fragments,
            active_memories: &input.active_memories,
            allowed_entities: allowed_entities(input),
        }
    }
}

impl<'a> From<&'a DreamPromptInput> for Pass2Payload<'a> {
    fn from(input: &'a DreamPromptInput) -> Self {
        Self {
            scope: input.scope.as_str(),
            scope_path: input.scope.storage_path_for_date(input.run_date),
            run_date: input.run_date.to_string(),
            harness: &input.harness,
            masking: &input.masking,
            pass_1_markdown: input.pass_1_markdown.as_deref().unwrap_or(""),
            active_memories: &input.active_memories,
            evidence_catalog: &input.evidence_catalog,
            candidate_schema: CandidateSchema::dream_pass_2(),
        }
    }
}

impl<'a> From<&'a DreamPromptInput> for Pass3Payload<'a> {
    fn from(input: &'a DreamPromptInput) -> Self {
        Self {
            scope: input.scope.as_str(),
            scope_path: input.scope.storage_path_for_date(input.run_date),
            run_date: input.run_date.to_string(),
            harness: &input.harness,
            masking: &input.masking,
            pass_1_markdown: input.pass_1_markdown.as_deref().unwrap_or(""),
            active_memories: &input.active_memories,
            previous_questions: &input.previous_questions,
            allowed_entities: allowed_entities(input),
        }
    }
}

fn allowed_entities(input: &DreamPromptInput) -> Vec<String> {
    let mut entities = BTreeSet::new();

    for fragment in &input.substrate_fragments {
        entities.extend(fragment.entities.iter().cloned());
    }

    for memory in &input.active_memories {
        entities.extend(memory.entities.iter().cloned());
    }

    entities.into_iter().collect()
}
