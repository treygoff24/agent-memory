use memory_substrate::tree::{bootstrap_repo_tree, validate_case_fold_paths, validate_tree, TreeValidationMode};
use memory_substrate::{Roots, Substrate, ValidationWarning};

#[tokio::test]
async fn fresh_init_creates_working_tree_dirs_and_tracked_bootstrap_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    assert!(roots.repo.join(".gitattributes").exists());
    assert!(roots.repo.join("config.yaml").exists());
    assert!(roots.repo.join("agent/patterns").is_dir());
    assert!(matches!(
        substrate.durability_tier(),
        memory_substrate::DurabilityTier::BestEffort | memory_substrate::DurabilityTier::Full
    ));
}

#[tokio::test]
async fn open_preserves_existing_bootstrap_file_contents() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    drop(substrate);
    std::fs::write(roots.repo.join(".gitignore"), "/.memoryd/\ncustom-user-rule\n").expect("custom gitignore");
    std::fs::write(roots.repo.join(".gitattributes"), "*.md merge=memory-merge-driver\n*.txt merge=union\n")
        .expect("custom gitattributes");

    let _reopened = Substrate::open(roots.clone()).await.expect("open");

    assert_eq!(
        std::fs::read_to_string(roots.repo.join(".gitignore")).expect("gitignore"),
        "/.memoryd/\ncustom-user-rule\n"
    );
    assert_eq!(
        std::fs::read_to_string(roots.repo.join(".gitattributes")).expect("gitattributes"),
        "*.md merge=memory-merge-driver\n*.txt merge=union\n"
    );
}

#[test]
fn duplicate_frontmatter_ids_fail_validation() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("tree");
    std::fs::create_dir_all(temp.path().join("agent/patterns")).expect("dirs");
    std::fs::write(temp.path().join("agent/patterns/a.md"), doc("A")).expect("write");
    std::fs::write(temp.path().join("agent/patterns/b.md"), doc("B")).expect("write");
    let err = validate_tree(temp.path(), TreeValidationMode::FullySynced).expect_err("duplicate id");
    assert!(err.to_string().contains("duplicate memory id"));
}

#[test]
fn case_only_path_collision_fixture_fails_validation() {
    let paths =
        vec![std::path::PathBuf::from("agent/patterns/Foo.md"), std::path::PathBuf::from("agent/patterns/foo.md")];

    let err = validate_case_fold_paths(&paths).expect_err("case-only collision");

    assert!(
        matches!(err, memory_substrate::ValidationError::CaseFoldCollision(path) if path == "agent/patterns/foo.md")
    );
}

#[test]
fn supersession_cycle_fails_cross_file_validation() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("tree");
    std::fs::create_dir_all(temp.path().join("agent/patterns")).expect("dirs");
    let a = "mem_20260424_a1b2c3d4e5f60718_000101";
    let b = "mem_20260424_a1b2c3d4e5f60718_000102";
    let c = "mem_20260424_a1b2c3d4e5f60718_000103";
    std::fs::write(
        temp.path().join(format!("agent/patterns/{a}.md")),
        doc_with_refs(RefDoc {
            summary: "A",
            id: a,
            status: "superseded",
            supersedes: &[b],
            superseded_by: &[c],
            related: &[],
        }),
    )
    .expect("write a");
    std::fs::write(
        temp.path().join(format!("agent/patterns/{b}.md")),
        doc_with_refs(RefDoc {
            summary: "B",
            id: b,
            status: "superseded",
            supersedes: &[c],
            superseded_by: &[a],
            related: &[],
        }),
    )
    .expect("write b");
    std::fs::write(
        temp.path().join(format!("agent/patterns/{c}.md")),
        doc_with_refs(RefDoc {
            summary: "C",
            id: c,
            status: "superseded",
            supersedes: &[a],
            superseded_by: &[b],
            related: &[],
        }),
    )
    .expect("write c");
    let err = validate_tree(temp.path(), TreeValidationMode::FullySynced).expect_err("cycle");
    assert!(err.to_string().contains("supersession cycle"));
}

#[test]
fn inverse_supersession_mismatch_fails_when_both_endpoints_exist() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("tree");
    std::fs::create_dir_all(temp.path().join("agent/patterns")).expect("dirs");
    let newer = "mem_20260424_a1b2c3d4e5f60718_000103";
    let older = "mem_20260424_a1b2c3d4e5f60718_000104";
    std::fs::write(
        temp.path().join(format!("agent/patterns/{newer}.md")),
        doc_with_refs(RefDoc {
            summary: "newer",
            id: newer,
            status: "active",
            supersedes: &[older],
            superseded_by: &[],
            related: &[],
        }),
    )
    .expect("write newer");
    std::fs::write(
        temp.path().join(format!("agent/patterns/{older}.md")),
        doc_with_refs(RefDoc {
            summary: "older",
            id: older,
            status: "active",
            supersedes: &[],
            superseded_by: &[],
            related: &[],
        }),
    )
    .expect("write older");
    let err = validate_tree(temp.path(), TreeValidationMode::FullySynced).expect_err("inverse mismatch");
    assert!(err.to_string().contains("inverse supersession mismatch"));
}

#[test]
fn inverse_supersession_mismatch_warns_during_partial_sync() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("tree");
    std::fs::create_dir_all(temp.path().join("agent/patterns")).expect("dirs");
    let newer = "mem_20260424_a1b2c3d4e5f60718_000105";
    let older = "mem_20260424_a1b2c3d4e5f60718_000106";
    std::fs::write(
        temp.path().join(format!("agent/patterns/{newer}.md")),
        doc_with_refs(RefDoc {
            summary: "newer",
            id: newer,
            status: "active",
            supersedes: &[older],
            superseded_by: &[],
            related: &[],
        }),
    )
    .expect("write newer");
    std::fs::write(
        temp.path().join(format!("agent/patterns/{older}.md")),
        doc_with_refs(RefDoc {
            summary: "older",
            id: older,
            status: "active",
            supersedes: &[],
            superseded_by: &[],
            related: &[],
        }),
    )
    .expect("write older");
    let report = validate_tree(temp.path(), TreeValidationMode::PartialSync).expect("partial sync warning");
    assert!(report
        .warnings
        .iter()
        .any(|warning| matches!(warning, ValidationWarning::InverseSupersessionMismatch { .. })));
}

fn doc(summary: &str) -> String {
    doc_with_refs(RefDoc {
        summary,
        id: "mem_20260424_a1b2c3d4e5f60718_000001",
        status: "active",
        supersedes: &[],
        superseded_by: &[],
        related: &[],
    })
}

struct RefDoc<'a> {
    summary: &'a str,
    id: &'a str,
    status: &'a str,
    supersedes: &'a [&'a str],
    superseded_by: &'a [&'a str],
    related: &'a [&'a str],
}

fn doc_with_refs(doc: RefDoc<'_>) -> String {
    format!(
        r#"---
schema_version: 1
id: {id}
type: pattern
scope: agent
summary: {summary}
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: {status}
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
supersedes: {supersedes}
superseded_by: {superseded_by}
related: {related}
---
Body text.
"#,
        id = doc.id,
        summary = doc.summary,
        status = doc.status,
        supersedes = yaml_array(doc.supersedes),
        superseded_by = yaml_array(doc.superseded_by),
        related = yaml_array(doc.related),
    )
}

fn yaml_array(values: &[&str]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", values.join(", "))
    }
}
