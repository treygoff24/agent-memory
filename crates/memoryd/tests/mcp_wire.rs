use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use memoryd::setup::mcp_wire::{
    claude_mcp_add_args, merge_claude_mcp_json, merge_codex_mcp_toml, wire_with_runtime, CommandResult, McpWireRuntime,
};
use memoryd::setup::{HarnessTarget, McpServerSpec, WireError, WireMode, WireStatus};

fn memorum_spec() -> McpServerSpec {
    McpServerSpec::new("memorum", "memoryd", vec!["mcp".into(), "--socket".into(), "/tmp/memoryd.sock".into()])
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
fn claude_falls_back_to_project_json_when_cli_is_absent() {
    let mut runtime = FakeRuntime::default().with_current_dir(PathBuf::from("/repo"));

    let outcome = wire_with_runtime(HarnessTarget::Claude, &memorum_spec(), WireMode::Apply, &mut runtime)
        .expect("claude fallback does not hard-fail setup");

    assert_eq!(outcome.status, WireStatus::Wired);
    let config = runtime.files.get(Path::new("/repo/.mcp.json")).expect("fallback config written");
    let parsed: serde_json::Value = serde_json::from_str(config).expect("valid json");
    assert_eq!(parsed["mcpServers"]["memorum"]["command"], "memoryd");
}

#[test]
fn claude_cli_grammar_matches_live_help_fixture() {
    // Source of truth captured from this worker's live `claude mcp add --help`
    // on 2026-06-01: `Usage: claude mcp add [options] <name> <commandOrUrl> [args...]`.
    // The same help examples show subprocess flags separated as:
    // `claude mcp add my-server -- my-command --some-flag arg1`.
    assert_eq!(
        claude_mcp_add_args(&memorum_spec()),
        vec!["mcp", "add", "memorum", "--", "memoryd", "mcp", "--socket", "/tmp/memoryd.sock"]
    );
}

#[derive(Default)]
struct FakeRuntime {
    files: BTreeMap<PathBuf, String>,
    env: HashMap<String, String>,
    home: Option<PathBuf>,
    current_dir: PathBuf,
    claude_result: Option<CommandResult>,
    write_count: usize,
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
}

impl McpWireRuntime for FakeRuntime {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>, WireError> {
        Ok(self.files.get(path).cloned())
    }

    fn write_string(&mut self, path: &Path, contents: &str) -> Result<(), WireError> {
        self.write_count += 1;
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
        Ok(self.claude_result.clone())
    }
}
