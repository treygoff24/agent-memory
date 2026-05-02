use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use memorum_eval::assertions::assert_no_pii_on_disk;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::harness_runner::{
    HarnessRunResult, HarnessRunner, RealHarness, HARNESS_MCP_CONFIG_PATH_ENV, HARNESS_PROJECT_CWD_ENV,
};
use serde_json::{json, Value};

use crate::support::daemon_request;

const CLAUDE_KEY_ENV: &str = "MEMORUM_EVAL_CLAUDE_KEY";
const HARNESS_TIMEOUT: Duration = Duration::from_secs(180);
const RAW_PHONE_DIGITS: &str = "15550000001";
const RETRY_SENTINEL: &str = "EVAL_T15_PRIVACY_RETRY";
const T15_PROMPT: &str = include_str!("../../../fixtures/prompts/t15_privacy_retry.md");

#[tokio::test]
async fn t15_privacy_filter_refusal_and_retry() {
    if std::env::var_os(CLAUDE_KEY_ENV).is_none() {
        eprintln!("SKIP_NO_AUTH: set {CLAUDE_KEY_ENV} to run real-harness Test #15.");
        return;
    }

    if missing_claude_cli() {
        return;
    }

    let scaffold = DaemonScaffold::fresh().await;
    let claude = HarnessRunner::new_with_socket(RealHarness::Claude, scaffold.socket_path());
    let mcp_config =
        claude.write_mcp_config_file(scaffold.tree_dir(), "t15-privacy-retry").expect("write Claude MCP config");
    let prompt = render_prompt(T15_PROMPT, scaffold.tree_dir());
    let output = run_claude_with_one_parse_retry(&claude, &prompt, &phase_env(scaffold.tree_dir(), &mcp_config)).await;

    assert_first_write_refused(&output);
    assert_retry_succeeded(&output);
    assert_retry_memory_is_searchable(scaffold.socket_path());
    assert_raw_phone_is_not_searchable(scaffold.socket_path());
    assert_no_pii_on_disk(scaffold.tree_dir(), RAW_PHONE_DIGITS)
        .expect("raw phone digits must not persist in temp tree");
}

fn missing_claude_cli() -> bool {
    match HarnessRunner::detect_cli(RealHarness::Claude) {
        Ok(Some(_)) => false,
        Ok(None) => {
            eprintln!("SKIP_MISSING_CLI: claude not found in PATH. Install and authenticate to run Test #15.");
            true
        }
        Err(error) => panic!("{error}"),
    }
}

fn phase_env(project_cwd: &Path, mcp_config: &Path) -> HashMap<String, String> {
    let mut env = HashMap::from([
        (HARNESS_MCP_CONFIG_PATH_ENV.to_owned(), mcp_config.to_string_lossy().into_owned()),
        (HARNESS_PROJECT_CWD_ENV.to_owned(), project_cwd.to_string_lossy().into_owned()),
        ("MEMORUM_EVAL_SOCKET_PATH".to_owned(), project_cwd.join("memoryd.sock").to_string_lossy().into_owned()),
    ]);

    copy_env(&mut env, CLAUDE_KEY_ENV);
    copy_env(&mut env, "ANTHROPIC_API_KEY");
    alias_eval_key(&mut env, CLAUDE_KEY_ENV, "ANTHROPIC_API_KEY");
    copy_env(&mut env, "CLAUDE_CONFIG_DIR");
    copy_env(&mut env, "HOME");
    copy_env(&mut env, "PATH");
    env
}

fn copy_env(env: &mut HashMap<String, String>, key: &str) {
    if let Ok(value) = std::env::var(key) {
        env.insert(key.to_owned(), value);
    }
}

fn alias_eval_key(env: &mut HashMap<String, String>, eval_key: &str, harness_key: &str) {
    if !env.contains_key(harness_key) {
        if let Some(value) = env.get(eval_key).cloned() {
            env.insert(harness_key.to_owned(), value);
        }
    }
}

fn render_prompt(template: &str, project_cwd: &Path) -> String {
    template.replace("{{PROJECT_CWD}}", &project_cwd.to_string_lossy())
}

async fn run_claude_with_one_parse_retry(claude: &HarnessRunner, prompt: &str, env: &HashMap<String, String>) -> Value {
    let first = claude.run(prompt, env, HARNESS_TIMEOUT).await;
    assert_harness_success("Claude privacy retry phase", &first);
    if let Some(parsed) = parse_last_json_object(&first.stdout) {
        return parsed;
    }

    eprintln!("HARNESS_OUTPUT_PARSE_FAILURE: Claude output was not JSON; retrying Test #15 once.");
    let retry = claude.run(prompt, env, HARNESS_TIMEOUT).await;
    assert_harness_success("Claude privacy retry parse-retry phase", &retry);
    parse_last_json_object(&retry.stdout).unwrap_or_else(|| {
        panic!(
            "HARNESS_OUTPUT_PARSE_FAILURE: Claude output was not parseable JSON after one retry\nstdout={}\nstderr={}",
            retry.stdout, retry.stderr
        )
    })
}

fn assert_harness_success(phase: &str, result: &HarnessRunResult) {
    assert_eq!(
        result.exit_code, 0,
        "{phase} failed with exit code {}\nstdout={}\nstderr={}",
        result.exit_code, result.stdout, result.stderr
    );
}

fn parse_last_json_object(stdout: &str) -> Option<Value> {
    serde_json::from_str(stdout.trim()).ok().or_else(|| {
        stdout
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .and_then(|line| serde_json::from_str(line).ok())
    })
}

fn assert_first_write_refused(output: &Value) {
    let status = output.pointer("/first_attempt_status").and_then(Value::as_str).unwrap_or_default();
    assert!(
        status.to_ascii_lowercase().contains("refused"),
        "first PII write should be refused; Claude output: {output:#?}"
    );
}

fn assert_retry_succeeded(output: &Value) {
    let retry_status = output.pointer("/retry_status").and_then(Value::as_str).unwrap_or_default();
    assert!(
        matches!(retry_status, "promoted" | "candidate"),
        "AGENT_DID_NOT_RETRY: retry_status should be promoted or candidate; Claude output: {output:#?}"
    );

    let retry_id = output.pointer("/retry_id").and_then(Value::as_str).unwrap_or_default();
    assert!(!retry_id.is_empty(), "retry_id should be non-null/non-empty; Claude output: {output:#?}");
    if retry_status == "promoted" {
        assert!(retry_id.starts_with("mem_"), "promoted retry_id should look like a memory id: {retry_id}");
    }
}

fn assert_retry_memory_is_searchable(socket_path: &Path) {
    let search =
        daemon_request(socket_path, json!({"search": {"query": RETRY_SENTINEL, "limit": null, "include_body": true}}));
    assert!(
        search_total(&search) >= 1,
        "memory_search should find the sanitized retry by sentinel {RETRY_SENTINEL}: {search:#?}"
    );
    assert!(
        !search_hits_text(&search).contains(RAW_PHONE_DIGITS),
        "sentinel search results must not include raw phone digits: {search:#?}"
    );
}

fn assert_raw_phone_is_not_searchable(socket_path: &Path) {
    let search = daemon_request(
        socket_path,
        json!({"search": {"query": RAW_PHONE_DIGITS, "limit": null, "include_body": true}}),
    );
    assert_eq!(search_total(&search), 0, "raw phone digits should not be searchable: {search:#?}");
}

fn search_total(response: &Value) -> u64 {
    response.pointer("/result/success/search/total").and_then(Value::as_u64).unwrap_or_default()
}

fn search_hits_text(response: &Value) -> String {
    response
        .pointer("/result/success/search/hits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hit| hit.as_object())
        .flat_map(|hit| {
            [
                hit.get("summary").and_then(Value::as_str).unwrap_or_default(),
                hit.get("snippet").and_then(Value::as_str).unwrap_or_default(),
            ]
        })
        .collect::<Vec<_>>()
        .join("\n")
}
