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
use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::harness_runner::{HarnessRunResult, HarnessRunner};

/// Marker line the orchestrator (and any log scraper) can pick out of cargo
/// test stdout to record a judge verdict. Format, one per line:
/// `MEMORUM_EVAL_JUDGE=<test>:<harness>:<score>:<rationale>`.
pub const EVAL_JUDGE_MARKER: &str = "MEMORUM_EVAL_JUDGE=";

/// Stable benchmark judge input dumped by the benchmark runner and accepted by
/// external judge commands on stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkJudgeInput {
    pub question: String,
    pub gold: String,
    pub retrieved_context: Vec<String>,
    pub answer_basis: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkJudgeVerdict {
    pub score: f64,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JudgeError {
    Timeout,
    Spawn(String),
    Io(String),
    Serialize(String),
    Parse(String),
    NonFinite { score: f64 },
    OutOfRange { score: f64, min: f64, max: f64 },
    External { status: String, stderr: String },
}

impl std::fmt::Display for JudgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "judge_timeout"),
            Self::Spawn(error) => write!(f, "judge_spawn_error: {error}"),
            Self::Io(error) => write!(f, "judge_io_error: {error}"),
            Self::Serialize(error) => write!(f, "judge_serialize_error: {error}"),
            Self::Parse(error) => write!(f, "judge_parse_error: {error}"),
            Self::NonFinite { score } => write!(f, "judge_non_finite_score: {score}"),
            Self::OutOfRange { score, min, max } => write!(f, "judge_score_out_of_range: {score} not in {min}..={max}"),
            Self::External { status, stderr } => write!(f, "judge_external_error: exited {status}: {stderr}"),
        }
    }
}

impl std::error::Error for JudgeError {}

pub trait BenchmarkJudge {
    fn judge(&self, input: &BenchmarkJudgeInput) -> Result<BenchmarkJudgeVerdict, JudgeError>;

    fn identity(&self) -> String {
        "unknown".to_owned()
    }
}

/// Judge adapter for a coordinator-pinned command. The executable and argv are
/// explicit: no shell expansion or hidden model choice occurs in the harness.
pub struct ExternalCommandJudge {
    program: OsString,
    args: Vec<OsString>,
    timeout: Duration,
    min_score: f64,
    max_score: f64,
}

impl ExternalCommandJudge {
    pub fn new(program: impl Into<OsString>, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: Duration::from_secs(60),
            min_score: 0.0,
            max_score: 1.0,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_score_range(mut self, min: f64, max: f64) -> Self {
        self.min_score = min;
        self.max_score = max;
        self
    }
}

impl BenchmarkJudge for ExternalCommandJudge {
    fn judge(&self, input: &BenchmarkJudgeInput) -> Result<BenchmarkJudgeVerdict, JudgeError> {
        let input_json = serde_json::to_vec(input).map_err(|error| JudgeError::Serialize(error.to_string()))?;

        let start = std::time::Instant::now();
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| JudgeError::Spawn(error.to_string()))?;

        let stdin = child.stdin.take().expect("piped judge stdin");
        let stdout = child.stdout.take().expect("piped judge stdout");
        let stderr = child.stderr.take().expect("piped judge stderr");

        let stdin_handle = std::thread::spawn(move || {
            let mut stdin = stdin;
            let _ = stdin.write_all(&input_json);
            let _ = stdin.flush();
        });

        let stdout_handle = std::thread::spawn(move || {
            let mut stdout = stdout;
            let mut buffer = Vec::new();
            let _ = stdout.read_to_end(&mut buffer);
            buffer
        });

        let stderr_handle = std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut buffer = Vec::new();
            let _ = stderr.read_to_end(&mut buffer);
            String::from_utf8_lossy(&buffer).trim().to_owned()
        });

        let status = loop {
            if start.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdin_handle.join();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(JudgeError::Timeout);
            }
            match child.try_wait().map_err(|error| JudgeError::Io(error.to_string()))? {
                Some(status) => break status,
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        };

        let _ = child.wait();
        let _ = stdin_handle.join();
        let stdout = stdout_handle.join().map_err(|_| JudgeError::Io("stdout reader thread panicked".to_owned()))?;
        let stderr = stderr_handle.join().map_err(|_| JudgeError::Io("stderr reader thread panicked".to_owned()))?;

        if !status.success() {
            return Err(JudgeError::External {
                status: status.to_string(),
                stderr,
            });
        }

        let mut verdict: BenchmarkJudgeVerdict =
            serde_json::from_slice(&stdout).map_err(|error| match error.to_string().as_str() {
                msg if msg.starts_with("number out of range") => JudgeError::NonFinite { score: f64::INFINITY },
                _ => JudgeError::Parse(error.to_string()),
            })?;

        if !verdict.score.is_finite() {
            return Err(JudgeError::NonFinite { score: verdict.score });
        }
        if !(self.min_score..=self.max_score).contains(&verdict.score) {
            return Err(JudgeError::OutOfRange {
                score: verdict.score,
                min: self.min_score,
                max: self.max_score,
            });
        }

        verdict.rationale = verdict.rationale.trim().to_owned();
        Ok(verdict)
    }

    fn identity(&self) -> String {
        let args = self.args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
        format!("{} {}", self.program.to_string_lossy(), args)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeterministicMockJudge;

impl BenchmarkJudge for DeterministicMockJudge {
    fn judge(&self, input: &BenchmarkJudgeInput) -> Result<BenchmarkJudgeVerdict, JudgeError> {
        let gold = normalize_answer(&input.gold);
        let contains = !gold.is_empty() && normalize_answer(&input.answer_basis).contains(&gold);
        Ok(BenchmarkJudgeVerdict {
            score: if contains { 1.0 } else { 0.0 },
            rationale: if contains {
                "gold answer appears in answer basis"
            } else {
                "gold answer absent from answer basis"
            }
            .to_owned(),
        })
    }

    fn identity(&self) -> String {
        "deterministic_mock".to_owned()
    }
}

fn normalize_answer(value: &str) -> String {
    value.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

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

    #[test]
    fn external_command_judge_uses_json_stdin_and_stdout() {
        let judge = ExternalCommandJudge::new(
            "/bin/sh",
            ["-c", "grep -q '\"question\"' && printf '{\"score\":0.75,\"rationale\":\"ok\"}'"],
        );
        let verdict = judge
            .judge(&BenchmarkJudgeInput {
                question: "q".to_owned(),
                gold: "a".to_owned(),
                retrieved_context: vec![],
                answer_basis: String::new(),
            })
            .expect("external judge verdict");
        assert_eq!(verdict.score, 0.75);
    }

    #[test]
    fn external_command_judge_times_out_after_configured_duration() {
        let judge = ExternalCommandJudge::new("/bin/sh", ["-c", "sleep 2"]).with_timeout(Duration::from_millis(100));
        let error = judge
            .judge(&BenchmarkJudgeInput {
                question: "q".to_owned(),
                gold: "a".to_owned(),
                retrieved_context: vec![],
                answer_basis: String::new(),
            })
            .expect_err("judge should time out");
        assert!(matches!(error, JudgeError::Timeout), "expected timeout, got {error:?}");
    }

    #[test]
    fn external_command_judge_times_out_without_reading_stdin() {
        // The deadline must cover spawn + stdin write + stdout read, even when the
        // child never drains its input pipe.
        let judge = ExternalCommandJudge::new("/bin/sh", ["-c", "sleep 5"]).with_timeout(Duration::from_millis(100));
        let error = judge
            .judge(&BenchmarkJudgeInput {
                question: "q".to_owned(),
                gold: "a".to_owned(),
                retrieved_context: vec!["context".to_owned()],
                answer_basis: String::new(),
            })
            .expect_err("judge should time out");
        assert!(matches!(error, JudgeError::Timeout), "expected timeout, got {error:?}");
    }

    #[test]
    fn external_command_judge_rejects_out_of_rubric_score() {
        let judge = ExternalCommandJudge::new(
            "/bin/sh",
            ["-c", "printf '{\"score\":2.0,\"rationale\":\"too high\"}'"],
        );
        let error = judge
            .judge(&BenchmarkJudgeInput {
                question: "q".to_owned(),
                gold: "a".to_owned(),
                retrieved_context: vec![],
                answer_basis: String::new(),
            })
            .expect_err("judge should reject out-of-range score");
        assert!(matches!(error, JudgeError::OutOfRange { .. }), "expected out-of-range, got {error:?}");
    }

    #[test]
    fn external_command_judge_rejects_non_finite_score() {
        // 1e309 overflows to +inf in JSON float parsing.
        let judge = ExternalCommandJudge::new(
            "/bin/sh",
            ["-c", "printf '{\"score\":1e309,\"rationale\":\"not finite\"}'"],
        );
        let error = judge
            .judge(&BenchmarkJudgeInput {
                question: "q".to_owned(),
                gold: "a".to_owned(),
                retrieved_context: vec![],
                answer_basis: String::new(),
            })
            .expect_err("judge should reject non-finite score");
        assert!(matches!(error, JudgeError::NonFinite { .. }), "expected non-finite, got {error:?}");
    }
}
