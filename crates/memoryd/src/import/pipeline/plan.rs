//! Planning phase: discover sources, parse, resolve per-cwd project scopes,
//! apply state-file dedup, and topologically sort the resulting actions by
//! wiki-link dependency. Holds `ImportEngine::plan` and `topo_sort`. Moved
//! verbatim from the former single-file `pipeline.rs`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::import::candidate::{compute_identity, ParsedMemory, short_hash};
use crate::import::discovery::{discover_claude_memory_roots, discover_codex_memory_root};
use crate::import::state::ImportRecord;
use crate::import::project_map::{ProjectMapper, PromptBackend, ResolutionKind, ScopeBinding};
use crate::import::sources::{candidate_aliases, claude, codex};
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
                    if record_identity(record_key, record, &candidate, canonical_project_id).as_deref()
                        == Some(identity.as_str())
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
                    [(record_key, record)] => plan_action_for_record(record, record_key, &candidate.content_hash, &scope),
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

fn record_identity(
    record_key: &str,
    record: &ImportRecord,
    candidate: &ParsedMemory,
    canonical_project_id: &str,
) -> Option<String> {
    if !record.source_identity.is_empty() {
        return Some(record.source_identity.clone());
    }
    let source_key = if record.source_key.is_empty() { record_key } else { &record.source_key };
    if source_key.is_empty() || source_key.starts_with("tuple:") {
        return None;
    }
    let section_disambiguation =
        candidate.section_disambiguation.as_deref().map(|_| short_hash(&record.content_hash));
    Some(compute_identity(
        source_key,
        &record.source_path_at_import,
        &record.harness,
        canonical_project_id,
        section_disambiguation,
    ))
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
