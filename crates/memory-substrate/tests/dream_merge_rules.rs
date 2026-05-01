//! Stream F noncanonical merge-driver rules.

use memory_substrate::merge::{merge_markdown, MergeInput, MergeResult};
use serde_json::Value;

#[test]
fn substrate_jsonl_merges_by_unique_concat_sorted_by_id() {
    let base = jsonl([r#"{"id":"frag_b","ts":"2026-04-30T03:00:00Z","text":"base"}"#]);
    let ours = jsonl([
        r#"{"id":"frag_b","ts":"2026-04-30T03:00:00Z","text":"base"}"#,
        r#"{"id":"frag_d","ts":"2026-04-30T03:00:03Z","text":"ours"}"#,
    ]);
    let theirs = jsonl([
        r#"{"id":"frag_b","ts":"2026-04-30T03:00:00Z","text":"base"}"#,
        r#"{"id":"frag_a","ts":"2026-04-30T03:00:01Z","text":"theirs"}"#,
    ]);

    let MergeResult::Clean(merged) = merge_file("substrate/dev_local/2026-04-30.jsonl", &base, &ours, &theirs) else {
        panic!("expected clean substrate JSONL merge");
    };

    assert_eq!(field_values(&merged, "id"), ["frag_a", "frag_b", "frag_d"]);
}

#[test]
fn questions_and_lease_jsonl_merge_by_scope_ts_id_with_missing_id_fallback() {
    let question_ours = jsonl([
        r#"{"scope":"project:proj_abc","ts":"2026-04-30T03:00:20Z","id":"q_2","question":"ours"}"#,
        r#"{"scope":"me","ts":"2026-04-30T03:00:15Z","question":"legacy no id"}"#,
    ]);
    let question_theirs = jsonl([
        r#"{"scope":"agent","ts":"2026-04-30T03:00:10Z","id":"q_1","question":"theirs"}"#,
        r#"{"scope":"project:proj_abc","ts":"2026-04-30T03:00:20Z","question":"same sort key without id"}"#,
    ]);

    let MergeResult::Clean(merged_questions) =
        merge_file("dreams/questions/project/proj_abc/2026-04-30.jsonl", "", &question_ours, &question_theirs)
    else {
        panic!("expected clean dream-question JSONL merge");
    };
    assert_eq!(field_values(&merged_questions, "scope"), ["agent", "me", "project:proj_abc", "project:proj_abc"]);
    assert_eq!(
        field_values(&merged_questions, "question"),
        ["theirs", "legacy no id", "ours", "same sort key without id"]
    );

    let lease_ours =
        jsonl([r#"{"device":"dev_a","scope":"me","acquired_at":"2026-04-30T03:00:20Z","run_id":"run_b"}"#]);
    let lease_theirs = jsonl([
        r#"{"device":"dev_b","scope":"agent","acquired_at":"2026-04-30T03:00:10Z","run_id":"run_a"}"#,
        r#"{"device":"dev_legacy","scope":"me","acquired_at":"2026-04-30T03:00:10Z"}"#,
    ]);

    let MergeResult::Clean(merged_lease) = merge_file("leases/journal.lease", "", &lease_ours, &lease_theirs) else {
        panic!("expected clean lease JSONL merge");
    };
    assert_eq!(field_values(&merged_lease, "scope"), ["agent", "me", "me"]);
    assert_eq!(field_values(&merged_lease, "device"), ["dev_b", "dev_legacy", "dev_a"]);
}

#[test]
fn dream_journal_markdown_uses_lww_and_quarantines_contested_same_scope_date_writes() {
    let base = "base journal\n";
    let ours = "journal from lease holder\n";

    let MergeResult::Clean(merged) = merge_file("dreams/journal/me/2026-04-30.md", base, ours, base) else {
        panic!("expected clean one-sided journal merge");
    };
    assert_eq!(merged, ours);

    let MergeResult::Quarantine(contested) =
        merge_file("dreams/journal/me/2026-04-30.md", base, "journal from dev_a\n", "journal from dev_b\n")
    else {
        panic!("expected quarantine marker for contested journal write");
    };

    assert!(contested.contains("stream-f-merge: quarantine contested dream journal"));
    assert!(contested.contains("scope_path: me"));
    assert!(contested.contains("date: 2026-04-30"));
    assert!(contested.contains("journal from dev_a"));
    assert!(contested.contains("journal from dev_b"));
}

#[test]
fn cleanup_json_uses_last_writer_wins_by_device_date() {
    let base =
        r#"{"device_id":"dev_local","date":"2026-04-30","completed_at":"2026-04-30T03:00:00Z","fragments_archived":1}"#;
    let ours =
        r#"{"device_id":"dev_local","date":"2026-04-30","completed_at":"2026-04-30T03:10:00Z","fragments_archived":2}"#;
    let theirs =
        r#"{"device_id":"dev_local","date":"2026-04-30","completed_at":"2026-04-30T03:20:00Z","fragments_archived":3}"#;

    let MergeResult::Clean(merged) = merge_file("dreams/cleanup/dev_local/2026-04-30.json", base, ours, theirs) else {
        panic!("expected clean cleanup JSON merge");
    };

    let parsed: Value = serde_json::from_str(&merged).expect("valid cleanup JSON");
    assert_eq!(parsed["device_id"], "dev_local");
    assert_eq!(parsed["date"], "2026-04-30");
    assert_eq!(parsed["completed_at"], "2026-04-30T03:20:00Z");
    assert_eq!(parsed["fragments_archived"], 3);
}

fn merge_file(path: &str, base: &str, ours: &str, theirs: &str) -> MergeResult {
    merge_markdown(MergeInput { base, ours, theirs, path }).expect("merge succeeds")
}

fn jsonl<const N: usize>(lines: [&str; N]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn field_values(jsonl: &str, field: &str) -> Vec<String> {
    jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("valid jsonl row");
            value[field].as_str().expect("string field").to_string()
        })
        .collect()
}
