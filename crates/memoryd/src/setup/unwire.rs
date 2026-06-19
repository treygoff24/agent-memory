//! MCP un-wiring for `memoryd uninstall`.
//!
//! The reverse of [`mcp_wire`](super::mcp_wire): remove the `memorum` MCP server
//! entry that setup wrote, and nothing else. Pure merge helpers operate on
//! config text so removal is testable without touching a developer's real Claude
//! or Codex state.
//!
//! Removal is deliberately narrow. Only an entry named exactly `memorum` whose
//! `command` is `memoryd` is removed — a user who repointed `memorum` at a
//! different binary, or who named an unrelated server `memorum`, keeps their
//! entry. For Claude this scans both the user scope (top-level `mcpServers`) and
//! every project scope (`projects.<path>.mcpServers`), because either `memoryd
//! init` lane or a hand edit could have written it at either level.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use toml_edit::DocumentMut;

use super::hooks_wire::RECALL_HOOK_MARKER;
use super::mcp_wire::WireError;

/// The MCP server name setup writes and uninstall removes.
pub const MEMORUM_SERVER_NAME: &str = "memorum";
/// The command an entry must carry to be recognized as ours.
pub const MEMORUM_SERVER_COMMAND: &str = "memoryd";

/// The lifecycle hook events Memorum wires; uninstall scans each for an entry to
/// remove. Mirrors `hooks_wire`'s event set.
const HOOK_EVENTS: &[&str] = &["SessionStart", "UserPromptSubmit", "SubagentStart"];

/// In-memory removal result for a single config body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUnwireOutcome {
    /// Number of `memorum`/`memoryd` entries removed (user + project scopes).
    pub removed: usize,
    /// The rewritten config body. Only meaningful when `removed > 0`.
    pub body: String,
}

/// Resolve the Claude MCP config path: `$CLAUDE_CONFIG_DIR/.claude.json` else
/// `~/.claude.json`. This is the config `claude mcp add` mutates, distinct from
/// the `~/.claude/settings.json` that carries `autoMemoryDirectory`.
pub fn claude_config_path(env_config_dir: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = env_config_dir.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(dir).join(".claude.json"));
    }
    home.map(|home| home.join(".claude.json"))
}

/// Resolve the Codex MCP config path: `$CODEX_HOME/config.toml` else
/// `~/.codex/config.toml`. Mirrors `mcp_wire::codex_config_path`.
pub fn codex_config_path(env_codex_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = env_codex_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(dir).join("config.toml"));
    }
    home.map(|home| home.join(".codex").join("config.toml"))
}

/// Remove the `memorum`/`memoryd` MCP entry from a Claude-style JSON config.
///
/// Scrubs both the user scope (top-level `mcpServers`) and every project scope
/// (`projects.<path>.mcpServers`). All sibling servers, unrelated projects, and
/// every other top-level field are preserved. An empty `mcpServers` object left
/// behind by the removal is dropped so the config does not accumulate empty
/// scaffolding.
pub fn remove_memorum_mcp_json(existing: &str) -> Result<ConfigUnwireOutcome, WireError> {
    let mut document = parse_json_document(existing)?;
    let root = document
        .as_object_mut()
        .ok_or(WireError::InvalidConfigShape("Claude MCP config root must be a JSON object"))?;

    let mut removed = remove_from_servers_object(root);

    if let Some(Value::Object(projects)) = root.get_mut("projects") {
        for project in projects.values_mut() {
            if let Some(scope) = project.as_object_mut() {
                removed += remove_from_servers_object(scope);
            }
        }
    }

    let body = if removed > 0 { format!("{}\n", serde_json::to_string_pretty(&document)?) } else { String::new() };
    Ok(ConfigUnwireOutcome { removed, body })
}

/// Remove a `memorum`/`memoryd` entry from one `mcpServers` object in place.
///
/// `scope` is the object that *holds* `mcpServers` (the config root or a single
/// project entry). Returns 1 if the entry was removed, 0 otherwise. A now-empty
/// `mcpServers` is dropped.
fn remove_from_servers_object(scope: &mut Map<String, Value>) -> usize {
    let Some(servers) = scope.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return 0;
    };
    if !entry_is_memorum(servers.get(MEMORUM_SERVER_NAME)) {
        return 0;
    }
    servers.remove(MEMORUM_SERVER_NAME);
    if servers.is_empty() {
        scope.remove("mcpServers");
    }
    1
}

/// Whether a JSON `mcpServers.memorum` value is the entry setup wrote: an object
/// whose `command` is exactly `memoryd`. A missing entry, or one repointed at a
/// different binary, is left untouched.
fn entry_is_memorum(entry: Option<&Value>) -> bool {
    entry.and_then(Value::as_object).and_then(|server| server.get("command")).and_then(Value::as_str)
        == Some(MEMORUM_SERVER_COMMAND)
}

/// Remove the `[mcp_servers.memorum]` entry from a Codex TOML config.
///
/// Sibling servers and unrelated top-level config are preserved by `toml_edit`.
/// A now-empty `[mcp_servers]` table is dropped.
pub fn remove_memorum_mcp_toml(existing: &str) -> Result<ConfigUnwireOutcome, WireError> {
    let mut document = parse_toml_document(existing)?;
    let Some(servers) = document.get_mut("mcp_servers").and_then(|item| item.as_table_like_mut()) else {
        return Ok(ConfigUnwireOutcome { removed: 0, body: String::new() });
    };

    if !codex_entry_is_memorum(servers.get(MEMORUM_SERVER_NAME)) {
        return Ok(ConfigUnwireOutcome { removed: 0, body: String::new() });
    }
    servers.remove(MEMORUM_SERVER_NAME);
    let servers_now_empty = servers.is_empty();
    if servers_now_empty {
        document.as_table_mut().remove("mcp_servers");
    }

    Ok(ConfigUnwireOutcome { removed: 1, body: document.to_string() })
}

/// Whether a Codex `mcp_servers.memorum` item is the entry setup wrote: a table
/// whose `command` is exactly `memoryd`.
fn codex_entry_is_memorum(item: Option<&toml_edit::Item>) -> bool {
    item.and_then(toml_edit::Item::as_table_like)
        .and_then(|table| table.get("command"))
        .and_then(toml_edit::Item::as_str)
        == Some(MEMORUM_SERVER_COMMAND)
}

/// Resolve the Claude settings.json path holding the recall hooks:
/// `$CLAUDE_CONFIG_DIR/settings.json` else `~/.claude/settings.json`. Distinct
/// from [`claude_config_path`], which resolves the `.claude.json` MCP file.
/// Re-exported from `hooks_wire` so uninstall and setup agree on the location.
pub use super::hooks_wire::claude_settings_path;
/// Resolve the Codex `hooks.json` path: `$CODEX_HOME/hooks.json` else
/// `~/.codex/hooks.json`. Re-exported from `hooks_wire`.
pub use super::hooks_wire::codex_hooks_path;

/// Remove the Memorum recall hooks from a Claude-style `settings.json` body.
///
/// Scans each lifecycle event array under the top-level `hooks` object and drops
/// every matcher group whose command carries the stable [`RECALL_HOOK_MARKER`]
/// — matching on the marker, not the absolute binary path, so an upgraded path
/// is still recognized. Sibling (non-Memorum) hooks and unrelated config are
/// preserved. Empty event arrays and an emptied `hooks` object left behind are
/// dropped so no empty scaffolding accumulates.
pub fn remove_memorum_hooks_json(existing: &str) -> Result<ConfigUnwireOutcome, WireError> {
    let mut document = parse_json_document(existing)?;
    let root = document
        .as_object_mut()
        .ok_or(WireError::InvalidConfigShape("Claude settings config root must be a JSON object"))?;

    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(ConfigUnwireOutcome { removed: 0, body: String::new() });
    };

    let mut removed = 0;
    for event in HOOK_EVENTS {
        removed += remove_marked_groups_from_event(hooks, event);
    }

    if removed == 0 {
        return Ok(ConfigUnwireOutcome { removed, body: String::new() });
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }

    Ok(ConfigUnwireOutcome { removed, body: format!("{}\n", serde_json::to_string_pretty(&document)?) })
}

/// Drop every Memorum-marked matcher group from one event array, removing the
/// array entirely when it ends up empty. Returns the number of groups removed.
fn remove_marked_groups_from_event(hooks: &mut Map<String, Value>, event: &str) -> usize {
    let Some(array) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
        return 0;
    };
    let before = array.len();
    array.retain(|group| !json_group_is_memorum(group));
    let removed = before - array.len();
    if array.is_empty() {
        hooks.remove(event);
    }
    removed
}

/// Whether a Claude matcher group carries a Memorum recall command: any inner
/// `hooks[].command` containing the stable [`RECALL_HOOK_MARKER`].
fn json_group_is_memorum(group: &Value) -> bool {
    group.get("hooks").and_then(Value::as_array).is_some_and(|hooks| hooks.iter().any(json_hook_command_is_memorum))
}

fn json_hook_command_is_memorum(hook: &Value) -> bool {
    hook.get("command").and_then(Value::as_str).is_some_and(|command| command.contains(RECALL_HOOK_MARKER))
}

/// Remove the Memorum recall hooks from a Codex `[hooks]` TOML body.
///
/// Scans each lifecycle event array under `[hooks]` and drops every group whose
/// command carries the stable [`RECALL_HOOK_MARKER`]. Sibling hooks and
/// unrelated top-level config are preserved by `toml_edit`. Empty event arrays
/// and an emptied `[hooks]` table are dropped.
pub fn remove_memorum_hooks_toml(existing: &str) -> Result<ConfigUnwireOutcome, WireError> {
    let mut document = parse_toml_document(existing)?;
    let Some(hooks) = document.get_mut("hooks").and_then(|item| item.as_table_mut()) else {
        return Ok(ConfigUnwireOutcome { removed: 0, body: String::new() });
    };

    let mut removed = 0;
    for event in HOOK_EVENTS {
        removed += remove_marked_groups_from_toml_event(hooks, event);
    }

    if removed == 0 {
        return Ok(ConfigUnwireOutcome { removed, body: String::new() });
    }
    let hooks_now_empty = hooks.is_empty();
    if hooks_now_empty {
        document.as_table_mut().remove("hooks");
    }

    Ok(ConfigUnwireOutcome { removed, body: document.to_string() })
}

/// Drop every Memorum-marked group from one Codex event array of tables.
fn remove_marked_groups_from_toml_event(hooks: &mut toml_edit::Table, event: &str) -> usize {
    let Some(array) = hooks.get_mut(event).and_then(toml_edit::Item::as_array_of_tables_mut) else {
        return 0;
    };
    let before = array.len();
    array.retain(|group| !toml_group_is_memorum(group));
    let removed = before - array.len();
    if array.is_empty() {
        hooks.remove(event);
    }
    removed
}

/// Whether a Codex inline hook group carries a Memorum recall command.
fn toml_group_is_memorum(group: &toml_edit::Table) -> bool {
    group.get("hooks").and_then(toml_edit::Item::as_array_of_tables).is_some_and(|handlers| {
        handlers.iter().any(|handler| {
            handler
                .get("command")
                .and_then(toml_edit::Item::as_str)
                .is_some_and(|command| command.contains(RECALL_HOOK_MARKER))
        })
    })
}

fn parse_json_document(existing: &str) -> Result<Value, WireError> {
    if existing.trim().is_empty() {
        Ok(Value::Object(Map::new()))
    } else {
        Ok(serde_json::from_str(existing)?)
    }
}

fn parse_toml_document(existing: &str) -> Result<DocumentMut, WireError> {
    if existing.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        Ok(existing.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_config_path_prefers_env_over_home() {
        let path = claude_config_path(Some("/cfg"), Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/cfg/.claude.json"));
        let path = claude_config_path(None, Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/home/u/.claude.json"));
        let path = claude_config_path(Some(""), Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/home/u/.claude.json"));
    }

    #[test]
    fn codex_config_path_prefers_env_over_home() {
        let path = codex_config_path(Some("/codex"), Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/codex/config.toml"));
        let path = codex_config_path(None, Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/home/u/.codex/config.toml"));
    }

    #[test]
    fn removes_memorum_from_user_and_project_scope_preserving_siblings() {
        let existing = r#"{
          "model": "claude-opus",
          "mcpServers": {
            "memorum": { "command": "memoryd", "args": ["mcp"] },
            "other": { "command": "other-bin", "args": [] }
          },
          "projects": {
            "/a": {
              "mcpServers": {
                "memorum": { "command": "memoryd", "args": ["mcp", "--socket", "/x"] }
              },
              "allowedTools": ["read"]
            },
            "/b": {
              "mcpServers": { "keep": { "command": "keep-bin" } }
            }
          }
        }"#;

        let outcome = remove_memorum_mcp_json(existing).expect("unwire");
        assert_eq!(outcome.removed, 2, "user scope + one project scope");

        let parsed: Value = serde_json::from_str(&outcome.body).expect("body json");
        let root = parsed.as_object().expect("root object");
        // Unrelated top-level field preserved.
        assert_eq!(root.get("model").and_then(Value::as_str), Some("claude-opus"));
        // Sibling server preserved; memorum gone.
        let user_servers = root.get("mcpServers").and_then(Value::as_object).expect("user servers");
        assert!(!user_servers.contains_key("memorum"));
        assert!(user_servers.contains_key("other"));
        // Project /a: empty mcpServers dropped, allowedTools preserved.
        let project_a = root.get("projects").and_then(|p| p.get("/a")).and_then(Value::as_object).expect("project a");
        assert!(!project_a.contains_key("mcpServers"), "empty mcpServers should be dropped");
        assert!(project_a.contains_key("allowedTools"));
        // Project /b untouched.
        let project_b_servers = root
            .get("projects")
            .and_then(|p| p.get("/b"))
            .and_then(|p| p.get("mcpServers"))
            .and_then(Value::as_object)
            .expect("project b servers");
        assert!(project_b_servers.contains_key("keep"));
    }

    #[test]
    fn leaves_non_memoryd_memorum_entry_untouched() {
        let existing = r#"{
          "mcpServers": {
            "memorum": { "command": "some-other-bin", "args": [] }
          }
        }"#;
        let outcome = remove_memorum_mcp_json(existing).expect("unwire");
        assert_eq!(outcome.removed, 0, "entry not commanded by memoryd is left alone");
        assert!(outcome.body.is_empty());
    }

    #[test]
    fn absent_entry_is_a_noop() {
        let outcome = remove_memorum_mcp_json(r#"{ "mcpServers": { "other": { "command": "x" } } }"#).expect("unwire");
        assert_eq!(outcome.removed, 0);
        let outcome = remove_memorum_mcp_json("").expect("unwire empty");
        assert_eq!(outcome.removed, 0);
    }

    #[test]
    fn removes_memorum_from_codex_toml_preserving_siblings() {
        let existing = "\
model = \"gpt\"\n\
\n\
[mcp_servers.memorum]\n\
command = \"memoryd\"\n\
args = [\"mcp\", \"--socket\", \"/x\"]\n\
\n\
[mcp_servers.other]\n\
command = \"other-bin\"\n\
args = []\n";

        let outcome = remove_memorum_mcp_toml(existing).expect("unwire");
        assert_eq!(outcome.removed, 1);
        let document: DocumentMut = outcome.body.parse().expect("toml");
        assert_eq!(document.get("model").and_then(toml_edit::Item::as_str), Some("gpt"));
        let servers = document.get("mcp_servers").and_then(toml_edit::Item::as_table_like).expect("servers");
        assert!(servers.get("memorum").is_none());
        assert!(servers.get("other").is_some());
    }

    #[test]
    fn codex_empty_table_is_dropped_and_non_memoryd_left_alone() {
        let only = "[mcp_servers.memorum]\ncommand = \"memoryd\"\nargs = [\"mcp\"]\n";
        let outcome = remove_memorum_mcp_toml(only).expect("unwire");
        assert_eq!(outcome.removed, 1);
        let document: DocumentMut = outcome.body.parse().expect("toml");
        assert!(document.get("mcp_servers").is_none(), "empty mcp_servers table should be dropped");

        let other = "[mcp_servers.memorum]\ncommand = \"not-memoryd\"\nargs = []\n";
        let outcome = remove_memorum_mcp_toml(other).expect("unwire");
        assert_eq!(outcome.removed, 0);
    }

    #[test]
    fn claude_settings_path_resolves_settings_json() {
        let path = claude_settings_path(Some("/cfg"), Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/cfg/settings.json"));
        let path = claude_settings_path(None, Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/home/u/.claude/settings.json"));
    }

    #[test]
    fn removes_marked_hooks_preserving_siblings_and_dropping_empties() {
        let existing = r#"{
          "model": "claude-opus",
          "hooks": {
            "SessionStart": [
              { "hooks": [ { "type": "command", "command": "echo sibling" } ] },
              { "matcher": "startup|resume|clear|compact", "hooks": [ { "type": "command", "command": "\"/v2/memoryd\" recall hook --socket \"/s.sock\" --harness claude-code", "timeout": 2 } ] }
            ],
            "UserPromptSubmit": [
              { "hooks": [ { "type": "command", "command": "\"/v2/memoryd\" recall hook --socket \"/s.sock\" --harness claude-code", "timeout": 2 } ] }
            ]
          }
        }"#;

        let outcome = remove_memorum_hooks_json(existing).expect("unwire");
        // One from SessionStart, one from UserPromptSubmit.
        assert_eq!(outcome.removed, 2);

        let parsed: Value = serde_json::from_str(&outcome.body).expect("json");
        let root = parsed.as_object().expect("root");
        assert_eq!(root.get("model").and_then(Value::as_str), Some("claude-opus"), "unrelated field preserved");

        let session_start = parsed["hooks"]["SessionStart"].as_array().expect("array");
        assert_eq!(session_start.len(), 1, "sibling survives, memorum dropped");
        assert_eq!(session_start[0]["hooks"][0]["command"], "echo sibling");

        // UserPromptSubmit held only the memorum hook; the emptied array is dropped.
        assert!(parsed["hooks"].as_object().expect("hooks obj").get("UserPromptSubmit").is_none());
    }

    #[test]
    fn marked_removal_drops_empty_hooks_object() {
        let existing = r#"{
          "hooks": {
            "SessionStart": [
              { "hooks": [ { "type": "command", "command": "\"/x/memoryd\" recall hook --socket \"/s\" --harness claude-code" } ] }
            ]
          }
        }"#;
        let outcome = remove_memorum_hooks_json(existing).expect("unwire");
        assert_eq!(outcome.removed, 1);
        let parsed: Value = serde_json::from_str(&outcome.body).expect("json");
        assert!(parsed.as_object().expect("root").get("hooks").is_none(), "emptied hooks object dropped");
    }

    #[test]
    fn marked_removal_is_noop_without_memorum_hooks() {
        let existing =
            r#"{ "hooks": { "SessionStart": [ { "hooks": [ { "type": "command", "command": "echo hi" } ] } ] } }"#;
        let outcome = remove_memorum_hooks_json(existing).expect("unwire");
        assert_eq!(outcome.removed, 0);
        assert!(outcome.body.is_empty());

        let outcome = remove_memorum_hooks_json("").expect("unwire empty");
        assert_eq!(outcome.removed, 0);
    }

    #[test]
    fn removes_marked_codex_inline_hooks_preserving_siblings() {
        let existing = "\
model = \"gpt\"\n\
\n\
[[hooks.SessionStart]]\n\
[[hooks.SessionStart.hooks]]\n\
type = \"command\"\n\
command = \"echo sibling\"\n\
\n\
[[hooks.UserPromptSubmit]]\n\
[[hooks.UserPromptSubmit.hooks]]\n\
type = \"command\"\n\
command = \"\\\"/v2/memoryd\\\" recall hook --socket \\\"/s.sock\\\" --harness codex\"\n\
timeout = 2\n";

        let outcome = remove_memorum_hooks_toml(existing).expect("unwire");
        assert_eq!(outcome.removed, 1);
        let document: DocumentMut = outcome.body.parse().expect("toml");
        assert_eq!(document.get("model").and_then(toml_edit::Item::as_str), Some("gpt"), "unrelated config preserved");
        let hooks = document.get("hooks").and_then(toml_edit::Item::as_table).expect("hooks table");
        // Sibling SessionStart hook survives; emptied UserPromptSubmit dropped.
        assert!(hooks.get("SessionStart").is_some());
        assert!(hooks.get("UserPromptSubmit").is_none(), "emptied event dropped");
    }

    #[test]
    fn codex_inline_removal_drops_empty_hooks_table() {
        let existing = "\
[[hooks.SessionStart]]\n\
[[hooks.SessionStart.hooks]]\n\
type = \"command\"\n\
command = \"\\\"/x/memoryd\\\" recall hook --socket \\\"/s\\\" --harness codex\"\n";
        let outcome = remove_memorum_hooks_toml(existing).expect("unwire");
        assert_eq!(outcome.removed, 1);
        let document: DocumentMut = outcome.body.parse().expect("toml");
        assert!(document.get("hooks").is_none(), "emptied hooks table dropped");
    }
}
