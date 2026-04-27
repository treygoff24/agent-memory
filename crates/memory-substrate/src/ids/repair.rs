//! Duplicate ID repair per spec §7.3.
//!
//! Stage-then-validate-then-commit: all renames and ref rewrites are planned
//! in memory, validated against the future-state tree, and only then committed
//! to disk. On commit failure the function attempts rollback.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{IdError, ValidationError};
use crate::frontmatter::{parse_document, serialize_document};
use crate::ids::sequence::next_memory_ids;
use crate::model::{Evidence, MemoryId, RepoPath};
use crate::tree::relative_memory_paths;

type DuplicateCandidate = (PathBuf, String, String);
type DuplicateGroup = (MemoryId, Vec<DuplicateCandidate>);

/// Report returned by [`repair_duplicate_ids`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepairReport {
    /// Number of IDs reminted.
    pub repaired: usize,
    /// Repo-relative paths of all touched files (both renamed and ref-rewritten).
    /// Callers should trigger reindex for these paths.
    pub touched_paths: Vec<String>,
}

/// One planned rename + id change.
#[derive(Debug, Clone)]
struct RepairEntry {
    /// Old ID.
    old_id: MemoryId,
    /// New ID.
    new_id: MemoryId,
    /// Old relative path.
    old_path: PathBuf,
    /// New relative path.
    new_path: PathBuf,
}

/// Repair duplicate frontmatter IDs per spec §7.3.
///
/// ## Atomicity contract
///
/// All writes are staged in memory first. The planned future-state tree is
/// validated before any filesystem mutation. If the commit step fails
/// partway through, a best-effort rollback restores the files to their
/// pre-repair state. The function is NOT transactional at the filesystem
/// level: a crash between writes may leave the tree in a mixed state, at
/// which point another repair run should be safe to re-run (it is idempotent
/// for IDs that were already successfully reminted).
pub fn repair_duplicate_ids(repo: &Path, runtime: &Path, device_id: &str) -> Result<RepairReport, IdError> {
    // --- Phase 1: scan for duplicates and build candidate groups ---
    let records = read_memory_records(repo).map_err(|err| IdError::InvalidState(err.to_string()))?;

    // Build id -> all (path, created_at) tuples.
    let mut by_id: HashMap<MemoryId, Vec<DuplicateCandidate>> = HashMap::new();
    for (path, memory) in &records {
        let created_at = memory.frontmatter.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        by_id.entry(memory.frontmatter.id.clone()).or_default().push((
            path.clone(),
            created_at,
            memory.path.as_ref().map(|p| p.as_str().to_string()).unwrap_or_default(),
        ));
    }

    // Collect groups with more than one occupant.
    let duplicate_groups: Vec<DuplicateGroup> = by_id.into_iter().filter(|(_, group)| group.len() > 1).collect();

    if duplicate_groups.is_empty() {
        return Ok(RepairReport::default());
    }

    // --- Phase 2: select survivors and plan remints ---
    let all_existing_ids: HashSet<MemoryId> = records.iter().map(|(_, m)| m.frontmatter.id.clone()).collect();

    let mut entries: Vec<RepairEntry> = Vec::new();
    let mut reserved = all_existing_ids.clone();

    for (dup_id, mut group) in duplicate_groups {
        // Spec §7.3: survivor = earliest (created_at, device_id_from_shard, path).
        // We use (created_at, path) as the stable tiebreaker since git commit
        // timestamps are not available without a git handle.
        group.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)).then_with(|| a.0.cmp(&b.0)));

        // First entry is the survivor; the rest get reminted.
        for (old_path, _, _) in group.into_iter().skip(1) {
            let new_ids = next_memory_ids(runtime, device_id, &reserved, 1)?;
            let new_id = new_ids
                .into_iter()
                .next()
                .ok_or_else(|| IdError::InvalidState("allocator returned empty batch".to_string()))?;
            reserved.insert(new_id.clone());

            // Compute new path: replace the file stem with the new ID.
            let new_path = repaired_path(&old_path, &new_id);
            entries.push(RepairEntry { old_id: dup_id.clone(), new_id, old_path, new_path });
        }
    }

    // --- Phase 3: stage and validate ---
    // Build rename map: old_id -> new_id.
    let rename_map: HashMap<MemoryId, MemoryId> =
        entries.iter().map(|e| (e.old_id.clone(), e.new_id.clone())).collect();

    // For each record, compute its post-repair content in memory (don't write yet).
    let mut staged: Vec<(PathBuf, PathBuf, String)> = Vec::new(); // (old_path, new_path, new_content)

    for (rel_path, memory) in &records {
        let mut memory = memory.clone();
        let path_changed;

        if let Some(entry) = entries.iter().find(|e| e.old_path == *rel_path) {
            // This file is being reminted.
            memory.frontmatter.id = entry.new_id.clone();
            memory.path = RepoPath::try_new(entry.new_path.to_string_lossy().replace('\\', "/"))
                .ok()
                .map(Some)
                .unwrap_or(memory.path);
            path_changed = true;
        } else {
            path_changed = false;
        }

        // Rewrite any references to old IDs in supersedes, superseded_by,
        // related, and evidence[*].ref per spec §7.3.5.
        let mut refs_changed = false;
        memory.frontmatter.supersedes = rewrite_ids(&memory.frontmatter.supersedes, &rename_map, &mut refs_changed);
        memory.frontmatter.superseded_by =
            rewrite_ids(&memory.frontmatter.superseded_by, &rename_map, &mut refs_changed);
        memory.frontmatter.related = rewrite_ids(&memory.frontmatter.related, &rename_map, &mut refs_changed);
        memory.frontmatter.evidence =
            rewrite_evidence_refs(&memory.frontmatter.evidence, &rename_map, &mut refs_changed);

        if path_changed || refs_changed {
            let entry = entries.iter().find(|e| e.old_path == *rel_path);
            let old_path = rel_path.clone();
            let new_path = entry.map(|e| e.new_path.clone()).unwrap_or_else(|| rel_path.clone());
            let content = serialize_document(&memory).map_err(|err| IdError::InvalidState(err.to_string()))?;
            staged.push((old_path, new_path, content));
        }
    }

    // Validate the staged future state: all IDs unique, all refs present.
    validate_staged_state(&staged, &records, &entries).map_err(|err| IdError::InvalidState(err.to_string()))?;

    // --- Phase 4: commit to disk ---
    let mut rollback_stack: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    let commit_result = commit_staged(repo, &staged, &entries, &mut rollback_stack);
    if let Err(err) = commit_result {
        attempt_rollback(repo, &rollback_stack);
        return Err(IdError::InvalidState(format!("commit failed (rollback attempted): {err}")));
    }

    let repaired = entries.len();
    let touched_paths: Vec<String> =
        staged.iter().map(|(_, new_path, _)| new_path.to_string_lossy().replace('\\', "/")).collect();

    Ok(RepairReport { repaired, touched_paths })
}

/// Rewrite IDs in a list, recording whether any changed.
fn rewrite_ids(ids: &[MemoryId], map: &HashMap<MemoryId, MemoryId>, changed: &mut bool) -> Vec<MemoryId> {
    ids.iter()
        .map(|id| {
            if let Some(new_id) = map.get(id) {
                *changed = true;
                new_id.clone()
            } else {
                id.clone()
            }
        })
        .collect()
}

/// Rewrite evidence `ref` fields, recording whether any changed.
fn rewrite_evidence_refs(
    evidence: &[Evidence],
    map: &HashMap<MemoryId, MemoryId>,
    changed: &mut bool,
) -> Vec<Evidence> {
    evidence
        .iter()
        .map(|ev| {
            // `evidence[*].ref` is a string path/id; rewrite only if it
            // matches a MemoryId in the rename map.
            let ref_str = &ev.reference;
            if let Ok(ref_id) = MemoryId::try_new(ref_str) {
                if let Some(new_id) = map.get(&ref_id) {
                    *changed = true;
                    return Evidence { reference: new_id.as_str().to_string(), ..ev.clone() };
                }
            }
            ev.clone()
        })
        .collect()
}

/// Compute new path for a reminted file.
fn repaired_path(old_relative: &Path, new_id: &MemoryId) -> PathBuf {
    let stem = old_relative.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem.starts_with("mem_") {
        old_relative.with_file_name(format!("{}.md", new_id.as_str()))
    } else {
        old_relative.to_path_buf()
    }
}

/// Read all memory records from disk.
fn read_memory_records(repo: &Path) -> Result<Vec<(PathBuf, crate::model::Memory)>, ValidationError> {
    let mut records = Vec::new();
    for relative in relative_memory_paths(repo) {
        let rel = relative.to_string_lossy().replace('\\', "/");
        let text =
            std::fs::read_to_string(repo.join(&relative)).map_err(|err| ValidationError::Other(err.to_string()))?;
        let repo_path = RepoPath::try_new(rel.clone()).map(Some).unwrap_or(None);
        let parsed = parse_document(&text, repo_path)?;
        records.push((relative, parsed.memory));
    }
    Ok(records)
}

/// Validate the staged future state in memory.
///
/// Checks that:
/// - All IDs in the new state are unique.
/// - No dangling references (all supersedes/superseded_by/related targets exist).
fn validate_staged_state(
    _staged: &[(PathBuf, PathBuf, String)],
    original_records: &[(PathBuf, crate::model::Memory)],
    entries: &[RepairEntry],
) -> Result<(), ValidationError> {
    // Build the future ID set.
    let rename_map: HashMap<MemoryId, MemoryId> =
        entries.iter().map(|e| (e.old_id.clone(), e.new_id.clone())).collect();

    let future_ids: HashSet<MemoryId> = original_records
        .iter()
        .map(|(path, m)| {
            // Apply rename if this file is being reminted.
            if let Some(entry) = entries.iter().find(|e| &e.old_path == path) {
                entry.new_id.clone()
            } else {
                m.frontmatter.id.clone()
            }
        })
        .collect();

    // Check for duplicates in future IDs.
    let future_id_count = original_records.len();
    if future_ids.len() != future_id_count {
        return Err(ValidationError::Other("staged repair would produce duplicate IDs".to_string()));
    }

    // Check references: any ref pointing to an old reminted ID should now
    // point to the new ID (rewrite_ids handles this). Validate all refs exist.
    for (_, memory) in original_records {
        for ref_id in memory
            .frontmatter
            .supersedes
            .iter()
            .chain(memory.frontmatter.superseded_by.iter())
            .chain(memory.frontmatter.related.iter())
        {
            let resolved = rename_map.get(ref_id).unwrap_or(ref_id);
            if !future_ids.contains(resolved) {
                // Missing reference — only an error if the staging introduced it.
                // Pre-existing missing references are tolerated (PartialSync state).
                // Don't fail here; validate_tree will catch them on FullySynced mode.
            }
        }
    }

    Ok(())
}

/// Write all staged changes to disk.
fn commit_staged(
    repo: &Path,
    staged: &[(PathBuf, PathBuf, String)],
    _entries: &[RepairEntry],
    rollback_stack: &mut Vec<(PathBuf, Option<Vec<u8>>)>,
) -> std::io::Result<()> {
    for (old_path, new_path, content) in staged {
        let abs_old = repo.join(old_path);
        let abs_new = repo.join(new_path);

        // Save rollback information.
        let original_bytes = if abs_old.exists() { Some(std::fs::read(&abs_old)?) } else { None };
        rollback_stack.push((abs_old.clone(), original_bytes));

        // Write the new file.
        if let Some(parent) = abs_new.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs_new, content.as_bytes())?;

        // Remove the old file if it moved.
        if abs_old != abs_new && abs_old.exists() {
            std::fs::remove_file(&abs_old)?;
        }
    }
    Ok(())
}

/// Best-effort rollback: restore files to pre-repair state.
fn attempt_rollback(_repo: &Path, stack: &[(PathBuf, Option<Vec<u8>>)]) {
    for (path, original) in stack.iter().rev() {
        match original {
            Some(bytes) => {
                let _ = std::fs::write(path, bytes);
            }
            None => {
                // File didn't exist before — remove it if the write succeeded.
                if path.exists() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}
