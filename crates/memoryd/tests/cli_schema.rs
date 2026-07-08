//! Task 3: `memoryd schema` publishes the machine contract and agrees, at field
//! level, with both the clap definitions and `docs/api/memoryd-cli-contract-v1.md`.

use std::collections::BTreeMap;
use std::process::Command;

use serde_json::Value;

fn schema(section: &[&str]) -> Value {
    let output =
        Command::new(env!("CARGO_BIN_EXE_memoryd")).arg("schema").args(section).output().expect("run memoryd schema");
    assert!(output.status.success(), "schema exits 0");
    serde_json::from_slice(&output.stdout).expect("schema stdout is valid JSON (round-trips through jq)")
}

fn command_named<'a>(contract: &'a Value, name: &str) -> &'a Value {
    contract["commands"]
        .as_array()
        .expect("commands is an array")
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("schema is missing covered command `{name}`"))
}

fn arg_named<'a>(command: &'a Value, name: &str) -> &'a Value {
    command["args"]
        .as_array()
        .expect("args is an array")
        .iter()
        .find(|arg| arg["name"] == name)
        .unwrap_or_else(|| panic!("command `{}` is missing arg `{name}`", command["name"]))
}

#[test]
fn schema_all_carries_every_covered_command_with_side_effect_and_exit_codes() {
    let contract = schema(&["--json"]);
    assert_eq!(contract["schema_version"], "1.0");

    let expected: BTreeMap<&str, &str> = BTreeMap::from([
        ("search", "read_only"),
        ("get", "read_only"),
        ("write", "mutating"),
        ("write-note", "mutating"),
        ("supersede", "mutating"),
        ("forget", "destructive"),
        ("source capture", "mutating"),
        ("status", "read_only"),
        ("schema", "read_only"),
    ]);
    for (name, side_effect) in expected {
        let command = command_named(&contract, name);
        assert_eq!(command["side_effect"], side_effect, "wrong side_effect for {name}");
        assert!(command["exit_codes"].as_array().unwrap().contains(&Value::from(0)), "{name} must allow exit 0");
    }
}

#[test]
fn schema_pins_field_level_argument_shapes() {
    let contract = schema(&["--json"]);

    // search: required positional query, --limit option, --include-body flag.
    let search = command_named(&contract, "search");
    let query = arg_named(search, "query");
    assert_eq!(query["kind"], "positional");
    assert_eq!(query["required"], true);
    assert_eq!(arg_named(search, "limit")["kind"], "option");
    assert_eq!(arg_named(search, "include_body")["kind"], "flag");

    // get: required positional id.
    let get = command_named(&contract, "get");
    assert_eq!(arg_named(get, "id")["kind"], "positional");
    assert_eq!(arg_named(get, "id")["required"], true);

    // write: required positional body, --meta option, repeatable --tag.
    let write = command_named(&contract, "write");
    assert_eq!(arg_named(write, "body")["kind"], "positional");
    assert_eq!(arg_named(write, "meta")["kind"], "option");
    assert_eq!(arg_named(write, "tags")["repeatable"], true);

    // forget: --reason is a required option.
    let forget = command_named(&contract, "forget");
    let reason = arg_named(forget, "reason");
    assert_eq!(reason["kind"], "option");
    assert_eq!(reason["required"], true);
}

#[test]
fn schema_exit_codes_match_the_contract_dictionary_and_crosswalk() {
    let contract = schema(&["exit-codes"]);
    let codes = &contract["exit_codes"];
    assert_eq!(codes["agent_dictionary"]["66"], "well-formed id that does not exist (not_found)");
    // Load-bearing crosswalk entries pinned by docs §3.
    let crosswalk = &codes["daemon_crosswalk"];
    assert_eq!(crosswalk["not_found"], 66);
    assert_eq!(crosswalk["substrate_error"], 75);
    assert_eq!(crosswalk["invalid_request"], 65);
    assert_eq!(crosswalk["embedding_provider_unsupported"], 70);
}

#[test]
fn schema_envelope_section_describes_both_shapes() {
    let contract = schema(&["envelope"]);
    let envelope = &contract["envelope"];
    assert_eq!(envelope["success"]["meta"]["schema_version"], "1.0");
    assert_eq!(envelope["success"]["stream"], "stdout");
    assert_eq!(envelope["error"]["stream"], "stderr");
    assert_eq!(envelope["error"]["ok"], false);
}

#[test]
fn schema_default_section_is_the_full_contract() {
    let contract = schema(&[]);
    assert!(contract.get("commands").is_some());
    assert!(contract.get("envelope").is_some());
    assert!(contract.get("exit_codes").is_some());
    assert!(contract.get("env").is_some());
}
