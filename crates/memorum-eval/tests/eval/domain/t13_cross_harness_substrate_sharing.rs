use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::harness_runner::{HarnessRunner, RealHarness, HARNESS_MCP_CONFIG_PATH_ENV, HARNESS_PROJECT_CWD_ENV};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};
use serde_json::{json, Value};

use crate::support::{daemon_request, find_file_with_extension};

const CLAUDE_KEY_ENV: &str = "MEMORUM_EVAL_CLAUDE_KEY";
const CODEX_KEY_ENV: &str = "MEMORUM_EVAL_CODEX_KEY";
const ENTITY_ID: &str = "ent_eval_t13_xk9m";
const FACT_TEXT: &str =
    "EVAL_T13: Found that the build system requires Go 1.22 for cross-compilation targets. This is a hard constraint.";
const HARNESS_TIMEOUT: Duration = Duration::from_secs(180);

#[tokio::test]
async fn t13_cross_harness_substrate_sharing() {
    if missing_auth_keys() {
        eprintln!("SKIP_NO_AUTH: set {CLAUDE_KEY_ENV} and {CODEX_KEY_ENV} to run real-harness Test #13.");
        return;
    }

    if missing_cli(RealHarness::Codex) || missing_cli(RealHarness::Claude) {
        return;
    }

    let scaffold = DaemonScaffold::fresh().await;
    let codex = HarnessRunner::new_with_socket(RealHarness::Codex, scaffold.socket_path());
    let claude = HarnessRunner::new_with_socket(RealHarness::Claude, scaffold.socket_path());

    let codex_config =
        codex.write_mcp_config_file(scaffold.tree_dir(), "t13-codex-observe").expect("write Codex MCP config");
    let codex_prompt = render_prompt(CODEX_OBSERVE_PROMPT, scaffold.tree_dir());
    let codex_result = codex.run(&codex_prompt, &phase_env(scaffold.tree_dir(), &codex_config), HARNESS_TIMEOUT).await;
    assert_harness_success("Codex observe phase", &codex_result);

    let fragment = find_t13_substrate_fragment(scaffold.tree_dir())
        .unwrap_or_else(|| panic!("Codex observe phase should write a substrate record for {ENTITY_ID}"));
    eval_assert_eq!(
        fragment.pointer("/harness").and_then(Value::as_str),
        Some("codex"),
        "substrate fragment should retain Codex harness provenance: {fragment:#?}"
    );

    let claude_config =
        claude.write_mcp_config_file(scaffold.tree_dir(), "t13-claude-recall").expect("write Claude MCP config");
    let claude_prompt = render_prompt(CLAUDE_RECALL_PROMPT, scaffold.tree_dir());
    let claude_output =
        run_claude_with_one_parse_retry(&claude, &claude_prompt, &phase_env(scaffold.tree_dir(), &claude_config)).await;
    eval_assert_eq!(
        claude_output.pointer("/found").and_then(Value::as_bool),
        Some(true),
        "Claude should report the Codex-written fragment as found: {claude_output:#?}"
    );
    eval_assert!(
        fragment_text_preserves_fact(&claude_output),
        "Claude fragment_text should preserve the sentinel fact: {claude_output:#?}"
    );

    let search = daemon_request(
        scaffold.socket_path(),
        json!({"search": {"query": ENTITY_ID, "limit": null, "include_body": true}}),
    );
    eval_assert!(
        search.pointer("/result/success/search/total").and_then(Value::as_u64).unwrap_or_default() >= 1,
        "memory_search should surface the cross-harness substrate fragment: {search:#?}"
    );
    eval_flush_assertion_count();
}

const CODEX_OBSERVE_PROMPT: &str = include_str!("../../../fixtures/prompts/t13_codex_observe.md");
const CLAUDE_RECALL_PROMPT: &str = include_str!("../../../fixtures/prompts/t13_claude_recall.md");

fn missing_auth_keys() -> bool {
    std::env::var_os(CLAUDE_KEY_ENV).is_none() || std::env::var_os(CODEX_KEY_ENV).is_none()
}

fn missing_cli(harness: RealHarness) -> bool {
    match HarnessRunner::detect_cli(harness) {
        Ok(Some(_)) => false,
        Ok(None) => {
            eprintln!(
                "SKIP_MISSING_CLI: {} not found in PATH. Install and authenticate to run Test #13.",
                harness.binary_name()
            );
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
    copy_env(&mut env, CODEX_KEY_ENV);
    copy_env(&mut env, "ANTHROPIC_API_KEY");
    copy_env(&mut env, "OPENAI_API_KEY");
    alias_eval_key(&mut env, CLAUDE_KEY_ENV, "ANTHROPIC_API_KEY");
    alias_eval_key(&mut env, CODEX_KEY_ENV, "OPENAI_API_KEY");
    copy_env(&mut env, "CODEX_HOME");
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
    template
        .replace("{{FACT_TEXT}}", FACT_TEXT)
        .replace("{{ENTITY_ID}}", ENTITY_ID)
        .replace("{{PROJECT_CWD}}", &project_cwd.to_string_lossy())
}

fn assert_harness_success(phase: &str, result: &memorum_eval::harness_runner::HarnessRunResult) {
    eval_assert_eq!(
        result.exit_code,
        0,
        "{phase} failed with exit code {}\nstdout={}\nstderr={}",
        result.exit_code,
        result.stdout,
        result.stderr
    );
}

async fn run_claude_with_one_parse_retry(claude: &HarnessRunner, prompt: &str, env: &HashMap<String, String>) -> Value {
    let first = claude.run(prompt, env, HARNESS_TIMEOUT).await;
    assert_harness_success("Claude recall phase", &first);
    if let Some(parsed) = parse_last_json_object(&first.stdout) {
        return parsed;
    }

    eprintln!("HARNESS_OUTPUT_PARSE_FAILURE: Claude output was not JSON; retrying Test #13 recall phase once.");
    let retry = claude.run(prompt, env, HARNESS_TIMEOUT).await;
    assert_harness_success("Claude recall retry phase", &retry);
    parse_last_json_object(&retry.stdout).unwrap_or_else(|| {
        panic!(
            "HARNESS_OUTPUT_PARSE_FAILURE: Claude output was not parseable JSON after one retry\nstdout={}\nstderr={}",
            retry.stdout, retry.stderr
        )
    })
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

fn fragment_text_preserves_fact(output: &Value) -> bool {
    let Some(text) = output.pointer("/fragment_text").and_then(Value::as_str) else {
        return false;
    };
    text.contains("EVAL_T13")
        || (text.contains("Go 1.22") && text.contains("cross-compilation") && text.contains("hard constraint"))
}

fn find_t13_substrate_fragment(tree_dir: &Path) -> Option<Value> {
    for path in find_file_with_extension(&tree_dir.join("substrate"), "jsonl") {
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in body.lines() {
            let Ok(record) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if record_entities_include(&record, ENTITY_ID) && record_contains_fact(&record) {
                return Some(record);
            }
        }
    }
    None
}

fn record_entities_include(record: &Value, entity_id: &str) -> bool {
    record
        .pointer("/entities")
        .and_then(Value::as_array)
        .is_some_and(|entities| entities.iter().any(|entity| entity.as_str() == Some(entity_id)))
}

fn record_contains_fact(record: &Value) -> bool {
    record.pointer("/text").and_then(Value::as_str).is_some_and(|text| text.contains("EVAL_T13"))
}
