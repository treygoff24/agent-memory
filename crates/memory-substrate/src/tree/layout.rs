//! Tree layout helpers.

use std::io;
use std::path::{Path, PathBuf};

/// Directory marker that identifies a repo as an initialized Memorum substrate.
pub const SUBSTRATE_MARKER_DIR: &str = ".memorum";
const SUBSTRATE_MARKER_FILE: &str = "substrate";

/// Canonical `.gitattributes` body emitted by `git::init` per spec §13.1 step 2.
/// The driver name `memory-merge-driver` is the value resolved by
/// `decisions/open-questions-resolved.md` Q3.
const GITATTRIBUTES_BODY: &str = "* text eol=lf\n\
*.md merge=memory-merge-driver\n\
events/*.jsonl merge=union\n\
substrate/**/*.jsonl merge=memory-merge-driver\n\
encrypted/substrate/**/*.jsonl merge=memory-merge-driver\n\
dreams/questions/**/*.jsonl merge=memory-merge-driver\n\
dreams/cleanup/**/*.json merge=memory-merge-driver\n\
dreams/journal/**/*.md merge=memory-merge-driver\n\
leases/journal.lease merge=memory-merge-driver\n\
tombstones/*.jsonl merge=union\n\
sources/web/**/manifest.json merge=memory-merge-driver\n\
sources/web/**/excerpts.jsonl merge=memory-merge-driver\n\
sources/web/**/extracted.txt merge=memory-merge-driver\n\
sources/web/**/raw.bin.zst binary\n\
sources/web/**/extracted.enc.age binary\n\
sources/web/**/raw.enc.age binary\n";

const MANAGED_GITATTRIBUTES_PATTERNS: &[&str] = &[
    "*",
    "*.md",
    "events/*.jsonl",
    "substrate/**/*.jsonl",
    "encrypted/substrate/**/*.jsonl",
    "dreams/questions/**/*.jsonl",
    "dreams/cleanup/**/*.json",
    "dreams/journal/**/*.md",
    "leases/journal.lease",
    "tombstones/*.jsonl",
    "sources/web/**/manifest.json",
    "sources/web/**/excerpts.jsonl",
    "sources/web/**/extracted.txt",
    "sources/web/**/raw.bin.zst",
    "sources/web/**/extracted.enc.age",
    "sources/web/**/raw.enc.age",
];

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

/// Create tree directories, tracked bootstrap files, and a synthetic synced
/// config when one is absent.
pub fn bootstrap_repo_tree(root: &Path) -> std::io::Result<()> {
    bootstrap_repo_layout(root)?;
    if !root.join("config.yaml").exists() {
        std::fs::write(
            root.join("config.yaml"),
            "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\n",
        )?;
    }
    Ok(())
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
    reconciled.push_str(GITATTRIBUTES_BODY);

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
    (!MANAGED_GITATTRIBUTES_PATTERNS.contains(&pattern)).then(|| line.to_string())
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
