use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use memorum_eval::daemon_scaffold::DaemonScaffold;

use crate::support::{command_output, debug_binary};

const BASE_UPDATED_AT: &str = "2026-05-01T12:00:00Z";
const DEVICE_A_UPDATED_AT: &str = "2026-05-01T12:05:00Z";
const DEVICE_B_UPDATED_AT: &str = "2026-05-01T12:04:00Z";
const MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000014";

#[tokio::test]
async fn t14_merge_driver_preserves_two_device_semantic_edits() {
    let scaffold = DaemonScaffold::two_device().await;
    let temp_dir = scratch_dir();
    std::fs::create_dir_all(&temp_dir).expect("create merge input dir");

    let base = temp_dir.join("base.md");
    let ours = temp_dir.join("ours.md");
    let theirs = temp_dir.join("theirs.md");
    std::fs::write(&base, memory_doc("Shared merge baseline", 0.80, BASE_UPDATED_AT, &[])).expect("write base");
    std::fs::write(
        &ours,
        memory_doc(
            "Shared merge baseline",
            0.92,
            DEVICE_A_UPDATED_AT,
            &[EntityFixture { id: "ent_merge_test_alpha", label: "Merge Test Alpha" }],
        ),
    )
    .expect("write ours");
    std::fs::write(
        &theirs,
        memory_doc(
            "Device B refined the shared merge summary",
            0.80,
            DEVICE_B_UPDATED_AT,
            &[EntityFixture { id: "ent_merge_test_beta", label: "Merge Test Beta" }],
        ),
    )
    .expect("write theirs");

    let merge_driver = debug_binary("memory-merge-driver", "memory-merge-driver");
    let output = command_output(
        &merge_driver,
        [
            "--base",
            path(&base),
            "--ours",
            path(&ours),
            "--theirs",
            path(&theirs),
            "--path",
            "agent/patterns/t14-merge-driver.md",
        ],
    );
    assert!(
        output.status.success(),
        "merge driver should exit 0\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let merged = std::fs::read_to_string(&ours).expect("read merged ours file");
    assert!(merged.contains("id: ent_merge_test_alpha"), "merged entities should include Device A:\n{merged}");
    assert!(merged.contains("id: ent_merge_test_beta"), "merged entities should include Device B:\n{merged}");
    assert!(merged.contains("confidence: 0.92"), "merged confidence should preserve Device A update:\n{merged}");
    assert!(
        merged.contains("summary: Device B refined the shared merge summary"),
        "merged summary should preserve Device B update:\n{merged}"
    );
    assert!(
        merged.contains(&format!("updated_at: {DEVICE_A_UPDATED_AT}")),
        "merged updated_at should be no earlier than Device A's write timestamp:\n{merged}"
    );

    let merged_tree_path = scaffold.device_a.tree_dir().join("agent/patterns/t14-merge-driver.md");
    std::fs::create_dir_all(merged_tree_path.parent().expect("merged tree path has parent"))
        .expect("create output dir");
    std::fs::write(&merged_tree_path, merged).expect("write merged memory into scaffold tree");

    let doctor = scaffold.device_a.doctor().await;
    assert!(doctor.healthy, "memoryd doctor should accept merged tree with zero validation errors: {doctor:?}");
}

struct EntityFixture<'a> {
    id: &'a str,
    label: &'a str,
}

fn memory_doc(summary: &str, confidence: f64, updated_at: &str, entities: &[EntityFixture<'_>]) -> String {
    format!(
        r#"---
schema_version: 1
id: {MEMORY_ID}
type: pattern
scope: agent
summary: {summary}
confidence: {confidence:.2}
trust_level: trusted
sensitivity: internal
status: active
created_at: {BASE_UPDATED_AT}
updated_at: {updated_at}
author:
  kind: system
  user_handle: null
  harness: memorum-eval
  harness_version: null
  session_id: t14-two-device
  subagent_id: null
  phase: null
  component: stream-h-domain-test
entities:{entities}
---
Shared memory body stays stable while frontmatter changes on two devices.
"#,
        entities = entity_yaml(entities),
    )
}

fn entity_yaml(entities: &[EntityFixture<'_>]) -> String {
    if entities.is_empty() {
        return " []".to_owned();
    }

    let mut yaml = String::new();
    for entity in entities {
        yaml.push_str(&format!("\n  - id: {}\n    label: {}\n", entity.id, entity.label));
    }
    yaml
}

fn path(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

fn scratch_dir() -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock after unix epoch").as_nanos();
    std::env::temp_dir().join(format!("memorum-eval-t14-{nanos}"))
}
