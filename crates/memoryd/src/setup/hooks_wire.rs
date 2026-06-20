//! Passive-recall hook wiring for setup.
//!
//! The reverse-companion of [`mcp_wire`](super::mcp_wire): where MCP wiring
//! registers the `memorum` server, this installs the `memoryd recall hook`
//! lifecycle hooks that inject recall blocks at `SessionStart`,
//! `UserPromptSubmit`, and `SubagentStart`. Pure merge helpers operate on config
//! text so mutation is testable without touching a developer's real Claude or
//! Codex state, and the public `wire_hooks` entrypoint shares the atomic
//! backup+temp-rename writer with MCP wiring.
//!
//! Two harness-specific concerns shape the merge:
//!
//! 1. **Claude hooks are arrays.** Each event holds an array of matcher groups,
//!    so we find-or-update the Memorum entry by a stable marker (the `recall
//!    hook` subcommand plus the matching `--harness`) rather than blind-appending
//!    — re-running setup, or upgrading the binary, refreshes the command in place
//!    instead of accumulating duplicates. Hooks live in `settings.json`
//!    (`CLAUDE_CONFIG_DIR`-aware), distinct from the `.claude.json` MCP file.
//! 2. **Codex trust is byte-keyed.** Codex records a `trusted_hash` over the exact
//!    bytes of the hook config; any rewrite silently invalidates it and forces
//!    the user to re-trust via `/hooks`. So a byte-identical re-merge must skip
//!    the write entirely and report [`HookWireStatus::AlreadyCurrent`].

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use toml_edit::{value, Item, Table};

use super::mcp_wire::{
    parse_json_document, parse_toml_document, write_config_file_safely, HarnessTarget, WireError, WireMode,
};

/// The stable marker substring identifying a Memorum recall hook command.
///
/// Find-or-update and removal key off this rather than the absolute binary path,
/// which changes whenever the `memoryd` binary is upgraded or relocated.
pub const RECALL_HOOK_MARKER: &str = "recall hook";

/// Backstop timeout (seconds) installed on each hook so a stalled `recall hook`
/// invocation never blocks the harness. The handler enforces a tighter internal
/// deadline; this is the outer guard the harness honors.
const HOOK_TIMEOUT_SECS: u64 = 2;

/// Verbatim trust guidance surfaced for Codex hooks, which stay inactive until
/// the user trusts them. The wording is shared by the step message and the
/// trust-aware verify output so both read identically.
pub const CODEX_HOOK_TRUST_NOTICE: &str =
    "Codex hooks configured but inactive until trusted — open Codex and run `/hooks`, then trust the Memorum hook.";

/// Desired passive-recall hook command for one harness.
///
/// `exe` is the absolute `memoryd` binary (resolved via `current_exe()` at the
/// call site, never a PATH lookup), `socket` the daemon socket, and `harness`
/// the canonical id passed to `--harness`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpec {
    pub exe: PathBuf,
    pub socket: PathBuf,
    pub harness: HarnessTarget,
}

impl HookSpec {
    pub fn new(exe: impl Into<PathBuf>, socket: impl Into<PathBuf>, harness: HarnessTarget) -> Self {
        Self { exe: exe.into(), socket: socket.into(), harness }
    }

    /// The shell command string the hook runs:
    /// `"<exe>" recall hook --socket "<socket>" --harness <id>`.
    ///
    /// The exe path and socket are quoted so a path containing spaces survives
    /// the shell parse Claude performs on the `command` field. The `--harness`
    /// id is a fixed canonical token (no spaces) and is left unquoted.
    pub fn command_string(&self) -> String {
        format!(
            "\"{}\" recall hook --socket \"{}\" --harness {}",
            self.exe.to_string_lossy(),
            self.socket.to_string_lossy(),
            self.harness.descriptor_id(),
        )
    }

    /// The `--harness <id>` fragment used as the second half of the stable
    /// marker, so two harnesses' hooks in the same array never collide.
    fn harness_marker(&self) -> String {
        format!("--harness {}", self.harness.descriptor_id())
    }
}

/// Status values produced by hook wiring. Mirrors
/// [`WireStatus`](super::mcp_wire::WireStatus) but is hook-specific so the two
/// surfaces evolve independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookWireStatus {
    /// No prior Memorum hook entry existed; one was inserted.
    Wired,
    /// A prior Memorum hook entry was found and refreshed in place.
    Updated,
    /// The desired config equals the current config byte-for-byte; no write.
    AlreadyCurrent,
    /// Hook wiring was not requested for this harness.
    Skipped,
}

/// Hook wiring outcome for one harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookWireOutcome {
    pub target: HarnessTarget,
    pub status: HookWireStatus,
    pub message: Option<String>,
}

/// In-memory hook-config merge result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookMergeOutcome {
    pub status: HookWireStatus,
    pub body: String,
}

/// Runtime boundary for filesystem and environment interactions during hook
/// wiring. Mirrors the MCP wiring seam so tests stub config state without
/// touching a developer's real Claude/Codex config.
pub trait HookWireRuntime {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>, WireError>;
    fn write_config_file(&mut self, path: &Path, contents: &str) -> Result<(), WireError>;
    fn create_dir_all(&mut self, path: &Path) -> Result<(), WireError>;
    fn env_var(&self, key: &str) -> Option<String>;
    fn home_dir(&self) -> Option<PathBuf>;
}

/// Wire passive-recall hooks using the process environment and filesystem.
pub fn wire_hooks(spec: &HookSpec, mode: WireMode) -> Result<HookWireOutcome, WireError> {
    let mut runtime = SystemHookWireRuntime;
    wire_hooks_with_runtime(spec, mode, &mut runtime)
}

/// Wire passive-recall hooks using an injectable runtime.
pub fn wire_hooks_with_runtime(
    spec: &HookSpec,
    mode: WireMode,
    runtime: &mut dyn HookWireRuntime,
) -> Result<HookWireOutcome, WireError> {
    match (spec.harness, mode) {
        (HarnessTarget::Claude, WireMode::PrintOnly) => {
            Ok(print_only_outcome(spec.harness, merge_claude_hooks_json("", spec)?.body))
        }
        (HarnessTarget::Codex, WireMode::PrintOnly) => {
            Ok(print_only_outcome(spec.harness, merge_codex_hooks_toml("", spec)?.body))
        }
        (HarnessTarget::Claude, WireMode::Apply) => wire_claude_hooks(spec, runtime),
        (HarnessTarget::Codex, WireMode::Apply) => wire_codex_hooks(spec, runtime),
    }
}

/// Resolve the Claude settings.json path: `$CLAUDE_CONFIG_DIR/settings.json`
/// else `~/.claude/settings.json`.
///
/// This is the hooks file, distinct from the `.claude.json` MCP file that
/// [`mcp_wire::claude_config_path`](super::mcp_wire) resolves. No prior resolver
/// honored `CLAUDE_CONFIG_DIR` for settings.json, so this is the canonical one.
pub fn claude_settings_path(env_config_dir: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = env_config_dir.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(dir).join("settings.json"));
    }
    home.map(|home| home.join(".claude").join("settings.json"))
}

/// Resolve the Codex inline-hooks config path: `$CODEX_HOME/config.toml` else
/// `~/.codex/config.toml`. Mirrors `mcp_wire::codex_config_path`.
pub fn codex_config_path(env_codex_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(home_dir) = env_codex_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(home_dir).join("config.toml"));
    }
    home.map(|home| home.join(".codex").join("config.toml"))
}

/// Resolve the Codex standalone hooks file: `$CODEX_HOME/hooks.json` else
/// `~/.codex/hooks.json`. Detected first so an existing `hooks.json` wins over
/// inline `[hooks]`, avoiding Codex's dual-representation warning.
pub fn codex_hooks_path(env_codex_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(home_dir) = env_codex_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(home_dir).join("hooks.json"));
    }
    home.map(|home| home.join(".codex").join("hooks.json"))
}

/// Merge the Memorum recall hooks into a Claude `settings.json` body.
///
/// Each event's value is an array of matcher groups. We find the Memorum entry
/// by the stable `recall hook` + `--harness <id>` marker and update it in place,
/// or insert a fresh entry when absent. Sibling (non-Memorum) hooks are left
/// untouched. A byte-identical re-merge reports [`HookWireStatus::AlreadyCurrent`]
/// and the caller skips the rewrite.
pub fn merge_claude_hooks_json(existing: &str, spec: &HookSpec) -> Result<HookMergeOutcome, WireError> {
    merge_hooks_json_under_hooks_object(existing, spec, &claude_event_matchers())
}

/// Merge recall hooks into a JSON config whose lifecycle events live under a
/// top-level `"hooks"` object. Shared by Claude `settings.json` and Codex
/// `hooks.json`, which carry the identical event/group shape (events nested
/// under `hooks`; each group is `{ [matcher,] hooks: [...] }`).
fn merge_hooks_json_under_hooks_object(
    existing: &str,
    spec: &HookSpec,
    events: &[(&str, Option<&str>)],
) -> Result<HookMergeOutcome, WireError> {
    let mut document = parse_json_document(existing)?;
    let root =
        document.as_object_mut().ok_or(WireError::InvalidConfigShape("hooks config root must be a JSON object"))?;

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(WireError::InvalidConfigShape("hooks must be a JSON object"))?;

    let command = spec.command_string();
    let marker = spec.harness_marker();

    let mut changed = false;
    for (event, matcher) in events {
        changed |= upsert_claude_event_hook(hooks, event, *matcher, &command, &marker)?;
    }

    let body = format!("{}\n", serde_json::to_string_pretty(&document)?);
    let status = json_like_status(existing, &body, changed);
    Ok(HookMergeOutcome { status, body })
}

/// Merge the Memorum recall hooks into a Codex inline `[hooks]` table.
///
/// Codex's hooks mirror Claude's event/command shape. Sibling hooks and
/// unrelated top-level config are preserved by `toml_edit`. A byte-identical
/// re-merge reports [`HookWireStatus::AlreadyCurrent`] so the caller skips the
/// rewrite and Codex's `trusted_hash` survives.
pub fn merge_codex_hooks_toml(existing: &str, spec: &HookSpec) -> Result<HookMergeOutcome, WireError> {
    let mut document = parse_toml_document(existing)?;

    if document.get("hooks").is_none() {
        document["hooks"] = Item::Table(Table::new());
    }
    let hooks =
        document["hooks"].as_table_mut().ok_or(WireError::InvalidConfigShape("Codex hooks must be a TOML table"))?;
    // Inline event arrays read more naturally without the implicit-parent dotted
    // header `toml_edit` would otherwise emit for `[hooks]`.
    hooks.set_implicit(false);

    let command = spec.command_string();
    let marker = spec.harness_marker();

    let mut changed = false;
    for (event, matcher) in codex_event_matchers() {
        changed |= upsert_codex_event_hook(hooks, event, matcher, &command, &marker)?;
    }

    let body = document.to_string();
    let status = json_like_status(existing, &body, changed);
    Ok(HookMergeOutcome { status, body })
}

/// Merge the Memorum recall hooks into a Codex standalone `hooks.json` body.
///
/// Used when an existing `~/.codex/hooks.json` is detected — Codex warns when
/// both a `hooks.json` and inline `[hooks]` are present, so we keep editing the
/// file the user already has. Codex `hooks.json` nests events under a top-level
/// `"hooks"` object exactly like Claude `settings.json`, so it shares the same
/// merge path (only the matcher set differs — Codex `SessionStart` is
/// matcher-free). Same find-or-update + byte-identical-skip discipline.
pub fn merge_codex_hooks_json(existing: &str, spec: &HookSpec) -> Result<HookMergeOutcome, WireError> {
    merge_hooks_json_under_hooks_object(existing, spec, &codex_event_matchers())
}

/// The lifecycle events wired for Claude, with the matcher for each. Only
/// `SessionStart` carries a matcher (`startup|resume|clear|compact`); the other
/// two events take no matcher and inject on every fire.
fn claude_event_matchers() -> [(&'static str, Option<&'static str>); 3] {
    [("SessionStart", Some("startup|resume|clear|compact")), ("UserPromptSubmit", None), ("SubagentStart", None)]
}

/// The lifecycle events wired for Codex. Codex mirrors Claude's events but does
/// not carry a `source`-style matcher on `SessionStart`, so all three are
/// matcher-free.
fn codex_event_matchers() -> [(&'static str, Option<&'static str>); 3] {
    [("SessionStart", None), ("UserPromptSubmit", None), ("SubagentStart", None)]
}

/// Find-or-update the Memorum hook in one Claude event array. Returns `true`
/// when the array was inserted into or mutated, `false` when the desired entry
/// was already present unchanged.
#[allow(clippy::too_many_arguments)]
fn upsert_claude_event_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    matcher: Option<&str>,
    command: &str,
    marker: &str,
) -> Result<bool, WireError> {
    let array = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or(WireError::InvalidConfigShape("Claude hook event must be a JSON array"))?;

    let desired = claude_matcher_group(matcher, command);

    if let Some(group) = array.iter_mut().find(|group| group_has_memorum_command(group, marker)) {
        if *group == desired {
            return Ok(false);
        }
        *group = desired;
        return Ok(true);
    }

    array.push(desired);
    Ok(true)
}

/// Build a Claude matcher group: `{ ["matcher": <m>,] "hooks": [ { "type":
/// "command", "command": <cmd>, "timeout": 2 } ] }`.
fn claude_matcher_group(matcher: Option<&str>, command: &str) -> Value {
    let mut group = Map::new();
    if let Some(matcher) = matcher {
        group.insert("matcher".to_string(), Value::String(matcher.to_string()));
    }
    group.insert("hooks".to_string(), Value::Array(vec![command_hook_value(command)]));
    Value::Object(group)
}

/// One command-hook handler object shared by the Claude and Codex JSON shapes.
fn command_hook_value(command: &str) -> Value {
    let mut hook = Map::new();
    hook.insert("type".to_string(), Value::String("command".to_string()));
    hook.insert("command".to_string(), Value::String(command.to_string()));
    hook.insert("timeout".to_string(), Value::Number(HOOK_TIMEOUT_SECS.into()));
    Value::Object(hook)
}

/// Whether a matcher group contains a Memorum hook command for this harness:
/// any inner `hooks[].command` carrying both the `recall hook` marker and the
/// matching `--harness <id>` fragment.
fn group_has_memorum_command(group: &Value, marker: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(|hook| hook_command_is_memorum(hook, marker)))
}

/// Whether a single hook handler's `command` is a Memorum recall hook for this
/// harness.
fn hook_command_is_memorum(hook: &Value, marker: &str) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.contains(RECALL_HOOK_MARKER) && command.contains(marker))
}

/// Find-or-update the Memorum hook in one Codex inline event array.
#[allow(clippy::too_many_arguments)]
fn upsert_codex_event_hook(
    hooks: &mut Table,
    event: &str,
    matcher: Option<&str>,
    command: &str,
    marker: &str,
) -> Result<bool, WireError> {
    if hooks.get(event).is_none() {
        hooks.insert(event, Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
    }
    let array = hooks
        .get_mut(event)
        .and_then(Item::as_array_of_tables_mut)
        .ok_or(WireError::InvalidConfigShape("Codex hook event must be a TOML array of tables"))?;

    let desired = codex_matcher_group(matcher, command);

    if let Some(group) = array.iter_mut().find(|group| codex_group_has_memorum_command(group, marker)) {
        if group.to_string() == desired.to_string() {
            return Ok(false);
        }
        *group = desired;
        return Ok(true);
    }

    array.push(desired);
    Ok(true)
}

/// Build a Codex matcher group table mirroring the Claude JSON shape.
fn codex_matcher_group(matcher: Option<&str>, command: &str) -> Table {
    let mut group = Table::new();
    if let Some(matcher) = matcher {
        group["matcher"] = value(matcher);
    }
    let mut handler = Table::new();
    handler["type"] = value("command");
    handler["command"] = value(command);
    handler["timeout"] = value(HOOK_TIMEOUT_SECS as i64);
    let mut handlers = toml_edit::ArrayOfTables::new();
    handlers.push(handler);
    group.insert("hooks", Item::ArrayOfTables(handlers));
    group
}

/// Whether a Codex inline hook group carries a Memorum recall command for this
/// harness.
fn codex_group_has_memorum_command(group: &Table, marker: &str) -> bool {
    group.get("hooks").and_then(Item::as_array_of_tables).is_some_and(|handlers| {
        handlers.iter().any(|handler| {
            handler
                .get("command")
                .and_then(Item::as_str)
                .is_some_and(|command| command.contains(RECALL_HOOK_MARKER) && command.contains(marker))
        })
    })
}

fn wire_claude_hooks(spec: &HookSpec, runtime: &mut dyn HookWireRuntime) -> Result<HookWireOutcome, WireError> {
    let config_path = claude_settings_path_for(runtime)?;
    let existing = runtime.read_to_string(&config_path)?.unwrap_or_default();
    let merge = merge_claude_hooks_json(&existing, spec)?;

    if merge.status != HookWireStatus::AlreadyCurrent {
        write_config(runtime, &config_path, &merge.body)?;
    }

    Ok(HookWireOutcome {
        target: HarnessTarget::Claude,
        status: merge.status,
        message: Some(format!("merged Claude recall hooks at {}", config_path.display())),
    })
}

/// Wire Codex hooks: edit an existing `hooks.json` if one is present, else merge
/// inline `[hooks]` into `config.toml`. Both paths skip the write on a
/// byte-identical re-merge so Codex's `trusted_hash` survives, and both surface
/// the trust notice.
fn wire_codex_hooks(spec: &HookSpec, runtime: &mut dyn HookWireRuntime) -> Result<HookWireOutcome, WireError> {
    let hooks_json_path = codex_hooks_path_for(runtime)?;
    if let Some(existing) = runtime.read_to_string(&hooks_json_path)? {
        let merge = merge_codex_hooks_json(&existing, spec)?;
        if merge.status != HookWireStatus::AlreadyCurrent {
            write_config(runtime, &hooks_json_path, &merge.body)?;
        }
        return Ok(codex_hook_outcome(merge.status, &hooks_json_path));
    }

    let config_path = codex_config_path_for(runtime)?;
    let existing = runtime.read_to_string(&config_path)?.unwrap_or_default();
    let merge = merge_codex_hooks_toml(&existing, spec)?;
    if merge.status != HookWireStatus::AlreadyCurrent {
        write_config(runtime, &config_path, &merge.body)?;
    }
    Ok(codex_hook_outcome(merge.status, &config_path))
}

fn codex_hook_outcome(status: HookWireStatus, path: &Path) -> HookWireOutcome {
    HookWireOutcome {
        target: HarnessTarget::Codex,
        status,
        message: Some(format!("merged Codex recall hooks at {}. {CODEX_HOOK_TRUST_NOTICE}", path.display())),
    }
}

fn claude_settings_path_for(runtime: &dyn HookWireRuntime) -> Result<PathBuf, WireError> {
    claude_settings_path(runtime.env_var("CLAUDE_CONFIG_DIR").as_deref(), runtime.home_dir().as_deref())
        .ok_or(WireError::MissingHome { target: HarnessTarget::Claude })
}

fn codex_config_path_for(runtime: &dyn HookWireRuntime) -> Result<PathBuf, WireError> {
    codex_config_path(runtime.env_var("CODEX_HOME").as_deref(), runtime.home_dir().as_deref())
        .ok_or(WireError::MissingHome { target: HarnessTarget::Codex })
}

fn codex_hooks_path_for(runtime: &dyn HookWireRuntime) -> Result<PathBuf, WireError> {
    codex_hooks_path(runtime.env_var("CODEX_HOME").as_deref(), runtime.home_dir().as_deref())
        .ok_or(WireError::MissingHome { target: HarnessTarget::Codex })
}

/// Status decision shared by every hook merge: a byte-identical body is
/// `AlreadyCurrent` (skip the write — Codex's trust hash survives); otherwise the
/// change is `Wired` when the source was empty/absent and `Updated` when it
/// edited an existing config.
fn json_like_status(existing: &str, body: &str, changed: bool) -> HookWireStatus {
    if !changed && existing == body {
        return HookWireStatus::AlreadyCurrent;
    }
    if existing.trim().is_empty() {
        HookWireStatus::Wired
    } else {
        HookWireStatus::Updated
    }
}

fn print_only_outcome(target: HarnessTarget, snippet: String) -> HookWireOutcome {
    HookWireOutcome { target, status: HookWireStatus::Skipped, message: Some(snippet) }
}

fn write_config(runtime: &mut dyn HookWireRuntime, path: &Path, body: &str) -> Result<(), WireError> {
    if let Some(parent) = path.parent() {
        runtime.create_dir_all(parent)?;
    }
    runtime.write_config_file(path, body)
}

#[derive(Debug, Default)]
struct SystemHookWireRuntime;

impl HookWireRuntime for SystemHookWireRuntime {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>, WireError> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
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
        std::env::var(key).ok().filter(|value| !value.is_empty())
    }

    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use toml_edit::DocumentMut;

    use super::*;

    fn claude_spec() -> HookSpec {
        HookSpec::new("/opt/memorum/bin/memoryd", "/tmp/memoryd.sock", HarnessTarget::Claude)
    }

    fn codex_spec() -> HookSpec {
        HookSpec::new("/opt/memorum/bin/memoryd", "/tmp/memoryd.sock", HarnessTarget::Codex)
    }

    fn spec_with_spaces() -> HookSpec {
        HookSpec::new("/Applications/My App/memoryd", "/tmp/space dir/memoryd.sock", HarnessTarget::Claude)
    }

    #[test]
    fn command_string_quotes_exe_and_socket_and_marks_harness() {
        let command = spec_with_spaces().command_string();
        assert_eq!(
            command,
            "\"/Applications/My App/memoryd\" recall hook --socket \"/tmp/space dir/memoryd.sock\" --harness claude-code"
        );
        assert!(command.contains(RECALL_HOOK_MARKER));
        assert!(command.contains("--harness claude-code"));
    }

    #[test]
    fn claude_merge_writes_all_three_events_with_correct_marker() {
        let outcome = merge_claude_hooks_json("", &claude_spec()).expect("merge");
        assert_eq!(outcome.status, HookWireStatus::Wired);
        let parsed: Value = serde_json::from_str(&outcome.body).expect("json");
        let hooks = &parsed["hooks"];

        // SessionStart carries the source matcher; the other two do not.
        assert_eq!(hooks["SessionStart"][0]["matcher"], "startup|resume|clear|compact");
        assert!(hooks["UserPromptSubmit"][0].get("matcher").is_none());
        assert!(hooks["SubagentStart"][0].get("matcher").is_none());

        let command = hooks["SessionStart"][0]["hooks"][0]["command"].as_str().expect("command");
        assert!(command.contains("recall hook"));
        assert!(command.contains("--harness claude-code"));
        assert!(command.starts_with("\"/opt/memorum/bin/memoryd\""));
        assert_eq!(hooks["SessionStart"][0]["hooks"][0]["timeout"], 2);
        assert_eq!(hooks["SessionStart"][0]["hooks"][0]["type"], "command");
    }

    #[test]
    fn claude_remerge_is_already_current_and_byte_identical() {
        let first = merge_claude_hooks_json("", &claude_spec()).expect("first merge");
        let second = merge_claude_hooks_json(&first.body, &claude_spec()).expect("remerge");
        assert_eq!(second.status, HookWireStatus::AlreadyCurrent);
        assert_eq!(second.body, first.body);
    }

    #[test]
    fn claude_marker_find_or_update_does_not_duplicate() {
        let first = merge_claude_hooks_json("", &claude_spec()).expect("first merge");
        // Re-run with an upgraded exe path: the marker matches, so the entry is
        // refreshed in place rather than duplicated.
        let upgraded = HookSpec::new("/opt/memorum/v2/memoryd", "/tmp/memoryd.sock", HarnessTarget::Claude);
        let second = merge_claude_hooks_json(&first.body, &upgraded).expect("remerge upgraded");
        assert_eq!(second.status, HookWireStatus::Updated);

        let parsed: Value = serde_json::from_str(&second.body).expect("json");
        let session_start = parsed["hooks"]["SessionStart"].as_array().expect("array");
        assert_eq!(session_start.len(), 1, "no duplicate entry");
        let command = session_start[0]["hooks"][0]["command"].as_str().expect("command");
        assert!(command.contains("/opt/memorum/v2/memoryd"), "exe refreshed in place");
    }

    #[test]
    fn claude_merge_preserves_sibling_hooks() {
        let existing = r#"{
          "hooks": {
            "SessionStart": [
              { "hooks": [ { "type": "command", "command": "echo sibling" } ] }
            ],
            "Stop": [
              { "hooks": [ { "type": "command", "command": "echo stop" } ] }
            ]
          }
        }"#;
        let outcome = merge_claude_hooks_json(existing, &claude_spec()).expect("merge");
        let parsed: Value = serde_json::from_str(&outcome.body).expect("json");

        let session_start = parsed["hooks"]["SessionStart"].as_array().expect("array");
        assert_eq!(session_start.len(), 2, "sibling kept, memorum appended");
        assert!(session_start.iter().any(|group| { group["hooks"][0]["command"].as_str() == Some("echo sibling") }));
        // Unrelated event preserved untouched.
        assert_eq!(parsed["hooks"]["Stop"][0]["hooks"][0]["command"], "echo stop");
    }

    #[test]
    fn claude_two_harness_markers_coexist() {
        let claude = merge_claude_hooks_json("", &claude_spec()).expect("claude merge");
        let both = merge_claude_hooks_json(&claude.body, &codex_spec()).expect("codex merge into same file");
        let parsed: Value = serde_json::from_str(&both.body).expect("json");
        let session_start = parsed["hooks"]["SessionStart"].as_array().expect("array");
        // Distinct --harness markers must not collide: two entries, not one.
        assert_eq!(session_start.len(), 2);
        assert!(session_start
            .iter()
            .any(|g| g["hooks"][0]["command"].as_str().unwrap().contains("--harness claude-code")));
        assert!(session_start.iter().any(|g| g["hooks"][0]["command"].as_str().unwrap().contains("--harness codex")));
    }

    #[test]
    fn codex_inline_remerge_does_not_rewrite() {
        let first = merge_codex_hooks_toml("", &codex_spec()).expect("first merge");
        assert_eq!(first.status, HookWireStatus::Wired);
        let second = merge_codex_hooks_toml(&first.body, &codex_spec()).expect("remerge");
        assert_eq!(second.status, HookWireStatus::AlreadyCurrent, "byte-identical re-merge must not rewrite");
        assert_eq!(second.body, first.body);
    }

    #[test]
    fn codex_inline_preserves_unrelated_config() {
        let existing = "model = \"gpt\"\n";
        let outcome = merge_codex_hooks_toml(existing, &codex_spec()).expect("merge");
        let document: DocumentMut = outcome.body.parse().expect("toml");
        assert_eq!(document.get("model").and_then(Item::as_str), Some("gpt"));
        assert!(document.get("hooks").is_some());
    }

    #[test]
    fn codex_hooks_json_remerge_does_not_rewrite() {
        let first = merge_codex_hooks_json("", &codex_spec()).expect("first merge");
        let second = merge_codex_hooks_json(&first.body, &codex_spec()).expect("remerge");
        assert_eq!(second.status, HookWireStatus::AlreadyCurrent);
        assert_eq!(second.body, first.body);
    }

    #[test]
    fn claude_settings_path_prefers_env_over_home() {
        let path = claude_settings_path(Some("/cfg"), Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/cfg/settings.json"));
        let path = claude_settings_path(None, Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/home/u/.claude/settings.json"));
        let path = claude_settings_path(Some(""), Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/home/u/.claude/settings.json"));
    }

    #[test]
    fn codex_hooks_json_path_prefers_env_over_home() {
        let path = codex_hooks_path(Some("/codex"), Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/codex/hooks.json"));
        let path = codex_hooks_path(None, Some(Path::new("/home/u"))).expect("path");
        assert_eq!(path, PathBuf::from("/home/u/.codex/hooks.json"));
    }

    #[test]
    fn apply_writes_settings_json_not_claude_json_under_config_dir() {
        let mut runtime = FakeHookRuntime::default().with_env("CLAUDE_CONFIG_DIR", "/custom/claude");
        let outcome = wire_hooks_with_runtime(&claude_spec(), WireMode::Apply, &mut runtime).expect("wire");
        assert_eq!(outcome.status, HookWireStatus::Wired);
        assert!(runtime.files.contains_key(Path::new("/custom/claude/settings.json")), "writes settings.json");
        assert!(!runtime.files.contains_key(Path::new("/custom/claude/.claude.json")), "not the MCP file");
    }

    #[test]
    fn apply_claude_command_is_quoted_absolute_with_marker() {
        let mut runtime = FakeHookRuntime::default().with_home(PathBuf::from("/home/tester"));
        wire_hooks_with_runtime(&claude_spec(), WireMode::Apply, &mut runtime).expect("wire");
        let config = runtime.files.get(Path::new("/home/tester/.claude/settings.json")).expect("settings written");
        let parsed: Value = serde_json::from_str(config).expect("json");
        let command = parsed["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"].as_str().expect("command");
        assert!(command.starts_with('"'), "exe path is quoted");
        assert!(command.contains("recall hook"));
        assert!(command.contains("--harness claude-code"));
    }

    #[test]
    fn apply_codex_prefers_existing_hooks_json() {
        let mut runtime = FakeHookRuntime::default().with_home(PathBuf::from("/home/tester"));
        runtime.files.insert(PathBuf::from("/home/tester/.codex/hooks.json"), "{}".to_string());
        let outcome = wire_hooks_with_runtime(&codex_spec(), WireMode::Apply, &mut runtime).expect("wire");
        assert_eq!(outcome.status, HookWireStatus::Updated);
        assert!(runtime.files.contains_key(Path::new("/home/tester/.codex/hooks.json")), "edits hooks.json");
        assert!(
            !runtime.files.contains_key(Path::new("/home/tester/.codex/config.toml")),
            "does not also touch config.toml when hooks.json exists"
        );
        assert!(
            outcome.message.as_deref().is_some_and(|m| m.contains(CODEX_HOOK_TRUST_NOTICE)),
            "trust notice surfaced"
        );
    }

    #[test]
    fn apply_codex_falls_back_to_inline_config_toml() {
        let mut runtime = FakeHookRuntime::default().with_home(PathBuf::from("/home/tester"));
        let outcome = wire_hooks_with_runtime(&codex_spec(), WireMode::Apply, &mut runtime).expect("wire");
        assert_eq!(outcome.status, HookWireStatus::Wired);
        assert!(
            runtime.files.contains_key(Path::new("/home/tester/.codex/config.toml")),
            "inline [hooks] in config.toml"
        );
        assert!(!runtime.files.contains_key(Path::new("/home/tester/.codex/hooks.json")), "no hooks.json created");
    }

    #[test]
    fn apply_already_current_does_not_rewrite() {
        let mut runtime = FakeHookRuntime::default().with_home(PathBuf::from("/home/tester"));
        wire_hooks_with_runtime(&claude_spec(), WireMode::Apply, &mut runtime).expect("first wire");
        let written = runtime.files.get(Path::new("/home/tester/.claude/settings.json")).cloned().expect("written");
        runtime.write_count = 0;
        let outcome = wire_hooks_with_runtime(&claude_spec(), WireMode::Apply, &mut runtime).expect("second wire");
        assert_eq!(outcome.status, HookWireStatus::AlreadyCurrent);
        assert_eq!(runtime.write_count, 0, "already-current must not rewrite");
        assert_eq!(runtime.files.get(Path::new("/home/tester/.claude/settings.json")), Some(&written));
    }

    #[derive(Default)]
    struct FakeHookRuntime {
        files: BTreeMap<PathBuf, String>,
        env: HashMap<String, String>,
        home: Option<PathBuf>,
        write_count: usize,
    }

    impl FakeHookRuntime {
        fn with_env(mut self, key: &str, value: &str) -> Self {
            self.env.insert(key.to_string(), value.to_string());
            self
        }

        fn with_home(mut self, home: PathBuf) -> Self {
            self.home = Some(home);
            self
        }
    }

    impl HookWireRuntime for FakeHookRuntime {
        fn read_to_string(&self, path: &Path) -> Result<Option<String>, WireError> {
            Ok(self.files.get(path).cloned())
        }

        fn write_config_file(&mut self, path: &Path, contents: &str) -> Result<(), WireError> {
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
    }
}
