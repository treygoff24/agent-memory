use std::process::Command;

#[test]
fn merge_driver_requires_base_ours_theirs_and_path_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_memory-merge-driver")).output().expect("run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("Usage"),
        "stderr should hint at missing args; got: {stderr}"
    );
}

#[test]
fn merge_driver_clean_round_trip_writes_merged_to_ours() {
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().join("base.md");
    let ours_path = temp.path().join("ours.md");
    let theirs = temp.path().join("theirs.md");
    let base_doc = doc(1, "base", "alpha\nbeta\ngamma\n");
    let ours_doc = doc(1, "ours-summary", "alpha\nbeta\ngamma\n");
    let theirs_doc = doc(1, "base", "alpha\nbeta\nGAMMA\n");
    std::fs::write(&base, &base_doc).expect("base");
    std::fs::write(&ours_path, &ours_doc).expect("ours");
    std::fs::write(&theirs, &theirs_doc).expect("theirs");

    let output = Command::new(env!("CARGO_BIN_EXE_memory-merge-driver"))
        .args([
            "--base",
            path(&base),
            "--ours",
            path(&ours_path),
            "--theirs",
            path(&theirs),
            "--path",
            "agent/patterns/m.md",
        ])
        .output()
        .expect("run");
    assert!(output.status.success(), "expected clean exit; stderr: {}", String::from_utf8_lossy(&output.stderr));
    let merged = std::fs::read_to_string(&ours_path).expect("merged");
    assert!(merged.contains("summary: ours-summary"));
    // Body should reflect theirs' GAMMA edit (ours unchanged on body).
    assert!(merged.contains("alpha\nbeta\nGAMMA\n"), "merged body kept theirs' GAMMA edit");
}

#[test]
fn merge_driver_secret_sensitivity_refuses_with_specific_stderr() {
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().join("base.md");
    let ours_path = temp.path().join("ours.md");
    let theirs = temp.path().join("theirs.md");
    let base_doc = doc(1, "base", "body");
    let secret_doc = doc_with_sensitivity(1, "secret-side", "body", "secret");
    let original_ours = secret_doc.clone();
    std::fs::write(&base, &base_doc).expect("base");
    std::fs::write(&ours_path, &secret_doc).expect("ours");
    std::fs::write(&theirs, doc(1, "theirs", "body")).expect("theirs");

    let output = Command::new(env!("CARGO_BIN_EXE_memory-merge-driver"))
        .args([
            "--base",
            path(&base),
            "--ours",
            path(&ours_path),
            "--theirs",
            path(&theirs),
            "--path",
            "agent/patterns/m.md",
        ])
        .output()
        .expect("run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("secret sensitivity refused"), "stderr: {stderr}");
    // Spec §14.4: ours file is left untouched on refusal.
    assert_eq!(std::fs::read_to_string(&ours_path).expect("ours after"), original_ours);
}

fn path(p: &std::path::Path) -> &str {
    p.to_str().expect("utf-8 path")
}

fn doc_with_sensitivity(schema_version: u32, summary: &str, body: &str, sensitivity: &str) -> String {
    let indexable = if matches!(sensitivity, "confidential" | "personal") { "false" } else { "true" };
    format!(
        r#"---
schema_version: {schema_version}
id: mem_20260424_a1b2c3d4e5f60718_000201
type: pattern
scope: agent
summary: {summary}
confidence: 1.0
trust_level: trusted
sensitivity: {sensitivity}
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: null
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: test
retrieval_policy:
  index_body: {indexable}
  index_embeddings: {indexable}
  mask_personal_for_synthesis: false
  max_scope: agent
  passive_recall: true
---
{body}
"#
    )
}

#[test]
fn merge_driver_schema_version_gate_exits_one_without_writing_ours() {
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().join("base.md");
    let ours = temp.path().join("ours.md");
    let theirs = temp.path().join("theirs.md");
    let base_doc = doc(1, "base", "base body");
    let ours_doc = doc(2, "ours", "ours body");
    std::fs::write(&base, base_doc).expect("base");
    std::fs::write(&ours, &ours_doc).expect("ours");
    std::fs::write(&theirs, doc(1, "theirs", "theirs body")).expect("theirs");

    let output = Command::new(env!("CARGO_BIN_EXE_memory-merge-driver"))
        .args([
            "--base",
            base.to_str().expect("base path"),
            "--ours",
            ours.to_str().expect("ours path"),
            "--theirs",
            theirs.to_str().expect("theirs path"),
            "--path",
            "agent/patterns/schema.md",
        ])
        .output()
        .expect("run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("schema_version=2 exceeds supported=1"));
    assert_eq!(std::fs::read_to_string(ours).expect("ours after"), ours_doc);
}

#[test]
fn merge_driver_schema_version_gate_checks_base_side() {
    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().join("base.md");
    let ours = temp.path().join("ours.md");
    let theirs = temp.path().join("theirs.md");
    let ours_doc = doc(1, "ours", "ours body");
    std::fs::write(&base, doc(2, "base", "base body")).expect("base");
    std::fs::write(&ours, &ours_doc).expect("ours");
    std::fs::write(&theirs, doc(1, "theirs", "theirs body")).expect("theirs");

    let output = Command::new(env!("CARGO_BIN_EXE_memory-merge-driver"))
        .args([
            "--base",
            base.to_str().expect("base path"),
            "--ours",
            ours.to_str().expect("ours path"),
            "--theirs",
            theirs.to_str().expect("theirs path"),
            "--path",
            "agent/patterns/schema.md",
        ])
        .output()
        .expect("run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("schema_version=2 exceeds supported=1"));
    assert_eq!(std::fs::read_to_string(ours).expect("ours after"), ours_doc);
}

fn doc(schema_version: u32, summary: &str, body: &str) -> String {
    format!(
        r#"---
schema_version: {schema_version}
id: mem_20260424_a1b2c3d4e5f60718_000201
type: pattern
scope: agent
summary: {summary}
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: null
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: test
---
{body}
"#
    )
}
