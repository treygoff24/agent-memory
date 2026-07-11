//! Planning phase: discover sources, parse, resolve per-cwd project scopes,
//! apply state-file dedup, and topologically sort the resulting actions by
//! wiki-link dependency. Holds `ImportEngine::plan` and `topo_sort`. Moved
//! verbatim from the former single-file `pipeline.rs`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::import::candidate::{compute_identity, short_hash, ParsedMemory};
use crate::import::discovery::{discover_claude_memory_roots, discover_codex_memory_root};
use crate::import::project_map::{ProjectMapper, PromptBackend, ResolutionKind, ScopeBinding};
use crate::import::sources::{candidate_aliases, claude, codex};
use crate::import::state::ImportRecord;
use crate::import::{ImportError, ImportResult};

use super::execute::plan_action_for_record;
use super::model::{
    DiscoverySummary, HarnessFilter, ImportOptions, ImportPlan, PlanAction, PlannedWrite, WikiLinkBackEdge,
};
use super::ImportEngine;

impl ImportEngine {
    /// Planning phase. Discovers sources, parses, asks the project mapper for
    /// each unique non-git cwd, applies state-file dedup, topologically sorts
    /// the resulting actions by wiki-link dependency.
    pub async fn plan(&self, options: ImportOptions, prompts: &mut dyn PromptBackend) -> ImportResult<ImportPlan> {
        let mut parse_errors = Vec::new();
        let mut candidates: Vec<ParsedMemory> = Vec::new();

        // Pass 0: discovery. Claude resolves to the UNION of profile roots
        // (the precedence root plus any sibling `.claude*/projects`), so a
        // multi-profile machine is covered without the operator naming each
        // root. Codex stays single-root.
        let claude_roots = if options.harness_filter.is_none_or(|f| matches!(f, HarnessFilter::Claude)) {
            discover_claude_memory_roots(&options.from_claude)?
        } else {
            Vec::new()
        };
        let codex_root = if options.harness_filter.is_none_or(|f| matches!(f, HarnessFilter::Codex)) {
            discover_codex_memory_root(options.from_codex.as_deref())?
        } else {
            None
        };

        // Pass 1: parse. Parse each Claude root, concatenate candidates, then
        // dedup by `source_key` (the relative path under the root) so a memory
        // reachable through more than one profile symlink is imported once.
        // First occurrence wins, and roots are listed precedence-first, so the
        // precedence root's copy is the survivor.
        let mut claude_root_summaries: Vec<(PathBuf, usize)> = Vec::with_capacity(claude_roots.len());
        let mut claude_candidates: Vec<ParsedMemory> = Vec::new();
        let mut frontmatter_recovered: Vec<String> = Vec::new();
        for root in &claude_roots {
            let output = claude::parse(&root.path)?;
            claude_root_summaries.push((root.path.clone(), output.candidates.len()));
            claude_candidates.extend(output.candidates);
            parse_errors.extend(output.errors);
            frontmatter_recovered.extend(output.recovered);
        }
        // The canonical-path dedup below collapses a memory reached through
        // multiple profile symlinks; dedup the recovered keys the same way so a
        // memory recovered through two roots is reported once.
        frontmatter_recovered.sort();
        frontmatter_recovered.dedup();
        let claude_roots_used: Vec<String> =
            claude_root_summaries.iter().map(|(path, _)| path.display().to_string()).collect();
        // Collapse the SAME backing file reached through multiple profile
        // symlinks (identical canonical path) but keep genuinely distinct files
        // that happen to share a relative source_key across non-shared profiles,
        // and keep decomposed sections of one file (same path, distinct
        // source_key). Keying on source_key alone would silently drop the
        // second of two same-named-but-different memories from separate
        // profiles; keying on canonical path alone would collapse a dossier's
        // sections. Canonicalize so symlinked roots resolve to one shared store.
        let mut seen_claude: HashSet<(PathBuf, String)> = HashSet::new();
        claude_candidates.retain(|candidate| {
            let canonical =
                std::fs::canonicalize(&candidate.source_path).unwrap_or_else(|_| candidate.source_path.clone());
            seen_claude.insert((canonical, candidate.source_key.clone()))
        });

        // Disambiguate collisions within the same source file. Must happen
        // before canonical-path dedup? No, canonical dedup doesn't depend on
        // disambiguation; it uses source_key. We do it before identity matching.
        crate::import::candidate::disambiguate_collisions(&mut claude_candidates);

        let claude_count = claude_candidates.len();
        candidates.extend(claude_candidates);

        let codex_count = if let Some(root) = &codex_root {
            let output = codex::parse(&root.path)?;
            let count = output.candidates.len();
            candidates.extend(output.candidates);
            parse_errors.extend(output.errors);
            count
        } else {
            0
        };

        crate::import::candidate::disambiguate_collisions(&mut candidates);

        if !options.quiet && !claude_roots.is_empty() {
            let roots_summary = claude_root_summaries
                .iter()
                .map(|(path, count)| format!("{} ({count})", path.display()))
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "import: discovered {} Claude root(s): {roots_summary}; {claude_count} candidate(s) after source-key dedup",
                claude_roots.len()
            );
        }

        // Pass 2: per-cwd project mapping. Walk unique cwds in deterministic
        // order so prompt order is stable across runs.
        let mut mapper = ProjectMapper::new();
        let mut cwd_to_scope: HashMap<Option<PathBuf>, ScopeBinding> = HashMap::new();
        let mut ordered_cwds: Vec<Option<PathBuf>> = Vec::new();
        let mut seen_cwds: HashSet<Option<PathBuf>> = HashSet::new();
        for candidate in &candidates {
            if seen_cwds.insert(candidate.cwd.clone()) {
                ordered_cwds.push(candidate.cwd.clone());
            }
        }
        for cwd in ordered_cwds {
            let scope = mapper.resolve(cwd.as_deref(), prompts).await.map_err(|error| ImportError::Parse {
                source_key: cwd.as_deref().map_or("<none>".to_string(), |c| c.display().to_string()),
                reason: format!("project mapping: {error}"),
            })?;
            cwd_to_scope.insert(cwd, scope);
        }

        // Pass 3: state-file dedup. Determine each candidate's action.
        let state = options.state;
        let mut prelim: Vec<PlannedWrite> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let scope = cwd_to_scope.get(&candidate.cwd).cloned().unwrap_or_else(|| ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                namespace_alias: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            });
            let action = if matches!(scope.resolution, ResolutionKind::PromptedSkip) {
                PlanAction::SkipByPrompt
            } else {
                let identity = candidate.import_identity(scope.canonical_namespace_id.as_deref());
                let anchor = candidate.recovered_memory_id();
                let canonical_project_id = scope.canonical_namespace_id.as_deref().unwrap_or("me");
                let mut matches: Vec<(&String, &ImportRecord)> = Vec::new();
                for (record_key, record) in &state.imports {
                    if record_identity_matches(record_key, record, identity.as_str(), canonical_project_id)
                        || anchor.is_some_and(|id| {
                            record.source_memory_id.as_deref() == Some(id)
                                && record.harness == candidate.harness.as_str()
                        })
                    {
                        matches.push((record_key, record));
                    }
                }
                // Schema-v1 compatibility: if a legacy record is keyed by the
                // exact source_key, include it as a fallback even when the
                // ordinal-free identity changed.
                if let Some(record) = state.imports.get(&candidate.source_key) {
                    if !matches.iter().any(|(_, matched)| matched.memory_id == record.memory_id) {
                        matches.push((&candidate.source_key, record));
                    }
                }
                matches.sort_by(|a, b| a.1.memory_id.cmp(&b.1.memory_id));
                matches.dedup_by(|a, b| a.1.memory_id == b.1.memory_id);
                match matches.as_slice() {
                    [] => PlanAction::WriteNew,
                    [(record_key, record)] => {
                        plan_action_for_record(record, record_key, &candidate.content_hash, &scope)
                    }
                    records => PlanAction::ReportAmbiguous {
                        matching_memory_ids: records.iter().map(|(_, record)| record.memory_id.clone()).collect(),
                    },
                }
            };
            prelim.push(PlannedWrite {
                source_key: candidate.source_key.clone(),
                candidate,
                scope,
                action,
                wiki_link_targets_resolvable: Vec::new(),
                wiki_link_targets_back_edge: Vec::new(),
            });
        }

        // Pass 4: topological sort by wiki-link dependency. Edges go from
        // source memory → wiki-link target memory; we sort so each write
        // happens after its dependencies. Back-edges in cycles get marked
        // and the alias is preserved as inert text in the body.
        let (actions, back_edges) = topo_sort(prelim);

        Ok(ImportPlan {
            actions,
            source_discovery_summary: DiscoverySummary {
                // The report's single-root field carries the precedence root
                // (first in the union); the full per-root breakdown is the
                // stderr summary above.
                claude_root: claude_roots.into_iter().next(),
                codex_root,
                claude_candidates: claude_count,
                codex_candidates: codex_count,
            },
            unresolved_back_edges: back_edges,
            parse_errors,
            frontmatter_recovered,
            claude_roots_used,
            state,
        })
    }
}

/// Whether the record's identity could plausibly match the candidate. The
/// record's stored `source_identity` is checked first, and then the identity is
/// recomputed both WITH and WITHOUT the content-hash suffix to survive a
/// suffix toggle (e.g. a sibling was deleted and the remaining section
/// renumbered).
///
/// F21: the recomputation uses the RECORD's persisted canonical namespace id —
/// never the candidate's — so a project-A record can never match a project-B
/// candidate and trigger a wrong-memory supersede. Legacy records that predate
/// the persisted field fall back to the candidate's project id (the historical
/// behavior; the exact-source-key compat path still covers them).
fn record_identity_matches(
    record_key: &str,
    record: &ImportRecord,
    candidate_identity: &str,
    candidate_project_id: &str,
) -> bool {
    if !record.source_identity.is_empty() && record.source_identity == candidate_identity {
        return true;
    }

    let source_key = if record.source_key.is_empty() { record_key } else { &record.source_key };
    if source_key.is_empty() || source_key.starts_with("tuple:") {
        return false;
    }

    let record_project_id = record.canonical_namespace_id.as_deref().unwrap_or(candidate_project_id);

    let base = compute_identity(source_key, &record.source_path_at_import, &record.harness, record_project_id, None);
    if base == candidate_identity {
        return true;
    }

    let suffixed = compute_identity(
        source_key,
        &record.source_path_at_import,
        &record.harness,
        record_project_id,
        Some(short_hash(&record.content_hash)),
    );
    suffixed == candidate_identity
}

/// Topological sort over wiki-link dependencies. Returns the sorted action list
/// (each write follows its wiki-link targets) and a list of back-edges that
/// were broken to resolve cycles.
pub(super) fn topo_sort(actions: Vec<PlannedWrite>) -> (Vec<PlannedWrite>, Vec<WikiLinkBackEdge>) {
    // Build an alias → source_key index. Aliases come from each candidate's
    // title (for Codex Task Groups: the header; for Claude: the topic name)
    // plus the candidate's source_key suffix. We index by lowercased alias so
    // wiki-link matching is case-insensitive.
    let mut alias_to_key: HashMap<String, String> = HashMap::new();
    let mut sorted_keys: Vec<String> = actions.iter().map(|w| w.source_key.clone()).collect();
    sorted_keys.sort();
    let key_order: HashMap<String, usize> = sorted_keys.iter().enumerate().map(|(i, k)| (k.clone(), i)).collect();
    // Aliases come from each candidate's title (for Codex Task Groups: the
    // header; for Claude: the topic name) plus the short source-key segment and
    // its stem, so `[[file.md]]`-style links resolve against file-named
    // candidates. `PlannedWrite::source_key` mirrors `candidate.source_key`, so
    // the shared deriver yields the same short/stem segments this loop did.
    for write in &actions {
        for alias in candidate_aliases(&write.candidate) {
            alias_to_key.entry(alias).or_insert_with(|| write.source_key.clone());
        }
    }

    // Edges: source_key → set of target source_keys it depends on.
    let mut deps: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut resolvable_aliases: HashMap<String, BTreeMap<String, String>> = HashMap::new();
    let mut back_edge_aliases: HashMap<String, Vec<String>> = HashMap::new();
    let mut back_edges = Vec::new();

    for write in &actions {
        let from_key = write.source_key.clone();
        let from_order = key_order.get(&from_key).copied().unwrap_or(usize::MAX);
        for alias in &write.candidate.wiki_links {
            let lowered = alias.to_ascii_lowercase();
            if let Some(target_key) = alias_to_key.get(&lowered).cloned() {
                if target_key == from_key {
                    // self-link; treat as back-edge so we don't loop
                    back_edge_aliases.entry(from_key.clone()).or_default().push(alias.clone());
                    back_edges.push(WikiLinkBackEdge { source_key: from_key.clone(), alias: alias.clone() });
                    continue;
                }
                let target_order = key_order.get(&target_key).copied().unwrap_or(usize::MAX);
                if from_order < target_order {
                    // Forward edge: write must come after the target.
                    deps.entry(from_key.clone()).or_default().insert(target_key.clone());
                    resolvable_aliases.entry(from_key.clone()).or_default().insert(alias.clone(), target_key);
                } else {
                    // Back-edge in source-key order — break deterministically.
                    back_edge_aliases.entry(from_key.clone()).or_default().push(alias.clone());
                    back_edges.push(WikiLinkBackEdge { source_key: from_key.clone(), alias: alias.clone() });
                }
            }
            // Unresolvable aliases (no candidate to link to) stay as inert
            // body text — they're not back-edges, they're just dangling.
        }
    }

    // Kahn's algorithm: in-degree sort over the forward-edge DAG. Tiebreak
    // ties by source-key for determinism.
    let mut in_degree: BTreeMap<String, usize> = actions.iter().map(|w| (w.source_key.clone(), 0)).collect();
    let mut reverse_adj: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (from, targets) in &deps {
        for target in targets {
            *in_degree.entry(from.clone()).or_default() += 1;
            reverse_adj.entry(target.clone()).or_default().insert(from.clone());
        }
    }

    let mut queue: VecDeque<String> = in_degree.iter().filter(|(_, &deg)| deg == 0).map(|(k, _)| k.clone()).collect();
    let mut sorted_order: Vec<String> = Vec::with_capacity(actions.len());
    while let Some(key) = queue.pop_front() {
        sorted_order.push(key.clone());
        let dependents = reverse_adj.remove(&key).unwrap_or_default();
        let mut newly_zero: Vec<String> = Vec::new();
        for dependent in dependents {
            let entry = in_degree.entry(dependent.clone()).or_default();
            if *entry > 0 {
                *entry -= 1;
            }
            if *entry == 0 {
                newly_zero.push(dependent);
            }
        }
        newly_zero.sort();
        for next in newly_zero {
            queue.push_back(next);
        }
    }

    // Any unconsumed nodes form a cycle. Walk them in source-key order, mark
    // their lowest-index incoming edge as a back-edge, and add them last.
    if sorted_order.len() < actions.len() {
        let remaining: Vec<String> =
            actions.iter().map(|w| w.source_key.clone()).filter(|k| !sorted_order.contains(k)).collect();
        for key in remaining {
            // Find the back-edge to break: a dep this key still has whose
            // target is also unfinished.
            if let Some(targets) = deps.get(&key) {
                for target in targets {
                    if !sorted_order.contains(target) {
                        back_edge_aliases.entry(key.clone()).or_default().push(format!("<cycle:{target}>"));
                        back_edges
                            .push(WikiLinkBackEdge { source_key: key.clone(), alias: format!("<cycle:{target}>") });
                    }
                }
            }
            sorted_order.push(key);
        }
    }

    let mut by_key: HashMap<String, PlannedWrite> = actions.into_iter().map(|w| (w.source_key.clone(), w)).collect();
    let mut output = Vec::with_capacity(sorted_order.len());
    for key in sorted_order {
        if let Some(mut write) = by_key.remove(&key) {
            let resolvable = resolvable_aliases.remove(&write.source_key).unwrap_or_default();
            write.wiki_link_targets_resolvable = resolvable.into_keys().collect();
            write.wiki_link_targets_back_edge = back_edge_aliases.remove(&write.source_key).unwrap_or_default();
            output.push(write);
        }
    }
    (output, back_edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::candidate::{Harness, ParsedMemory};
    use crate::import::state::ImportRecord;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn codex_candidate(source_key: &str, body: &str) -> ParsedMemory {
        let mut hint = BTreeMap::new();
        hint.insert("name".to_string(), serde_json::Value::String("foo".to_string()));
        let content_hash = ParsedMemory::compute_content_hash(&hint, body);
        ParsedMemory {
            source_key: source_key.to_string(),
            source_path: PathBuf::from("/u/.codex/memories/MEMORY.md"),
            content_hash,
            harness: Harness::Codex,
            frontmatter_hint: hint,
            body: body.to_string(),
            wiki_links: Vec::new(),
            cwd: None,
            title: Some("foo".to_string()),
            section_disambiguation: None,
        }
    }

    fn record_with_identity(source_identity: &str, content_hash: &str, memory_id: &str) -> ImportRecord {
        ImportRecord {
            source_identity: source_identity.to_string(),
            source_key: "codex:memories/MEMORY.md#task-group-2-foo".to_string(),
            source_memory_id: None,
            memory_id: memory_id.to_string(),
            content_hash: content_hash.to_string(),
            imported_at: Utc::now(),
            harness: "codex".to_string(),
            source_path_at_import: PathBuf::from("/u/.codex/memories/MEMORY.md"),
            namespace: Some("me".to_string()),
            canonical_namespace_id: None,
            aliases: Vec::new(),
            supersession_chain: Vec::new(),
        }
    }

    #[test]
    fn record_identity_matches_after_suffix_toggle_and_renumbered_edit() {
        // A sibling was deleted, the remaining section was renumbered, and its
        // content edited. The new candidate has no suffix; the old record is
        // keyed with the suffixed identity. Matching must fall back to the
        // ordinal-free base identity.
        let candidate = codex_candidate("codex:memories/MEMORY.md#task-group-1-foo", "new body");
        let candidate_identity = candidate.import_identity(None);

        let old_hash = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
        let mut record = record_with_identity("", old_hash, "mem_old");
        record.source_identity = format!("{}-22222222", candidate_identity);

        assert!(
            record_identity_matches("old-key", &record, &candidate_identity, "me"),
            "suffix-toggle record should match candidate base identity"
        );
    }

    #[test]
    fn record_identity_matches_multiple_records_marks_ambiguous() {
        let candidate = codex_candidate("codex:memories/MEMORY.md#task-group-1-foo", "body");
        let candidate_identity = candidate.import_identity(None);

        let record_a = record_with_identity(
            &candidate_identity,
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            "mem_a",
        );
        let record_b = record_with_identity(
            "",
            "sha256:2222222222222222222222222222222222222222222222222222222222222222",
            "mem_b",
        );

        assert!(record_identity_matches("key_a", &record_a, &candidate_identity, "me"));
        assert!(record_identity_matches("key_b", &record_b, &candidate_identity, "me"));
    }

    /// F21: the legacy-record identity recompute must use the RECORD's
    /// persisted canonical namespace id, never the candidate's — otherwise a
    /// project-A record matches a same-shaped project-B candidate and the
    /// import supersedes the wrong memory.
    #[test]
    fn record_identity_recompute_uses_record_namespace_not_candidates() {
        let candidate = codex_candidate("codex:memories/MEMORY.md#task-group-1-foo", "body");
        let candidate_identity = candidate.import_identity(Some("proj_b"));

        let mut record = record_with_identity(
            "",
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            "mem_a",
        );

        record.canonical_namespace_id = Some("proj_a".to_string());
        assert!(
            !record_identity_matches("old-key", &record, &candidate_identity, "proj_b"),
            "a project-A record must not match a project-B candidate"
        );

        record.canonical_namespace_id = Some("proj_b".to_string());
        assert!(
            record_identity_matches("old-key", &record, &candidate_identity, "proj_b"),
            "same-project record still matches"
        );

        // Legacy record with no persisted namespace: falls back to the
        // candidate's project id (historical behavior, deliberately kept).
        record.canonical_namespace_id = None;
        assert!(
            record_identity_matches("old-key", &record, &candidate_identity, "proj_b"),
            "legacy record without a persisted namespace keeps the historical fallback"
        );
    }
}
