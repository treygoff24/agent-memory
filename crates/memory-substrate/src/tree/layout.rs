//! Tree layout helpers.

use std::io;
use std::path::{Path, PathBuf};

/// Directory marker that identifies a repo as an initialized Memorum substrate.
pub const SUBSTRATE_MARKER_DIR: &str = ".memorum";
const SUBSTRATE_MARKER_FILE: &str = "substrate";

/// Single source of truth for the substrate-managed `.gitattributes` rules:
/// the `(pattern, attributes)` pairs `git::init` emits (spec §13.1 step 2) and
/// the patterns `reconcile_gitattributes` treats as substrate-owned. The driver
/// name `memory-merge-driver` is the value resolved by
/// `decisions/open-questions-resolved.md` Q3.
///
/// The emitted body ([`gitattributes_body`]) and the managed-pattern check
/// ([`is_managed_gitattributes_pattern`]) are both derived from this one table,
/// so routing a new path is a single edit that cannot desync the writer from the
/// reconciler. Order is load-bearing: it fixes the emitted byte sequence.
const MANAGED_GITATTRIBUTES: &[(&str, &str)] = &[
    ("*", "text eol=lf"),
    ("*.md", "merge=memory-merge-driver"),
    ("events/*.jsonl", "merge=union"),
    ("substrate/**/*.jsonl", "merge=memory-merge-driver"),
    ("encrypted/substrate/**/*.jsonl", "merge=memory-merge-driver"),
    ("dreams/questions/**/*.jsonl", "merge=memory-merge-driver"),
    ("dreams/cleanup/**/*.json", "merge=memory-merge-driver"),
    ("dreams/journal/**/*.md", "merge=memory-merge-driver"),
    ("dreams/calibration/*.jsonl", "merge=union"),
    ("leases/journal.lease", "merge=memory-merge-driver"),
    ("tombstones/*.jsonl", "merge=union"),
    ("sources/web/**/manifest.json", "merge=memory-merge-driver"),
    ("sources/web/**/excerpts.jsonl", "merge=memory-merge-driver"),
    ("sources/web/**/extracted.txt", "merge=memory-merge-driver"),
    ("sources/web/**/raw.bin.zst", "binary"),
    ("sources/web/**/extracted.enc.age", "binary"),
    ("sources/web/**/raw.enc.age", "binary"),
];

/// Patterns `reconcile_gitattributes` treats as substrate-managed. Derived from
/// [`MANAGED_GITATTRIBUTES`] so it can never drift from the emitted body.
fn is_managed_gitattributes_pattern(pattern: &str) -> bool {
    MANAGED_GITATTRIBUTES.iter().any(|(managed, _)| *managed == pattern)
}

/// Canonical `.gitattributes` body emitted by `git::init` per spec §13.1 step 2.
///
/// Rendered from [`MANAGED_GITATTRIBUTES`] as `"{pattern} {attributes}\n"` per
/// row, preserving the exact byte sequence the merge-driver routing depends on.
fn gitattributes_body() -> String {
    let mut body = String::new();
    for (pattern, attributes) in MANAGED_GITATTRIBUTES {
        body.push_str(pattern);
        body.push(' ');
        body.push_str(attributes);
        body.push('\n');
    }
    body
}

/// Directories that should exist after init/adoption.
pub fn memory_dirs() -> Vec<&'static str> {
    vec![
        "me/identity",
        "me/relationship/facts",
        "me/relationship/preferences",
        "me/relationship/corrections",
        "me/relationship/patterns",
        "me/knowledge",
        "me/episodic",
        "me/prospective",
        "projects",
        "agent/patterns",
        "agent/playbooks",
        "agent/postmortems",
        "agent/anti-patterns",
        "agent/heuristics",
        "agent/regressions",
        "agent/episodic",
        "dreams/journal",
        "dreams/questions",
        "dreams/cleanup",
        "dreams/reports",
        "substrate",
        "encrypted",
        "encrypted/substrate",
        "tombstones",
        "events",
        "policies",
        "leases",
        "sources",
        "sources/web",
    ]
}

/// Create tree directories and tracked bootstrap files, but do not seed
/// synced config.
pub fn bootstrap_repo_layout(root: &Path) -> std::io::Result<()> {
    write_substrate_marker(root)?;
    for dir in memory_dirs() {
        std::fs::create_dir_all(root.join(dir))?;
    }
    reconcile_gitattributes(&root.join(".gitattributes"))?;
    reconcile_gitignore(&root.join(".gitignore"))?;
    for dir in ["events", "policies", "leases"] {
        write_if_missing(&root.join(dir).join(".keep"), "")?;
    }
    Ok(())
}

/// Return true when `root` has the explicit Memorum substrate marker.
pub fn has_substrate_marker(root: &Path) -> bool {
    root.join(SUBSTRATE_MARKER_DIR).join(SUBSTRATE_MARKER_FILE).is_file()
}

fn write_substrate_marker(root: &Path) -> std::io::Result<()> {
    let marker_dir = root.join(SUBSTRATE_MARKER_DIR);
    std::fs::create_dir_all(&marker_dir)?;
    write_if_missing(&marker_dir.join(SUBSTRATE_MARKER_FILE), "schema_version: 1\nkind: memorum-substrate\n")
}

/// Default active-embedding triple written into a fresh `config.yaml`.
///
/// `(provider, model_ref, dimension)` is the unit of vector-table identity
/// (spec §10.2.2 #9). This default is the production embedding lane shipped by
/// Stream B: Qwen3-Embedding-0.6B served locally via the fastembed candle
/// backend (Apache 2.0, 1024-dim). See the dated amendment block in
/// `docs/specs/stream-a-core-substrate-v1.1.md` for the rationale behind
/// superseding the spec's literal `embeddinggemma-300m-qat-Q8_0 / 768` default.
///
/// Stored in a single place so the bootstrap path, the embedding provider, and
/// tests agree on one canonical triple rather than re-spelling the string.
pub const DEFAULT_ACTIVE_EMBEDDING_PROVIDER: &str = "fastembed-candle";
/// Default active-embedding model reference (HF repo id).
pub const DEFAULT_ACTIVE_EMBEDDING_MODEL_REF: &str = "Qwen/Qwen3-Embedding-0.6B";
/// Default active-embedding dimension.
pub const DEFAULT_ACTIVE_EMBEDDING_DIMENSION: u32 = 1024;

/// Create tree directories, tracked bootstrap files, and a synced config
/// carrying the production default embedding triple when one is absent.
pub fn bootstrap_repo_tree(root: &Path) -> std::io::Result<()> {
    bootstrap_repo_layout(root)?;
    if !root.join("config.yaml").exists() {
        std::fs::write(root.join("config.yaml"), default_config_yaml())?;
    }
    Ok(())
}

/// The `config.yaml` body written for a fresh substrate.
///
/// Pins the production default embedding triple (spec §10.2.2 #2 amendment).
/// `active_embedding` is never optional and has no silent fallback: a substrate
/// initialized without it cannot determine which vector table to enqueue
/// pending embedding jobs against.
fn default_config_yaml() -> String {
    format!(
        "schema_version: 1\nactive_embedding:\n  provider: {DEFAULT_ACTIVE_EMBEDDING_PROVIDER}\n  model_ref: {DEFAULT_ACTIVE_EMBEDDING_MODEL_REF}\n  dimension: {DEFAULT_ACTIVE_EMBEDDING_DIMENSION}\n",
    )
}

fn write_if_missing(path: &Path, contents: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents)
}

/// Substrate-managed `.gitignore` entries. The substrate writes runtime state under
/// these paths on its own and the user is never expected to commit them. Each entry
/// must be present in every substrate-initialized repo, including ones initialized
/// before a given entry was added to this list — that's the migration responsibility
/// of `reconcile_gitignore`. Don't rely on `write_if_missing` for this; existing
/// repos created before the Memorum rebrand (when `/.memorum/` was added here)
/// would otherwise keep leaking substrate state into `git status` forever.
const MANAGED_GITIGNORE_ENTRIES: &[&str] =
    &["/.memoryd/", "/.memorum/", "*.sqlite", "*.sqlite-wal", "*.sqlite-shm", "/.*.tmp"];

fn reconcile_gitignore(path: &Path) -> std::io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };

    let existing_entries: std::collections::HashSet<&str> =
        existing.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).collect();
    let missing: Vec<&&str> =
        MANAGED_GITIGNORE_ENTRIES.iter().filter(|entry| !existing_entries.contains(**entry)).collect();
    if missing.is_empty() && !existing.is_empty() {
        return Ok(());
    }

    let mut reconciled = existing;
    if !reconciled.is_empty() && !reconciled.ends_with('\n') {
        reconciled.push('\n');
    }
    for entry in missing {
        reconciled.push_str(entry);
        reconciled.push('\n');
    }
    std::fs::write(path, reconciled)
}

fn reconcile_gitattributes(path: &Path) -> std::io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };

    let mut reconciled = String::new();
    for line in existing.split_inclusive('\n') {
        if let Some(line) = reconcile_existing_gitattributes_line(line) {
            reconciled.push_str(&line);
        }
    }
    if !reconciled.is_empty() && !reconciled.ends_with('\n') {
        reconciled.push('\n');
    }
    reconciled.push_str(&gitattributes_body());

    if existing != reconciled {
        std::fs::write(path, reconciled)?;
    }
    Ok(())
}

fn reconcile_existing_gitattributes_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Some(line.to_string());
    }
    let mut fields = trimmed.split_whitespace();
    let Some(pattern) = fields.next() else {
        return Some(line.to_string());
    };
    if pattern == "*" {
        return reconcile_global_gitattributes_line(line, fields);
    }
    (!is_managed_gitattributes_pattern(pattern)).then(|| line.to_string())
}

fn reconcile_global_gitattributes_line(line: &str, attributes: std::str::SplitWhitespace<'_>) -> Option<String> {
    let attributes = attributes.collect::<Vec<_>>();
    if attributes.iter().all(|attribute| !is_managed_global_attribute(attribute)) {
        return Some(line.to_string());
    }
    let unmanaged_attributes =
        attributes.into_iter().filter(|attribute| !is_managed_global_attribute(attribute)).collect::<Vec<_>>();
    (!unmanaged_attributes.is_empty()).then(|| format!("* {}\n", unmanaged_attributes.join(" ")))
}

fn is_managed_global_attribute(attribute: &str) -> bool {
    let name = attribute
        .trim_start_matches(['-', '!'])
        .split_once('=')
        .map_or(attribute.trim_start_matches(['-', '!']), |(name, _)| name);
    matches!(name, "text" | "eol")
}

/// Return markdown memory paths under a root.
pub fn relative_memory_paths(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            path.strip_prefix(root)
                .ok()
                .filter(|relative| is_canonical_memory_markdown_path(relative))
                .map(Path::to_path_buf)
        })
        .collect()
}

fn is_canonical_memory_markdown_path(relative: &Path) -> bool {
    if relative.extension().is_none_or(|ext| ext != "md") {
        return false;
    }
    let rel = relative.to_string_lossy().replace('\\', "/");
    !rel.starts_with("dreams/journal/") && !rel.starts_with("sources/web/")
}
