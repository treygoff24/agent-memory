#[path = "eval/handbook/t01_exact_identifier_recall.rs"]
mod t01_exact_identifier_recall;
#[path = "eval/handbook/t02_superseded_fact.rs"]
mod t02_superseded_fact;
#[path = "eval/handbook/t03_cross_project_entity_collision.rs"]
mod t03_cross_project_entity_collision;
#[path = "eval/handbook/t04_abstention.rs"]
mod t04_abstention;
#[path = "eval/handbook/t05_poisoned_candidate.rs"]
mod t05_poisoned_candidate;
#[path = "eval/handbook/t06_tool_output_preservation.rs"]
mod t06_tool_output_preservation;
#[path = "eval/handbook/t07_subagent_writeback.rs"]
mod t07_subagent_writeback;
#[path = "eval/handbook/t08_deletion_and_tombstone.rs"]
mod t08_deletion_and_tombstone;
#[path = "eval/handbook/t09_recall_budget_pressure.rs"]
mod t09_recall_budget_pressure;
#[path = "eval/handbook/t10_compaction_resumption.rs"]
mod t10_compaction_resumption;
#[path = "eval/handbook/t11_self_poisoning.rs"]
mod t11_self_poisoning;
#[path = "eval/handbook/t12_temporal_validity.rs"]
mod t12_temporal_validity;

mod support {
    use std::fs;
    use std::path::{Path, PathBuf};

    use memorum_eval::simulator::SimulatorObservations;
    use serde_json::{json, Value};

    pub const DEFAULT_PROJECT_ID: &str = "agent-memory";

    pub fn promoted_project_meta(label: &str, memory_type: &str) -> String {
        promoted_meta("project", label, memory_type)
    }

    pub fn promoted_meta(namespace: &str, label: &str, memory_type: &str) -> String {
        governed_meta_json(GovernedMetaJson {
            namespace,
            memory_type,
            confidence: 0.95,
            source_kind: "agent_primary",
            source_ref: Some(grounding_source_ref(label)),
            explicit_user_context: true,
        })
    }

    pub struct GovernedMetaJson<'a> {
        pub namespace: &'a str,
        pub memory_type: &'a str,
        pub confidence: f64,
        pub source_kind: &'a str,
        pub source_ref: Option<String>,
        pub explicit_user_context: bool,
    }

    pub fn governed_meta_json(meta: GovernedMetaJson<'_>) -> String {
        json!({
            "namespace": meta.namespace,
            "type": meta.memory_type,
            "confidence": meta.confidence,
            "source_kind": meta.source_kind,
            "source_ref": meta.source_ref,
            "explicit_user_context": meta.explicit_user_context
        })
        .to_string()
    }

    pub fn low_confidence_ungrounded_meta() -> String {
        json!({
            "namespace": "project",
            "type": "claim",
            "confidence": 0.30,
            "source_kind": "agent_primary",
            "source_ref": null,
            "explicit_user_context": false
        })
        .to_string()
    }

    pub fn write_project_file(cwd: &Path, canonical_id: &str, alias: &str) {
        fs::create_dir_all(cwd).unwrap_or_else(|err| panic!("create project cwd {}: {err}", cwd.display()));
        fs::write(cwd.join(".memory-project.yaml"), format!("canonical_id: {canonical_id}\nalias: {alias}\n"))
            .unwrap_or_else(|err| panic!("write .memory-project.yaml in {}: {err}", cwd.display()));
    }

    pub fn assert_success_response(json: &str) {
        let parsed = parse_json(json);
        assert!(parsed.pointer("/result/success").is_some(), "expected success response, got:\n{}", pretty(&parsed));
    }

    pub fn payload(json: &str, name: &str) -> Value {
        let parsed = parse_json(json);
        parsed
            .pointer(&format!("/result/success/{name}"))
            .cloned()
            .unwrap_or_else(|| panic!("missing payload `{name}` in response:\n{}", pretty(&parsed)))
    }

    pub fn write_id(observations: &SimulatorObservations) -> String {
        let json = observations.last_write_json.as_deref().expect("write response captured");
        write_id_from_json(json)
    }

    pub fn write_id_from_json(json: &str) -> String {
        payload(json, "governance_write")
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("write response missing id:\n{json}"))
            .to_owned()
    }

    pub fn supersede_new_id(observations: &SimulatorObservations) -> String {
        let json = observations.last_supersede_json.as_deref().expect("supersede response captured");
        payload(json, "governance_supersede")
            .get("new_id")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("supersede response missing new_id:\n{json}"))
            .to_owned()
    }

    pub fn search_total(json: &str) -> usize {
        payload(json, "search").get("total").and_then(Value::as_u64).expect("search total") as usize
    }

    pub fn search_hits(json: &str) -> Vec<Value> {
        payload(json, "search").get("hits").and_then(Value::as_array).expect("search hits").clone()
    }

    pub fn startup_invoked_total(json: &str) -> u64 {
        payload(json, "status")
            .pointer("/recall/startup_invoked_total")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("status response missing startup counter:\n{json}"))
    }

    pub fn memory_file_body(tree_dir: &Path, memory_id: &str) -> String {
        let id_field = format!("id: {memory_id}");
        let path = find_file_containing(tree_dir, &id_field)
            .or_else(|| find_file_containing(tree_dir, memory_id))
            .unwrap_or_else(|| panic!("could not find canonical memory file containing id {memory_id}"));
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read memory file {}: {err}", path.display()))
    }

    pub fn find_file_containing(root: &Path, needle: &str) -> Option<PathBuf> {
        let entries = fs::read_dir(root).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find_file_containing(&path, needle) {
                    return Some(found);
                }
                continue;
            }
            if path.is_file() && fs::read_to_string(&path).is_ok_and(|body| body.contains(needle)) {
                return Some(path);
            }
        }
        None
    }

    pub fn parse_json(json: &str) -> Value {
        serde_json::from_str(json).unwrap_or_else(|err| panic!("invalid JSON response: {err}\n{json}"))
    }

    pub fn grounding_source_ref(label: &str) -> String {
        let path =
            std::env::temp_dir().join(format!("memorum-eval-handbook-grounding-{}-{label}.txt", std::process::id()));
        fs::write(&path, format!("grounding fixture for {label}\n"))
            .unwrap_or_else(|err| panic!("write grounding fixture {}: {err}", path.display()));
        format!("file:{}#{label}", path.display())
    }

    fn pretty(value: &Value) -> String {
        serde_json::to_string_pretty(value).expect("value pretty prints")
    }
}
