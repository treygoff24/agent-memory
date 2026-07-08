//! `memoryd schema` — the machine-readable CLI agent contract.
//!
//! Generated from the implementing types, not a hand-maintained blob: the
//! per-command argument shapes are introspected from the clap definitions
//! (`Cli::command()`), the exit crosswalk is read from `cli::exit`, and the
//! envelope shape mirrors `cli::output`. The only hand-authored data is what
//! clap cannot express — each covered command's side-effect class and the set of
//! exit codes it can produce. Kept in agreement with
//! `docs/api/memoryd-cli-contract-v1.md` by `tests/cli_schema.rs`.

use clap::{ArgAction, Command as ClapCommand, CommandFactory};
use serde_json::{json, Value};

use crate::cli::exit::agent_exit_crosswalk;
use crate::cli::output::SCHEMA_VERSION;
use crate::cli::{Cli, SchemaArgs, SchemaSection};

/// Metadata clap cannot express: the side-effect class and the exit codes each
/// covered command can produce. The `path` walks nested subcommands to the leaf.
struct CommandMeta {
    path: &'static [&'static str],
    side_effect: &'static str,
    exit_codes: &'static [i32],
}

const COVERED: &[CommandMeta] = &[
    CommandMeta { path: &["search"], side_effect: "read_only", exit_codes: &[0, 2, 75] },
    CommandMeta { path: &["get"], side_effect: "read_only", exit_codes: &[0, 2, 65, 66, 75] },
    CommandMeta { path: &["write"], side_effect: "mutating", exit_codes: &[0, 2, 65, 75] },
    CommandMeta { path: &["write-note"], side_effect: "mutating", exit_codes: &[0, 2, 65, 75] },
    CommandMeta { path: &["supersede"], side_effect: "mutating", exit_codes: &[0, 2, 65, 66, 75] },
    CommandMeta { path: &["forget"], side_effect: "destructive", exit_codes: &[0, 2, 65, 66, 75] },
    CommandMeta { path: &["source", "capture"], side_effect: "mutating", exit_codes: &[0, 2, 65, 75] },
    CommandMeta { path: &["reveal"], side_effect: "read_only", exit_codes: &[0, 2, 65, 66, 75, 77] },
    CommandMeta { path: &["observe"], side_effect: "mutating", exit_codes: &[0, 2, 65, 75] },
    CommandMeta { path: &["status"], side_effect: "read_only", exit_codes: &[0, 2, 75] },
    CommandMeta { path: &["schema"], side_effect: "read_only", exit_codes: &[0, 2] },
];

pub fn run(args: SchemaArgs) -> anyhow::Result<()> {
    let contract = match args.section {
        SchemaSection::All => full_contract(),
        SchemaSection::Commands => json!({ "schema_version": SCHEMA_VERSION, "commands": commands_json() }),
        SchemaSection::Envelope => json!({ "schema_version": SCHEMA_VERSION, "envelope": envelope_json() }),
        SchemaSection::ExitCodes => json!({ "schema_version": SCHEMA_VERSION, "exit_codes": exit_codes_json() }),
    };
    println!("{}", serde_json::to_string_pretty(&contract)?);
    Ok(())
}

fn full_contract() -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "commands": commands_json(),
        "envelope": envelope_json(),
        "exit_codes": exit_codes_json(),
        "env": {
            "MEMORUM_REPO": "canonical Memorum repo root (default ~/memorum)",
            "MEMORUM_SOCKET": "daemon socket path override",
        },
    })
}

fn commands_json() -> Value {
    let root = Cli::command();
    Value::Array(COVERED.iter().map(|meta| command_json(&root, meta)).collect())
}

fn command_json(root: &ClapCommand, meta: &CommandMeta) -> Value {
    let leaf = descend(root, meta.path);
    let args: Vec<Value> =
        leaf.map(|command| command.get_arguments().filter_map(arg_json).collect()).unwrap_or_default();
    json!({
        "name": meta.path.join(" "),
        "side_effect": meta.side_effect,
        "exit_codes": meta.exit_codes,
        "args": args,
    })
}

/// Walk a subcommand path from the root, returning the leaf `Command` if present.
fn descend<'a>(root: &'a ClapCommand, path: &[&str]) -> Option<&'a ClapCommand> {
    let mut current = root;
    for segment in path {
        current = current.find_subcommand(segment)?;
    }
    Some(current)
}

/// Introspect one clap argument into a contract entry, dropping the auto-injected
/// `help`/`version` args that are not part of the surface.
fn arg_json(arg: &clap::Arg) -> Option<Value> {
    let id = arg.get_id().as_str();
    if id == "help" || id == "version" {
        return None;
    }
    let takes_value = matches!(arg.get_action(), ArgAction::Set | ArgAction::Append);
    let kind = if arg.is_positional() {
        "positional"
    } else if takes_value {
        "option"
    } else {
        "flag"
    };
    Some(json!({
        "name": id,
        "kind": kind,
        "required": arg.is_required_set(),
        "long": arg.get_long(),
        "repeatable": matches!(arg.get_action(), ArgAction::Append),
    }))
}

fn envelope_json() -> Value {
    json!({
        "success": {
            "ok": true,
            "data": "<command-specific payload object>",
            "meta": { "schema_version": SCHEMA_VERSION, "warnings": ["string"] },
            "stream": "stdout",
        },
        "error": {
            "ok": false,
            "error": {
                "code": "string",
                "message": "string",
                "details": "object | absent",
                "retryable": "bool",
                "suggested_fix": "string | absent",
            },
            "meta": { "schema_version": SCHEMA_VERSION, "warnings": ["string"] },
            "stream": "stderr",
        },
        "rules": [
            "stdout carries only the success envelope; diagnostics and the first-write banner go to stderr",
            "data is the inner payload DTO, never the daemon frame wrapper",
            "output is byte-stable across identical invocations",
        ],
    })
}

fn exit_codes_json() -> Value {
    let crosswalk: serde_json::Map<String, Value> =
        agent_exit_crosswalk().into_iter().map(|(code, exit)| (code.to_string(), json!(exit))).collect();
    json!({
        "agent_dictionary": {
            "0": "success, including a valid empty result and candidate/quarantined writes",
            "2": "usage / argument error (clap)",
            "65": "invalid input / validation / governance refusal",
            "66": "well-formed id that does not exist (not_found)",
            "70": "internal bug / invariant violation",
            "75": "daemon unreachable / transient failure (retryable)",
            "77": "client-side gate refusal (reveal without --allow-reveal)",
            "78": "config problem detected pre-connect",
        },
        "exceptions": {
            "doctor": "0 healthy / 1 unhealthy",
            "recall": "pinned Stream E v0.7 dictionary (1-5); raw block output, not the envelope",
            "dream": "lease/dream dictionary",
            "admin": "init/uninstall/export/review/quarantine/peer/web/reality-check/privacy/device/ui/import keep 1/2 until contract v2",
        },
        "daemon_crosswalk": Value::Object(crosswalk),
    })
}
