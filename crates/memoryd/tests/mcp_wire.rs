use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use memoryd::setup::mcp_wire::{
    claude_mcp_add_args, merge_claude_mcp_json, merge_codex_mcp_toml, wire_with_runtime, CommandResult, McpWireRuntime,
};
use memoryd::setup::{HarnessTarget, McpServerSpec, WireError, WireMode, WireStatus};

fn memorum_spec() -> McpServerSpec {
    McpServerSpec::new("memorum", "memoryd", vec!["mcp".into(), "--socket".into(), "/tmp/memoryd.sock".into()])
}

fn successful_claude_add() -> CommandResult {
    CommandResult { success: true, stdout: "Added MCP server memorum".to_string(), stderr: String::new() }
}

fn already_exists_claude_add() -> CommandResult {
    CommandResult {
        success: false,
        stdout: String::new(),
        stderr: "MCP server named memorum already exists".to_string(),
    }
}

#[test]
fn codex_toml_merge_preserves_sibling_servers() {
    let existing = r#"
model = "gpt-5.4"

[mcp_servers.other]
command = "otherd"
args = ["serve"]
"#;

    let outcome = merge_codex_mcp_toml(existing, &memorum_spec()).expect("merge succeeds");

    assert_eq!(outcome.status, WireStatus::Wired);
    assert!(outcome.body.contains("[mcp_servers.other]"));
    assert!(outcome.body.contains("command = \"otherd\""));
    assert!(outcome.body.contains("[mcp_servers.memorum]"));
    assert!(outcome.body.contains("command = \"memoryd\""));
    assert!(outcome.body.contains("args = [\"mcp\", \"--socket\", \"/tmp/memoryd.sock\"]"));
}

#[test]
fn codex_toml_remerge_reports_already_current() {
    let first = merge_codex_mcp_toml("", &memorum_spec()).expect("initial merge succeeds");
    assert_eq!(first.status, WireStatus::Wired);

    let second = merge_codex_mcp_toml(&first.body, &memorum_spec()).expect("remerge succeeds");

    assert_eq!(second.status, WireStatus::AlreadyCurrent);
    assert_eq!(second.body, first.body);
}

#[test]
fn codex_toml_remerge_repairs_malformed_args() {
    let existing = r#"
[mcp_servers.memorum]
command = "memoryd"
args = ["mcp", "--socket", "/tmp/memoryd.sock", true]
"#;

    let outcome = merge_codex_mcp_toml(existing, &memorum_spec()).expect("merge succeeds");

    assert_eq!(outcome.status, WireStatus::Updated);
    assert!(outcome.body.contains("args = [\"mcp\", \"--socket\", \"/tmp/memoryd.sock\"]"));
    assert!(!outcome.body.contains("true"));
}

#[test]
fn codex_toml_rejects_non_table_mcp_servers() {
    let error = merge_codex_mcp_toml("mcp_servers = \"legacy string\"\n", &memorum_spec())
        .expect_err("non-table mcp_servers must not be overwritten");

    assert!(
        matches!(error, WireError::InvalidConfigShape("Codex mcp_servers must be a TOML table")),
        "unexpected error: {error}"
    );
}

#[test]
fn claude_json_merge_preserves_sibling_servers() {
    let existing = r#"
{
  "mcpServers": {
    "other": {
      "command": "otherd",
      "args": ["serve"]
    }
  },
  "theme": "dark"
}
"#;

    let outcome = merge_claude_mcp_json(existing, &memorum_spec()).expect("merge succeeds");
    let parsed: serde_json::Value = serde_json::from_str(&outcome.body).expect("valid json");

    assert_eq!(outcome.status, WireStatus::Wired);
    assert_eq!(parsed["theme"], "dark");
    assert_eq!(parsed["mcpServers"]["other"]["command"], "otherd");
    assert_eq!(parsed["mcpServers"]["memorum"]["command"], "memoryd");
    assert_eq!(parsed["mcpServers"]["memorum"]["args"], serde_json::json!(["mcp", "--socket", "/tmp/memoryd.sock"]));
}

#[test]
fn print_only_writes_nothing() {
    let mut runtime = FakeRuntime::default().with_home(PathBuf::from("/home/tester"));

    let outcome = wire_with_runtime(HarnessTarget::Codex, &memorum_spec(), WireMode::PrintOnly, &mut runtime)
        .expect("print-only succeeds");

    assert_eq!(outcome.status, WireStatus::PrintedOnly);
    assert_eq!(runtime.write_count, 0);
    assert_eq!(runtime.claude_add_count, 0);
    assert!(runtime.files.is_empty());

    let outcome = wire_with_runtime(HarnessTarget::Claude, &memorum_spec(), WireMode::PrintOnly, &mut runtime)
        .expect("print-only succeeds");

    assert_eq!(outcome.status, WireStatus::PrintedOnly);
    assert_eq!(runtime.write_count, 0);
    assert_eq!(runtime.claude_add_count, 0);
    assert!(runtime.files.is_empty());
}

#[test]
fn codex_apply_honors_codex_home_config_path() {
    let mut runtime = FakeRuntime::default().with_env("CODEX_HOME", "/custom/codex");

    let outcome = wire_with_runtime(HarnessTarget::Codex, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("codex config write succeeds");

    assert_eq!(outcome.status, WireStatus::Wired);
    assert!(runtime.files.contains_key(Path::new("/custom/codex/config.toml")));
}

#[test]
fn codex_apply_preserves_backup_when_updating_existing_config() {
    let config_path = PathBuf::from("/custom/codex/config.toml");
    let mut runtime = FakeRuntime::default().with_env("CODEX_HOME", "/custom/codex");
    runtime.files.insert(config_path.clone(), "model = \"gpt-5.4\"\n".to_string());

    let outcome = wire_with_runtime(HarnessTarget::Codex, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("codex config update succeeds");

    assert_eq!(outcome.status, WireStatus::Wired);
    assert_eq!(runtime.safe_write_count, 1);
    assert_eq!(runtime.backups.get(&config_path).map(String::as_str), Some("model = \"gpt-5.4\"\n"));
    assert!(runtime.files[&config_path].contains("[mcp_servers.memorum]"));
}

#[test]
fn codex_apply_rejects_invalid_top_level_mcp_servers_without_writing() {
    let config_path = PathBuf::from("/custom/codex/config.toml");
    let mut runtime = FakeRuntime::default().with_env("CODEX_HOME", "/custom/codex");
    runtime.files.insert(config_path.clone(), "mcp_servers = \"legacy string\"\n".to_string());

    let error = wire_with_runtime(HarnessTarget::Codex, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect_err("invalid config shape should fail closed");

    assert!(matches!(error, WireError::InvalidConfigShape("Codex mcp_servers must be a TOML table")));
    assert_eq!(runtime.safe_write_count, 0);
    assert_eq!(runtime.files[&config_path], "mcp_servers = \"legacy string\"\n");
}

#[test]
fn claude_falls_back_to_user_config_when_cli_is_absent() {
    let mut runtime = FakeRuntime::default().with_home(PathBuf::from("/home/tester"));

    let outcome = wire_with_runtime(HarnessTarget::Claude, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("claude fallback does not hard-fail setup");

    assert_eq!(outcome.status, WireStatus::Wired);
    assert_eq!(runtime.claude_add_count, 1);
    let config = runtime.files.get(Path::new("/home/tester/.claude.json")).expect("user-scope config written");
    let parsed: serde_json::Value = serde_json::from_str(config).expect("valid json");
    assert_eq!(parsed["mcpServers"]["memorum"]["command"], "memoryd");
}

#[test]
fn claude_fallback_preserves_backup_when_updating_existing_user_config() {
    let config_path = PathBuf::from("/home/tester/.claude.json");
    let mut runtime = FakeRuntime::default().with_home(PathBuf::from("/home/tester"));
    runtime.files.insert(config_path.clone(), "{\"theme\":\"dark\"}\n".to_string());

    let outcome = wire_with_runtime(HarnessTarget::Claude, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("claude fallback config update succeeds");

    assert_eq!(outcome.status, WireStatus::Wired);
    assert_eq!(runtime.safe_write_count, 1);
    assert_eq!(runtime.backups.get(&config_path).map(String::as_str), Some("{\"theme\":\"dark\"}\n"));
    assert!(runtime.files[&config_path].contains("\"mcpServers\""));
}

#[test]
fn claude_cli_success_does_not_write_config_when_nothing_is_stale() {
    let mut runtime = FakeRuntime::default().with_claude_add(successful_claude_add());

    let outcome = wire_with_runtime(HarnessTarget::Claude, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("claude cli succeeds");

    assert_eq!(outcome.status, WireStatus::Wired);
    assert_eq!(runtime.claude_add_count, 1);
    assert_eq!(runtime.write_count, 0);
    assert!(runtime.files.is_empty());
}

#[test]
fn claude_cli_duplicate_add_does_not_write_config_when_nothing_is_stale() {
    let mut runtime = FakeRuntime::default().with_claude_add(already_exists_claude_add());

    let outcome = wire_with_runtime(HarnessTarget::Claude, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("duplicate add is treated as already present");

    assert_eq!(outcome.status, WireStatus::AlreadyCurrent);
    assert_eq!(runtime.claude_add_count, 1);
    assert_eq!(runtime.write_count, 0);
    assert!(runtime.files.is_empty());
}

#[test]
fn claude_failed_cli_and_invalid_project_json_degrades_to_print_only() {
    let mut runtime = FakeRuntime::default()
        .with_current_dir(PathBuf::from("/repo"))
        .with_file(PathBuf::from("/repo/.mcp.json"), "{not json")
        .with_claude_add(CommandResult {
            success: false,
            stdout: String::new(),
            stderr: "Claude CLI rejected the request".to_string(),
        });

    let outcome = wire_with_runtime(HarnessTarget::Claude, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("print-only fallback succeeds");

    assert_eq!(outcome.status, WireStatus::PrintedOnly);
    assert!(outcome.message.as_deref().is_some_and(|message| message.contains("printed JSON instead")));
    assert_eq!(runtime.write_count, 0);
}

#[test]
fn claude_cli_grammar_matches_live_help_fixture() {
    // Source of truth captured from live `claude mcp add --help` on 2026-06-12:
    // `Usage: claude mcp add [options] <name> <commandOrUrl> [args...]`, with
    // `-s, --scope` accepting `user` for cross-project wiring and `--`
    // separating subprocess flags as in the help examples.
    assert_eq!(
        claude_mcp_add_args(&memorum_spec()),
        vec!["mcp", "add", "--scope", "user", "memorum", "--", "memoryd", "mcp", "--socket", "/tmp/memoryd.sock"]
    );
}

#[derive(Default)]
struct FakeRuntime {
    files: BTreeMap<PathBuf, String>,
    env: HashMap<String, String>,
    home: Option<PathBuf>,
    current_dir: PathBuf,
    claude_add_result: Option<CommandResult>,
    write_count: usize,
    safe_write_count: usize,
    backups: BTreeMap<PathBuf, String>,
    claude_add_count: usize,
}

impl FakeRuntime {
    fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    fn with_home(mut self, home: PathBuf) -> Self {
        self.home = Some(home);
        self
    }

    fn with_current_dir(mut self, current_dir: PathBuf) -> Self {
        self.current_dir = current_dir;
        self
    }

    fn with_file(mut self, path: PathBuf, contents: &str) -> Self {
        self.files.insert(path, contents.to_string());
        self
    }

    fn with_claude_add(mut self, result: CommandResult) -> Self {
        self.claude_add_result = Some(result);
        self
    }
}

impl McpWireRuntime for FakeRuntime {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>, WireError> {
        Ok(self.files.get(path).cloned())
    }

    fn write_config_file(&mut self, path: &Path, contents: &str) -> Result<(), WireError> {
        self.write_count += 1;
        self.safe_write_count += 1;
        if let Some(existing) = self.files.get(path) {
            self.backups.insert(path.to_path_buf(), existing.clone());
        }
        self.files.insert(path.to_path_buf(), contents.to_string());
        Ok(())
    }

    fn create_dir_all(&mut self, _path: &Path) -> Result<(), WireError> {
        Ok(())
    }

    fn env_var(&self, key: &str) -> Option<String> {
        self.env.get(key).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn current_dir(&self) -> Result<PathBuf, WireError> {
        Ok(self.current_dir.clone())
    }

    fn claude_mcp_add(&mut self, _args: &[String]) -> Result<Option<CommandResult>, WireError> {
        self.claude_add_count += 1;
        Ok(self.claude_add_result.clone())
    }
}
