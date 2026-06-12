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

use super::mcp_wire::WireError;

/// The MCP server name setup writes and uninstall removes.
pub const MEMORUM_SERVER_NAME: &str = "memorum";
/// The command an entry must carry to be recognized as ours.
pub const MEMORUM_SERVER_COMMAND: &str = "memoryd";

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
    entry
        .and_then(Value::as_object)
        .and_then(|server| server.get("command"))
        .and_then(Value::as_str)
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
}
