//! Golden corpus fixture lint (Task 4.1).
//!
//! This is the integrity gate for the dynamics-program measuring instrument
//! (`fixtures/golden/`). It does NOT measure recall quality — that is Task 4.2's
//! `quality.rs` runner. It only guarantees the corpus is well-formed so the
//! quality runner can trust it:
//!
//!   1. Every memory file parses and validates through the real Stream A
//!      frontmatter pipeline (`parse_document` -> `validate_frontmatter`).
//!   2. Every `mem_...` id referenced by `queries.yaml` exists in the corpus.
//!   3. Per query case, the graded sets (essential / useful / irrelevant_traps)
//!      are pairwise disjoint.
//!   4. The `secret` sensitivity value never appears on disk (CLAUDE.md
//!      invariant: `secret` is a runtime classification, never persisted).
//!
//! If this test fails after editing the corpus, regenerate via the authoring
//! scripts (`fixtures/golden/_generate.py`, then `_generate_queries.py`) rather
//! than hand-patching the emitted files.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use memory_substrate::frontmatter::parse_document;

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/golden")
}

fn memories_root() -> PathBuf {
    golden_root().join("memories")
}

/// Recursively collect every `.md` file under `dir`.
fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|err| panic!("read dir {}: {err}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry readable").path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn all_memory_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_markdown(&memories_root(), &mut files);
    files.sort();
    files
}

#[test]
fn corpus_directory_is_present_and_populated() {
    let root = memories_root();
    assert!(root.is_dir(), "golden corpus memories/ must exist: {}", root.display());
    let files = all_memory_files();
    assert!(
        files.len() >= 80,
        "golden corpus should hold the full hand-curated set (>=80 memories); found {}",
        files.len()
    );
}

#[test]
fn every_memory_parses_and_validates() {
    let mut failures = Vec::new();
    let mut ids: BTreeMap<String, PathBuf> = BTreeMap::new();

    for path in all_memory_files() {
        let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let rel = path.strip_prefix(golden_root()).unwrap_or(&path).to_path_buf();

        // `parse_document` runs `validate_frontmatter` internally and rejects
        // anything that violates the Stream A schema/lifecycle/cross-field rules.
        match parse_document(&text, None) {
            Ok(parsed) => {
                let id = parsed.memory.frontmatter.id.to_string();
                if let Some(existing) = ids.insert(id.clone(), rel.clone()) {
                    failures.push(format!("duplicate memory id {id}: {} and {}", existing.display(), rel.display()));
                }
            }
            Err(err) => failures.push(format!("{}: parse/validate failed: {err:?}", rel.display())),
        }
    }

    assert!(failures.is_empty(), "golden corpus integrity violations:\n{}", failures.join("\n"));
}

#[test]
fn no_secret_sensitivity_on_disk() {
    // `secret` is a runtime ClassificationOutcome, never a persisted sensitivity
    // value (CLAUDE.md invariant). Guard the corpus against it explicitly.
    let mut offenders = Vec::new();
    for path in all_memory_files() {
        let text = fs::read_to_string(&path).expect("memory readable");
        let rel = path.strip_prefix(golden_root()).unwrap_or(&path).to_path_buf();
        if text.lines().any(|line| line.trim() == "sensitivity: secret") {
            offenders.push(rel.display().to_string());
        }
    }
    assert!(offenders.is_empty(), "`secret` sensitivity must never be persisted:\n{}", offenders.join("\n"));
}

// --- queries.yaml -----------------------------------------------------------

#[derive(serde::Deserialize)]
struct QueryFile {
    cases: Vec<QueryCase>,
}

#[derive(serde::Deserialize)]
struct QueryCase {
    id: String,
    query: String,
    namespace_scope: Vec<String>,
    graded: Graded,
}

#[derive(serde::Deserialize)]
struct Graded {
    essential: Vec<String>,
    useful: Vec<String>,
    irrelevant_traps: Vec<String>,
}

fn load_queries() -> QueryFile {
    let path = golden_root().join("queries.yaml");
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_yaml::from_str(&text).unwrap_or_else(|err| panic!("parse queries.yaml: {err}"))
}

/// Set of all `mem_...` ids present in the corpus, parsed via the real pipeline.
fn corpus_ids() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for path in all_memory_files() {
        let text = fs::read_to_string(&path).expect("memory readable");
        let parsed = parse_document(&text, None).unwrap_or_else(|err| panic!("{}: {err:?}", path.display()));
        ids.insert(parsed.memory.frontmatter.id.to_string());
    }
    ids
}

#[test]
fn queries_reference_only_existing_memories() {
    let corpus = corpus_ids();
    let queries = load_queries();

    let mut dangling = Vec::new();
    for case in &queries.cases {
        for (grade, ids) in [
            ("essential", &case.graded.essential),
            ("useful", &case.graded.useful),
            ("irrelevant_traps", &case.graded.irrelevant_traps),
        ] {
            for id in ids {
                if !corpus.contains(id) {
                    dangling.push(format!("{}/{grade}: id {id} not in corpus", case.id));
                }
            }
        }
    }
    assert!(dangling.is_empty(), "queries.yaml references nonexistent memory ids:\n{}", dangling.join("\n"));
}

#[test]
fn graded_sets_are_disjoint_per_case() {
    let queries = load_queries();
    let mut violations = Vec::new();

    for case in &queries.cases {
        let essential: BTreeSet<&String> = case.graded.essential.iter().collect();
        let useful: BTreeSet<&String> = case.graded.useful.iter().collect();
        let traps: BTreeSet<&String> = case.graded.irrelevant_traps.iter().collect();

        let report = |a: &str, b: &str, overlap: Vec<&String>| {
            if overlap.is_empty() {
                None
            } else {
                let joined = overlap.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", ");
                Some(format!("{}: {a}/{b} overlap: {joined}", case.id))
            }
        };

        if let Some(msg) = report("essential", "useful", essential.intersection(&useful).copied().collect()) {
            violations.push(msg);
        }
        if let Some(msg) = report("essential", "irrelevant_traps", essential.intersection(&traps).copied().collect()) {
            violations.push(msg);
        }
        if let Some(msg) = report("useful", "irrelevant_traps", useful.intersection(&traps).copied().collect()) {
            violations.push(msg);
        }
    }

    assert!(violations.is_empty(), "graded-set disjointness violations:\n{}", violations.join("\n"));
}

#[test]
fn query_cases_are_well_formed() {
    let queries = load_queries();
    assert!(queries.cases.len() >= 40, "expected >=40 labeled cases; found {}", queries.cases.len());

    let mut seen = BTreeSet::new();
    let mut problems = Vec::new();
    let mut abstention_cases = 0usize;
    for case in &queries.cases {
        if !seen.insert(case.id.clone()) {
            problems.push(format!("duplicate case id: {}", case.id));
        }
        if case.query.trim().is_empty() {
            problems.push(format!("{}: empty query", case.id));
        }
        if case.namespace_scope.is_empty() {
            problems.push(format!("{}: empty namespace_scope", case.id));
        }
        if case.graded.essential.is_empty() && case.graded.useful.is_empty() {
            abstention_cases += 1;
        }
    }

    assert!(problems.is_empty(), "malformed query cases:\n{}", problems.join("\n"));
    assert!(
        abstention_cases >= 1,
        "corpus must include at least one abstention case (empty essential+useful) to measure precision"
    );
}
