//! LLM-as-judge for the real-harness end-to-end eval tests (Tests #13, #15).
//!
//! After a real-harness test's structural assertions pass, the test can ask a
//! second harness-CLI invocation to score, on a 3-point rubric, whether the
//! agent's recall/usage of the memory actually served the task. The score is
//! **recorded, not gating**: it is printed as a `MEMORUM_EVAL_JUDGE=` marker
//! line so it lands in eval output (the same `--nocapture` stdout channel the
//! orchestrator already scans for `MEMORUM_EVAL_ASSERTIONS=`), and it never
//! fails the calling test. The distribution is collected during dogfood before
//! a gate is ever considered.
//!
//! JSON parsing mirrors dream Pass 2's one-retry + corrective-preamble pattern
//! (`memoryd::dream::pass2::PASS2_RETRY_PREAMBLE`). The pattern is reproduced
//! here in eval's own code rather than importing memoryd's dream internals.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::harness_runner::{HarnessRunResult, HarnessRunner};

/// Marker line the orchestrator (and any log scraper) can pick out of cargo
/// test stdout to record a judge verdict. Format, one per line:
/// `MEMORUM_EVAL_JUDGE=<test>:<harness>:<score>:<rationale>`.
pub const EVAL_JUDGE_MARKER: &str = "MEMORUM_EVAL_JUDGE=";

/// Corrective preamble appended to the judge prompt on its single retry when the
/// first response did not parse as the rubric JSON. Mirrors dream Pass 2's
/// `PASS2_RETRY_PREAMBLE` (see `crates/memoryd/src/dream/pass2.rs`).
const JUDGE_RETRY_PREAMBLE: &str =
    "\n\nYour previous response was not valid JSON. Return only a single JSON object of the form \
     {\"score\": <1|2|3>, \"rationale\": \"<one sentence>\"} with no prose, Markdown, or code fences.";

/// A 3-point rubric verdict on whether the agent's recall/usage served the task.
///
/// - `1` — the memory was not surfaced or was ignored; the task was not served.
/// - `2` — the memory was surfaced but only partially or awkwardly used.
/// - `3` — the memory was recalled and used such that it directly served the task.
#[derive(Debug, Clone, Deserialize)]
pub struct JudgeVerdict {
    pub score: u8,
    #[serde(default)]
    pub rationale: String,
}

impl JudgeVerdict {
    fn is_in_rubric(&self) -> bool {
        (1..=3).contains(&self.score)
    }
}

/// Inputs for one judge invocation.
pub struct JudgeRequest<'a> {
    /// Stable test identifier for the recorded marker, e.g. `"t13"`.
    pub test: &'a str,
    /// Harness label for the recorded marker, e.g. `"claude"` or `"codex"`.
    pub harness_label: &'a str,
    /// One-line description of what the agent was supposed to accomplish.
    pub task_summary: &'a str,
    /// The agent's task-phase output (the JSON object the structural assertions
    /// already validated), serialized for the judge to inspect.
    pub agent_output: &'a Value,
    /// Per-invocation environment (auth keys, MCP config path, cwd, …). The judge
    /// reuses the task phase's env so it runs under the same harness/auth.
    pub env: &'a HashMap<String, String>,
    /// Per-invocation timeout.
    pub timeout: Duration,
}

/// Run the judge as a recorded, non-gating step.
///
/// Returns the parsed verdict when the harness produced an in-rubric score
/// within one retry, or `None` when the judge could not be scored (harness
/// failure or unparseable output after the retry). In **all** cases a
/// `MEMORUM_EVAL_JUDGE=` marker line is printed so the outcome — score or
/// `skip`/`error` — is visible in eval output. This function never panics and
/// never asserts; the caller's structural pass/fail is untouched.
pub async fn judge_recall_served_task(runner: &HarnessRunner, request: JudgeRequest<'_>) -> Option<JudgeVerdict> {
    let prompt = build_judge_prompt(request.task_summary, request.agent_output);

    match complete_and_parse_with_retry(runner, &prompt, request.env, request.timeout).await {
        JudgeParse::Verdict(verdict) => {
            record_marker(request.test, request.harness_label, &format!("{}", verdict.score), &verdict.rationale);
            Some(verdict)
        }
        JudgeParse::MalformedAfterRetry => {
            record_marker(
                request.test,
                request.harness_label,
                "error",
                "judge output was not in-rubric JSON after one retry",
            );
            None
        }
        JudgeParse::HarnessFailed(detail) => {
            record_marker(
                request.test,
                request.harness_label,
                "error",
                &format!("judge harness invocation failed: {detail}"),
            );
            None
        }
    }
}

/// Build the judge prompt: a fixed rubric plus the task context and the agent's
/// own output, instructing the judge to return only the rubric JSON object.
fn build_judge_prompt(task_summary: &str, agent_output: &Value) -> String {
    let agent_output_pretty = serde_json::to_string_pretty(agent_output).unwrap_or_else(|_| agent_output.to_string());
    format!(
        "You are scoring a Memorum eval transcript. An agent was asked to recall a memory and use it to \
         complete a task. Judge whether the agent's recall and usage of the memory actually served the task.\n\n\
         TASK THE AGENT WAS GIVEN:\n{task_summary}\n\n\
         THE AGENT'S OUTPUT:\n{agent_output_pretty}\n\n\
         Score on this 3-point rubric:\n\
         - 1: the memory was not surfaced or was ignored; the task was not served.\n\
         - 2: the memory was surfaced but only partially or awkwardly used.\n\
         - 3: the memory was recalled and used such that it directly served the task.\n\n\
         Output exactly one JSON object on stdout and nothing else:\n\
         {{\"score\": <1|2|3>, \"rationale\": \"<one sentence>\"}}\n\
         No prose, Markdown, or code fences."
    )
}

enum JudgeParse {
    Verdict(JudgeVerdict),
    MalformedAfterRetry,
    HarnessFailed(String),
}

/// Invoke the harness with at most two attempts: the second attempt appends the
/// corrective preamble when the first response did not parse as in-rubric JSON.
/// Mirrors dream Pass 2's `complete_and_parse_with_retry`.
async fn complete_and_parse_with_retry(
    runner: &HarnessRunner,
    prompt: &str,
    env: &HashMap<String, String>,
    timeout: Duration,
) -> JudgeParse {
    for attempt in 0..=1 {
        let effective_prompt;
        let prompt_for_attempt = if attempt == 0 {
            prompt
        } else {
            effective_prompt = format!("{prompt}{JUDGE_RETRY_PREAMBLE}");
            &effective_prompt
        };

        let result = runner.run(prompt_for_attempt, env, timeout).await;
        if result.exit_code != 0 {
            if attempt == 0 {
                continue;
            }
            return JudgeParse::HarnessFailed(harness_failure_detail(&result));
        }

        match parse_verdict(&result.stdout) {
            Some(verdict) => return JudgeParse::Verdict(verdict),
            None if attempt == 0 => continue,
            None => return JudgeParse::MalformedAfterRetry,
        }
    }
    JudgeParse::MalformedAfterRetry
}

fn harness_failure_detail(result: &HarnessRunResult) -> String {
    format!("exit_code={} stderr={}", result.exit_code, result.stderr.trim())
}

/// Parse the rubric verdict from harness stdout, accepting either a whole-stdout
/// JSON object or the last non-empty JSON line — the same tolerance the tests
/// already apply to agent output. Only in-rubric (1..=3) scores are accepted.
fn parse_verdict(stdout: &str) -> Option<JudgeVerdict> {
    let parse_one =
        |candidate: &str| serde_json::from_str::<JudgeVerdict>(candidate).ok().filter(JudgeVerdict::is_in_rubric);

    parse_one(stdout.trim())
        .or_else(|| stdout.lines().rev().map(str::trim).find(|line| !line.is_empty()).and_then(parse_one))
}

/// Print the recorded judge marker. Rationale is flattened to a single line so
/// the marker stays one line (the orchestrator scans line-by-line).
fn record_marker(test: &str, harness_label: &str, score: &str, rationale: &str) {
    let rationale = rationale.replace(['\n', '\r'], " ");
    println!("{EVAL_JUDGE_MARKER}{test}:{harness_label}:{score}:{rationale}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_whole_stdout_object() {
        let verdict = parse_verdict(r#"{"score": 3, "rationale": "used the fact directly"}"#).expect("in-rubric");
        assert_eq!(verdict.score, 3);
        assert_eq!(verdict.rationale, "used the fact directly");
    }

    #[test]
    fn parses_last_json_line_amid_prose() {
        let stdout = "thinking out loud\nmore noise\n{\"score\": 2, \"rationale\": \"partial\"}\n";
        let verdict = parse_verdict(stdout).expect("trailing json line");
        assert_eq!(verdict.score, 2);
    }

    #[test]
    fn rejects_out_of_rubric_score() {
        assert!(parse_verdict(r#"{"score": 0, "rationale": "x"}"#).is_none());
        assert!(parse_verdict(r#"{"score": 4, "rationale": "x"}"#).is_none());
    }

    #[test]
    fn rejects_non_json() {
        assert!(parse_verdict("the agent did great").is_none());
    }

    #[test]
    fn rationale_defaults_when_absent() {
        let verdict = parse_verdict(r#"{"score": 1}"#).expect("score-only object is valid");
        assert_eq!(verdict.score, 1);
        assert_eq!(verdict.rationale, "");
    }

    #[test]
    fn retry_preamble_matches_house_pattern_shape() {
        // The preamble is a corrective JSON-only instruction, mirroring dream
        // Pass 2's PASS2_RETRY_PREAMBLE: leads with a blank line and asks for
        // valid JSON only.
        assert!(JUDGE_RETRY_PREAMBLE.starts_with("\n\n"));
        assert!(JUDGE_RETRY_PREAMBLE.contains("not valid JSON"));
        assert!(JUDGE_RETRY_PREAMBLE.contains("score"));
    }

    #[test]
    fn build_judge_prompt_embeds_task_and_output_and_rubric() {
        let output = json!({"found": true, "fragment_text": "Go 1.22 hard constraint"});
        let prompt = build_judge_prompt("Recall the Go 1.22 fact", &output);
        assert!(prompt.contains("Recall the Go 1.22 fact"));
        assert!(prompt.contains("Go 1.22 hard constraint"));
        assert!(prompt.contains("3-point rubric"));
        assert!(prompt.contains("\"score\""));
    }

    #[test]
    fn marker_const_ends_with_equals() {
        assert!(EVAL_JUDGE_MARKER.ends_with('='));
    }
}
