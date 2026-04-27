//! Tree layout helpers.

use std::path::{Path, PathBuf};

/// Canonical `.gitattributes` body emitted by `git::init` per spec §13.1 step 2.
/// The driver name `memory-merge-driver` is the value resolved by
/// `decisions/open-questions-resolved.md` Q3.
const GITATTRIBUTES_BODY: &str = "* text eol=lf\n\
*.md merge=memory-merge-driver\n\
events/*.jsonl merge=union\n\
substrate/**/*.jsonl merge=union\n\
tombstones/*.jsonl merge=union\n";

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
        "dreams/reports",
        "substrate",
        "encrypted",
        "tombstones",
        "events",
        "policies",
        "leases",
    ]
}

/// Create tree directories and tracked bootstrap files, but do not seed
/// synced config.
pub fn bootstrap_repo_layout(root: &Path) -> std::io::Result<()> {
    for dir in memory_dirs() {
        std::fs::create_dir_all(root.join(dir))?;
    }
    write_if_missing(&root.join(".gitattributes"), GITATTRIBUTES_BODY)?;
    write_if_missing(&root.join(".gitignore"), "/.memoryd/\n*.sqlite\n*.sqlite-wal\n*.sqlite-shm\n/.*.tmp\n")?;
    for dir in ["events", "policies", "leases"] {
        write_if_missing(&root.join(dir).join(".keep"), "")?;
    }
    Ok(())
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

/// Return markdown memory paths under a root.
pub fn relative_memory_paths(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                path.strip_prefix(root).ok().map(Path::to_path_buf)
            } else {
                None
            }
        })
        .collect()
}
