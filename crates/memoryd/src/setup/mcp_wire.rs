//! MCP wiring for setup.
//!
//! The public `wire` entrypoint uses process I/O. Pure merge helpers and the
//! injectable runtime seam keep config mutation testable without touching a
//! developer's real Claude or Codex state.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use toml_edit::{value, Array, DocumentMut, Item, Table};

/// Harness whose MCP configuration should be wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessTarget {
    Claude,
    Codex,
}

/// Desired MCP server command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerSpec {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
}

impl McpServerSpec {
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self { name: name.into(), command: command.into(), args }
    }
}

/// Wiring mode for config writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireMode {
    Apply,
    PrintOnly,
}

/// MCP wiring outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireOutcome {
    pub target: HarnessTarget,
    pub status: WireStatus,
    pub message: Option<String>,
}

/// Status values produced by MCP wiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireStatus {
    Wired,
    AlreadyCurrent,
    Updated,
    PrintedOnly,
    Skipped,
}

/// In-memory config merge result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigMergeOutcome {
    pub status: WireStatus,
    pub body: String,
}

/// Command execution result returned by an injectable wire runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runtime boundary for filesystem, environment, and Claude CLI interactions.
pub trait McpWireRuntime {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>, WireError>;
    fn write_config_file(&mut self, path: &Path, contents: &str) -> Result<(), WireError>;
    fn create_dir_all(&mut self, path: &Path) -> Result<(), WireError>;
    fn env_var(&self, key: &str) -> Option<String>;
    fn home_dir(&self) -> Option<PathBuf>;
    fn current_dir(&self) -> Result<PathBuf, WireError>;
    fn claude_mcp_add(&mut self, args: &[String]) -> Result<Option<CommandResult>, WireError>;
}

/// Errors returned by MCP wiring implementations.
#[derive(Debug, Error)]
pub enum WireError {
    #[error("invalid TOML MCP config: {0}")]
    TomlParse(#[from] toml_edit::TomlError),

    #[error("invalid JSON MCP config: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("invalid MCP config shape: {0}")]
    InvalidConfigShape(&'static str),

    #[error("cannot resolve home directory for {target:?} MCP config")]
    MissingHome { target: HarnessTarget },

    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to back up {path} to {backup_path}: {source}")]
    Backup {
        path: PathBuf,
        backup_path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to replace {path} with {temp_path}: {source}")]
    Replace {
        path: PathBuf,
        temp_path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to resolve current directory: {0}")]
    CurrentDir(#[source] io::Error),

    #[error("failed to run claude MCP command: {0}")]
    ClaudeCommand(#[source] io::Error),
}

/// Wire an MCP server using the process environment and filesystem.
pub fn wire(target: HarnessTarget, spec: &McpServerSpec, mode: WireMode) -> Result<WireOutcome, WireError> {
    let mut runtime = SystemWireRuntime;
    wire_with_runtime(target, spec, mode, &mut runtime)
}

/// Wire an MCP server using an injectable runtime.
pub fn wire_with_runtime(
    target: HarnessTarget,
    spec: &McpServerSpec,
    mode: WireMode,
    runtime: &mut dyn McpWireRuntime,
) -> Result<WireOutcome, WireError> {
    match (target, mode) {
        (HarnessTarget::Claude, WireMode::PrintOnly) => Ok(print_only_outcome(target, claude_json_snippet(spec)?)),
        (HarnessTarget::Codex, WireMode::PrintOnly) => Ok(print_only_outcome(target, codex_toml_snippet(spec)?)),
        (HarnessTarget::Claude, WireMode::Apply) => wire_claude(spec, runtime),
        (HarnessTarget::Codex, WireMode::Apply) => wire_codex(spec, runtime),
    }
}

/// Merge a Claude-style JSON `mcpServers` entry into an existing config.
///
/// Sibling servers and unrelated top-level fields are preserved.
pub fn merge_claude_mcp_json(existing: &str, spec: &McpServerSpec) -> Result<ConfigMergeOutcome, WireError> {
    let mut document = parse_json_document(existing)?;
    let desired = claude_server_value(spec);
    let status = claude_status_before_merge(&document, spec, &desired)?;

    if status != WireStatus::AlreadyCurrent {
        let root = document
            .as_object_mut()
            .ok_or(WireError::InvalidConfigShape("Claude MCP config root must be a JSON object"))?;
        let servers = root.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
        let servers =
            servers.as_object_mut().ok_or(WireError::InvalidConfigShape("Claude mcpServers must be a JSON object"))?;
        servers.insert(spec.name.clone(), desired);
    }

    Ok(ConfigMergeOutcome { status, body: format!("{}\n", serde_json::to_string_pretty(&document)?) })
}

/// Merge a Codex `[mcp_servers.<name>]` server entry into an existing config.
///
/// Sibling servers and unrelated top-level config are preserved by `toml_edit`.
pub fn merge_codex_mcp_toml(existing: &str, spec: &McpServerSpec) -> Result<ConfigMergeOutcome, WireError> {
    let mut document = parse_toml_document(existing)?;
    let status = codex_status_before_merge(&document, spec)?;

    if status != WireStatus::AlreadyCurrent {
        insert_codex_server(&mut document, spec)?;
    }

    Ok(ConfigMergeOutcome { status, body: document.to_string() })
}

/// Arguments passed after the `claude` binary for CLI-first Claude wiring.
pub fn claude_mcp_add_args(spec: &McpServerSpec) -> Vec<String> {
    // Verified on 2026-06-01 with live `claude mcp add --help`:
    // `claude mcp add [options] <name> <commandOrUrl> [args...]`, with `--`
    // separating stdio subprocess flags in the official help examples.
    let mut args = vec![
        "mcp".to_string(),
        "add".to_string(),
        spec.name.clone(),
        "--".to_string(),
        spec.command.to_string_lossy().into_owned(),
    ];
    args.extend(spec.args.iter().cloned());
    args
}

fn wire_claude(spec: &McpServerSpec, runtime: &mut dyn McpWireRuntime) -> Result<WireOutcome, WireError> {
    let cli_args = claude_mcp_add_args(spec);
    match runtime.claude_mcp_add(&cli_args) {
        Ok(Some(result)) if result.success => Ok(WireOutcome {
            target: HarnessTarget::Claude,
            status: WireStatus::Wired,
            message: Some("configured with `claude mcp add`".to_string()),
        }),
        Ok(Some(result)) if is_existing_claude_server(&result) => Ok(WireOutcome {
            target: HarnessTarget::Claude,
            status: WireStatus::AlreadyCurrent,
            message: Some(command_failure_reason(&result)),
        }),
        Ok(Some(result)) => wire_claude_json_fallback(spec, runtime, Some(command_failure_reason(&result))),
        Ok(None) => wire_claude_json_fallback(spec, runtime, Some("`claude` was not found on PATH".to_string())),
        Err(error) => wire_claude_json_fallback(spec, runtime, Some(error.to_string())),
    }
}

fn wire_claude_json_fallback(
    spec: &McpServerSpec,
    runtime: &mut dyn McpWireRuntime,
    cli_reason: Option<String>,
) -> Result<WireOutcome, WireError> {
    match write_claude_project_config(spec, runtime) {
        Ok(merge) => Ok(WireOutcome {
            target: HarnessTarget::Claude,
            status: merge.status,
            message: Some(fallback_message("wrote Claude project `.mcp.json` fallback", cli_reason.as_deref())),
        }),
        Err(error) => Ok(WireOutcome {
            target: HarnessTarget::Claude,
            status: WireStatus::PrintedOnly,
            message: Some(format!(
                "could not run Claude CLI or write fallback config; printed JSON instead. CLI: {}; config: {}; snippet:\n{}",
                cli_reason.as_deref().unwrap_or("not attempted"),
                error,
                claude_json_snippet(spec)?
            )),
        }),
    }
}

fn wire_codex(spec: &McpServerSpec, runtime: &mut dyn McpWireRuntime) -> Result<WireOutcome, WireError> {
    let config_path = codex_config_path(runtime)?;
    let existing = runtime.read_to_string(&config_path)?.unwrap_or_default();
    let merge = merge_codex_mcp_toml(&existing, spec)?;

    if merge.status != WireStatus::AlreadyCurrent {
        write_config(runtime, &config_path, &merge.body)?;
    }

    Ok(WireOutcome {
        target: HarnessTarget::Codex,
        status: merge.status,
        message: Some(format!("merged Codex MCP config at {}", config_path.display())),
    })
}

fn write_claude_project_config(
    spec: &McpServerSpec,
    runtime: &mut dyn McpWireRuntime,
) -> Result<ConfigMergeOutcome, WireError> {
    let config_path = runtime.current_dir()?.join(".mcp.json");
    let existing = runtime.read_to_string(&config_path)?.unwrap_or_default();
    let merge = merge_claude_mcp_json(&existing, spec)?;

    if merge.status != WireStatus::AlreadyCurrent {
        write_config(runtime, &config_path, &merge.body)?;
    }

    Ok(merge)
}

fn write_config(runtime: &mut dyn McpWireRuntime, path: &Path, body: &str) -> Result<(), WireError> {
    if let Some(parent) = path.parent() {
        runtime.create_dir_all(parent)?;
    }
    runtime.write_config_file(path, body)
}

fn print_only_outcome(target: HarnessTarget, snippet: String) -> WireOutcome {
    WireOutcome { target, status: WireStatus::PrintedOnly, message: Some(snippet) }
}

fn fallback_message(action: &str, cli_reason: Option<&str>) -> String {
    match cli_reason {
        Some(reason) => format!("{action} after Claude CLI fallback: {reason}"),
        None => action.to_string(),
    }
}

fn command_failure_reason(result: &CommandResult) -> String {
    let detail = command_output(result);
    if detail.is_empty() {
        "Claude MCP command exited unsuccessfully".to_string()
    } else {
        format!("Claude MCP command exited unsuccessfully: {detail}")
    }
}

fn command_output(result: &CommandResult) -> String {
    let stderr = result.stderr.trim();
    if stderr.is_empty() {
        result.stdout.trim().to_string()
    } else {
        stderr.to_string()
    }
}

fn is_existing_claude_server(result: &CommandResult) -> bool {
    let output = command_output(result).to_ascii_lowercase();
    output.contains("already exists") || output.contains("already configured") || output.contains("already been added")
}

fn codex_config_path(runtime: &dyn McpWireRuntime) -> Result<PathBuf, WireError> {
    if let Some(home) = runtime.env_var("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home).join("config.toml"));
    }

    runtime
        .home_dir()
        .map(|home| home.join(".codex").join("config.toml"))
        .ok_or(WireError::MissingHome { target: HarnessTarget::Codex })
}

fn parse_json_document(existing: &str) -> Result<Value, WireError> {
    if existing.trim().is_empty() {
        Ok(Value::Object(Map::new()))
    } else {
        Ok(serde_json::from_str(existing)?)
    }
}

fn claude_status_before_merge(
    document: &Value,
    spec: &McpServerSpec,
    desired: &Value,
) -> Result<WireStatus, WireError> {
    let root =
        document.as_object().ok_or(WireError::InvalidConfigShape("Claude MCP config root must be a JSON object"))?;
    let Some(servers) = root.get("mcpServers") else {
        return Ok(WireStatus::Wired);
    };
    let servers =
        servers.as_object().ok_or(WireError::InvalidConfigShape("Claude mcpServers must be a JSON object"))?;

    match servers.get(&spec.name) {
        None => Ok(WireStatus::Wired),
        Some(current) if current == desired => Ok(WireStatus::AlreadyCurrent),
        Some(_) => Ok(WireStatus::Updated),
    }
}

fn claude_server_value(spec: &McpServerSpec) -> Value {
    let mut server = Map::new();
    server.insert("command".to_string(), Value::String(spec.command.to_string_lossy().into_owned()));
    server.insert("args".to_string(), Value::Array(spec.args.iter().cloned().map(Value::String).collect()));
    Value::Object(server)
}

fn parse_toml_document(existing: &str) -> Result<DocumentMut, WireError> {
    if existing.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        Ok(existing.parse()?)
    }
}

fn codex_status_before_merge(document: &DocumentMut, spec: &McpServerSpec) -> Result<WireStatus, WireError> {
    let Some(mcp_servers) = document.get("mcp_servers") else {
        return Ok(WireStatus::Wired);
    };
    let servers =
        mcp_servers.as_table_like().ok_or(WireError::InvalidConfigShape("Codex mcp_servers must be a TOML table"))?;

    Ok(match servers.get(&spec.name) {
        None => WireStatus::Wired,
        Some(current) if codex_server_matches(current, spec) => WireStatus::AlreadyCurrent,
        Some(_) => WireStatus::Updated,
    })
}

fn codex_server_matches(item: &Item, spec: &McpServerSpec) -> bool {
    let Some(table) = item.as_table_like() else {
        return false;
    };
    let command = table.get("command").and_then(Item::as_str);
    let args = table
        .get("args")
        .and_then(Item::as_array)
        .and_then(|array| array.iter().map(|value| value.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>());

    command == Some(spec.command.to_string_lossy().as_ref()) && args.as_deref() == Some(spec.args.as_slice())
}

fn codex_server_table(spec: &McpServerSpec) -> Table {
    let mut table = Table::new();
    table["command"] = value(spec.command.to_string_lossy().as_ref());
    table["args"] = value(toml_args_array(&spec.args));
    table
}

fn insert_codex_server(document: &mut DocumentMut, spec: &McpServerSpec) -> Result<(), WireError> {
    if document.get("mcp_servers").is_none() {
        document["mcp_servers"] = Item::Table(Table::new());
    }

    let servers = document["mcp_servers"]
        .as_table_like_mut()
        .ok_or(WireError::InvalidConfigShape("Codex mcp_servers must be a TOML table"))?;
    servers.insert(&spec.name, Item::Table(codex_server_table(spec)));
    Ok(())
}

fn toml_args_array(args: &[String]) -> Array {
    let mut array = Array::new();
    for arg in args {
        array.push(arg.as_str());
    }
    array
}

fn claude_json_snippet(spec: &McpServerSpec) -> Result<String, WireError> {
    Ok(merge_claude_mcp_json("", spec)?.body)
}

fn codex_toml_snippet(spec: &McpServerSpec) -> Result<String, WireError> {
    Ok(merge_codex_mcp_toml("", spec)?.body)
}

#[derive(Debug, Default)]
struct SystemWireRuntime;

impl McpWireRuntime for SystemWireRuntime {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>, WireError> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(WireError::Read { path: path.to_path_buf(), source }),
        }
    }

    fn write_config_file(&mut self, path: &Path, contents: &str) -> Result<(), WireError> {
        write_config_file_safely(path, contents)
    }

    fn create_dir_all(&mut self, path: &Path) -> Result<(), WireError> {
        std::fs::create_dir_all(path).map_err(|source| WireError::CreateDir { path: path.to_path_buf(), source })
    }

    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }

    fn current_dir(&self) -> Result<PathBuf, WireError> {
        std::env::current_dir().map_err(WireError::CurrentDir)
    }

    fn claude_mcp_add(&mut self, args: &[String]) -> Result<Option<CommandResult>, WireError> {
        let Ok(claude) = which::which("claude") else {
            return Ok(None);
        };
        let output = Command::new(claude).args(args).output().map_err(WireError::ClaudeCommand)?;
        Ok(Some(CommandResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }))
    }
}

fn write_config_file_safely(path: &Path, contents: &str) -> Result<(), WireError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if path.exists() {
        let backup_path = sibling_with_unique_suffix(path, "bak");
        std::fs::copy(path, &backup_path).map_err(|source| WireError::Backup {
            path: path.to_path_buf(),
            backup_path: backup_path.clone(),
            source,
        })?;
    }

    let temp_path = sibling_with_unique_suffix(path, "tmp");
    let write_result = (|| -> Result<(), WireError> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|source| WireError::Write { path: temp_path.clone(), source })?;
        file.write_all(contents.as_bytes()).map_err(|source| WireError::Write { path: temp_path.clone(), source })?;
        file.sync_all().map_err(|source| WireError::Write { path: temp_path.clone(), source })?;
        drop(file);

        std::fs::rename(&temp_path, path).map_err(|source| WireError::Replace {
            path: path.to_path_buf(),
            temp_path: temp_path.clone(),
            source,
        })?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    write_result?;

    let _ = std::fs::File::open(parent).and_then(|dir| dir.sync_all());
    Ok(())
}

fn sibling_with_unique_suffix(path: &Path, kind: &str) -> PathBuf {
    let unique = ulid::Ulid::new();
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("config");
    path.with_file_name(format!("{file_name}.{kind}-{unique}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_writer_replaces_config_and_preserves_backup() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "old = true\n").expect("write old config");

        write_config_file_safely(&config_path, "new = true\n").expect("safe write");

        assert_eq!(std::fs::read_to_string(&config_path).expect("read new config"), "new = true\n");
        let backups = std::fs::read_dir(temp.path())
            .expect("list temp dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("config.toml.bak-"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1, "expected one backup file");
        assert_eq!(std::fs::read_to_string(backups[0].path()).expect("read backup"), "old = true\n");
    }
}
