use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::protocol::{CandidateWriteResult, PassOutcome, PassStatus};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};

use super::{
    harness::HarnessCli,
    masking::DreamMaskingSession,
    prompts::{render_prompt, DreamPromptInput},
    run::DreamQuestionCounters,
    types::{DreamError, DreamPass},
};

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct DreamQuestionRecord {
    pub entities: Vec<String>,
    pub question: String,
}

pub struct Pass3RunContext<'a> {
    pub repo_root: &'a Path,
    pub cli: &'a dyn HarnessCli,
    pub masking: &'a DreamMaskingSession,
    pub input: &'a DreamPromptInput,
    pub timeout: Duration,
    pub counters: &'a DreamQuestionCounters,
}

pub async fn run_pass_3(context: Pass3RunContext<'_>) -> Result<PassOutcome, DreamError> {
    let Pass3RunContext { repo_root, cli, masking, input, timeout, counters } = context;
    let started_at = Instant::now();
    let prompt = render_prompt(DreamPass::Pass3, input)?;
    let output = cli
        .complete(&prompt, false, timeout)
        .await
        .map_err(|error| DreamError::invalid_request(format!("pass 3 harness failed: {error}")))?;
    let allowed_entities = allowed_entities(input);
    let classifier = DeterministicPrivacyClassifier::new();
    let records = parse_valid_records(QuestionValidation {
        output: &output,
        allowed_entities: &allowed_entities,
        masking,
        counters,
        classifier: &classifier,
    });
    let relative_path = input.scope.questions_path(input.run_date);
    let path = repo_root.join(&relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| DreamError::invalid_request(format!("failed to create questions dir: {error}")))?;
    }
    write_jsonl(&path, &records)?;

    Ok(PassOutcome {
        status: PassStatus::Success,
        output_path: Some(relative_path),
        candidate_results: Vec::<CandidateWriteResult>::new(),
        error_code: None,
        duration_ms: started_at.elapsed().as_millis() as u64,
    })
}

struct QuestionValidation<'a> {
    output: &'a str,
    allowed_entities: &'a BTreeSet<String>,
    masking: &'a DreamMaskingSession,
    counters: &'a DreamQuestionCounters,
    classifier: &'a DeterministicPrivacyClassifier,
}

fn parse_valid_records(validation: QuestionValidation<'_>) -> Vec<DreamQuestionRecord> {
    validation
        .output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let record = match serde_json::from_str::<DreamQuestionRecord>(line) {
                Ok(record) => record,
                Err(_) => {
                    validation.counters.increment_omitted(Pass3Omission::MalformedJson.counter_key());
                    return None;
                }
            };
            if let Err(omission) = validate_question_record(&record, &validation) {
                validation.counters.increment_omitted(omission.counter_key());
                return None;
            }
            if safe_plaintext_fragment(validation.classifier, &record.question) != SafeFragmentDecision::Allow {
                validation.counters.increment_omitted(Pass3Omission::UnsafeFragment.counter_key());
                return None;
            }
            Some(record)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pass3Omission {
    MalformedJson,
    EmptyQuestion,
    EmptyEntities,
    BlankEntity,
    UnknownEntity,
    OriginalPrivateValue,
    UnsafeFragment,
}

impl Pass3Omission {
    fn counter_key(self) -> &'static str {
        match self {
            Self::OriginalPrivateValue | Self::UnsafeFragment => "unsafe_fragment",
            Self::MalformedJson
            | Self::EmptyQuestion
            | Self::EmptyEntities
            | Self::BlankEntity
            | Self::UnknownEntity => "malformed_record",
        }
    }
}

fn validate_question_record(
    record: &DreamQuestionRecord,
    validation: &QuestionValidation<'_>,
) -> Result<(), Pass3Omission> {
    if record.question.trim().is_empty() {
        return Err(Pass3Omission::EmptyQuestion);
    }
    if record.entities.is_empty() {
        return Err(Pass3Omission::EmptyEntities);
    }
    for entity in &record.entities {
        if entity.trim().is_empty() {
            return Err(Pass3Omission::BlankEntity);
        }
        if !validation.allowed_entities.contains(entity) {
            return Err(Pass3Omission::UnknownEntity);
        }
    }
    if validation.masking.contains_original_private_value(&record.question) {
        return Err(Pass3Omission::OriginalPrivateValue);
    }
    Ok(())
}

fn write_jsonl(path: &Path, records: &[DreamQuestionRecord]) -> Result<(), DreamError> {
    let mut output = String::new();
    for record in records {
        let line = serde_json::to_string(record)
            .map_err(|error| DreamError::invalid_request(format!("failed to serialize question record: {error}")))?;
        output.push_str(&line);
        output.push('\n');
    }
    fs::write(path, output).map_err(|error| DreamError::invalid_request(format!("failed to write questions: {error}")))
}

fn allowed_entities(input: &DreamPromptInput) -> BTreeSet<String> {
    let mut entities = BTreeSet::new();
    for fragment in &input.substrate_fragments {
        entities.extend(fragment.entities.iter().cloned());
    }
    for memory in &input.active_memories {
        entities.extend(memory.entities.iter().cloned());
    }
    entities
}
