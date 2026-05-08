use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

use crate::protocol::{CandidateWriteResult, PassOutcome, PassStatus};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};
use memory_substrate::config::PromptVersion;

use super::{
    harness::HarnessCli,
    prompts::{render_prompt, DreamPromptInput},
    types::{DreamError, DreamPass},
};

pub struct Pass1Result {
    pub outcome: PassOutcome,
    pub markdown: Option<String>,
}

pub struct Pass1RunContext<'a> {
    pub repo_root: &'a Path,
    pub cli: &'a dyn HarnessCli,
    pub input: &'a DreamPromptInput,
    pub prompt_version: PromptVersion,
    pub timeout: Duration,
}

pub async fn run_pass_1(context: Pass1RunContext<'_>) -> Result<Pass1Result, DreamError> {
    let Pass1RunContext { repo_root, cli, input, prompt_version, timeout } = context;
    let started_at = Instant::now();
    let prompt = render_prompt(DreamPass::Pass1, input, prompt_version)?;
    let output = cli
        .complete(&prompt, false, timeout)
        .await
        .map_err(|error| DreamError::invalid_request(format!("pass 1 harness failed: {error}")))?;

    if output.trim().is_empty() {
        return Ok(Pass1Result {
            outcome: outcome(PassStatus::Failed, None, Some("empty_pass_1_output"), started_at),
            markdown: None,
        });
    }
    if safe_plaintext_fragment(&DeterministicPrivacyClassifier::new(), &output) != SafeFragmentDecision::Allow {
        return Ok(Pass1Result {
            outcome: outcome(PassStatus::Failed, None, Some("unsafe_pass_1_output"), started_at),
            markdown: None,
        });
    }

    let relative_path = input.scope.journal_path(input.run_date);
    let path = repo_root.join(&relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| DreamError::invalid_request(format!("failed to create journal dir: {error}")))?;
    }
    fs::write(&path, &output)
        .map_err(|error| DreamError::invalid_request(format!("failed to write journal: {error}")))?;

    Ok(Pass1Result {
        outcome: outcome(PassStatus::Success, Some(relative_path), None, started_at),
        markdown: Some(output),
    })
}

fn outcome(
    status: PassStatus,
    output_path: Option<String>,
    error_code: Option<&str>,
    started_at: Instant,
) -> PassOutcome {
    PassOutcome {
        status,
        output_path,
        candidate_results: Vec::<CandidateWriteResult>::new(),
        error_code: error_code.map(str::to_string),
        duration_ms: started_at.elapsed().as_millis() as u64,
    }
}
