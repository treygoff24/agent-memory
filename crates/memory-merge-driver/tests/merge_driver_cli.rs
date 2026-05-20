use std::process::Command;

use memory_substrate::frontmatter::parse_document;

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
    // `doc()` appends a trailing newline after `{body}`, so passing bodies
    // without their own trailing `\n` keeps the on-disk body section a single
    // line-terminated block — what the round-trip parse expects below.
    let base_doc = doc(1, "base", "alpha\nbeta\ngamma");
    let ours_doc = doc(1, "ours-summary", "alpha\nbeta\ngamma");
    let theirs_doc = doc(1, "base", "alpha\nbeta\nGAMMA");
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
    assert_no_conflict_markers(&merged);
    assert_eq!(
        merged.lines().filter(|line| *line == "---").count(),
        2,
        "merged file should have one frontmatter block"
    );

    let parsed = parse_document(&merged, None).expect("merged output should remain a valid canonical memory document");
    assert_eq!(parsed.memory.frontmatter.summary, "ours-summary");
    assert_eq!(parsed.memory.body, "alpha\nbeta\nGAMMA\n", "merged body kept theirs' GAMMA edit exactly");
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

#[cfg(unix)]
#[test]
fn merge_driver_write_failure_reports_write_and_leaves_ours_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let base = temp.path().join("base.md");
    let ours_path = temp.path().join("ours.md");
    let theirs = temp.path().join("theirs.md");
    let base_doc = doc(1, "base", "alpha\nbeta\ngamma\n");
    let original_ours = doc(1, "ours-summary", "alpha\nbeta\ngamma\n");
    let theirs_doc = doc(1, "base", "alpha\nbeta\nGAMMA\n");
    std::fs::write(&base, &base_doc).expect("base");
    std::fs::write(&ours_path, &original_ours).expect("ours");
    std::fs::write(&theirs, &theirs_doc).expect("theirs");

    let original_permissions = std::fs::metadata(temp.path()).expect("temp metadata").permissions();
    let mut readonly_permissions = original_permissions.clone();
    readonly_permissions.set_mode(0o555);
    std::fs::set_permissions(temp.path(), readonly_permissions).expect("make temp dir read-only");
    match write_probe_succeeds(temp.path()) {
        Ok(true) => {
            std::fs::set_permissions(temp.path(), original_permissions).expect("restore temp dir permissions");
            eprintln!(
                "skipping write-failure assertion because this runner can still create files in a 0555 directory"
            );
            return;
        }
        Ok(false) => {}
        Err(error) => {
            std::fs::set_permissions(temp.path(), original_permissions).expect("restore temp dir permissions");
            panic!("permission probe failed unexpectedly in {}: {error}", temp.path().display());
        }
    }

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

    std::fs::set_permissions(temp.path(), original_permissions).expect("restore temp dir permissions");

    assert!(!output.status.success(), "expected write failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("write "), "stderr should report write operation: {stderr}");
    assert!(stderr.contains(path(&ours_path)), "stderr should name ours path: {stderr}");
    assert!(!stderr.contains("read "), "write failure must not be labeled as read failure: {stderr}");
    assert_eq!(std::fs::read_to_string(&ours_path).expect("ours after"), original_ours);
}

fn path(p: &std::path::Path) -> &str {
    p.to_str().expect("utf-8 path")
}

fn assert_no_conflict_markers(text: &str) {
    for marker in ["<<<<<<<", "=======", ">>>>>>>"] {
        assert!(!text.contains(marker), "merged output should not contain conflict marker {marker:?}:\n{text}");
    }
}

#[cfg(unix)]
fn write_probe_succeeds(directory: &std::path::Path) -> std::io::Result<bool> {
    let probe_path = directory.join(format!(".merge-driver-permission-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe_path) {
        Ok(_) => {
            let _ = std::fs::remove_file(probe_path);
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(false),
        Err(error) => Err(error),
    }
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
