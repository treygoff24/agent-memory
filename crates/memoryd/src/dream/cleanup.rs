use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use chrono::{DateTime, Duration, Utc};
use memory_substrate::events::{decode_line, encode_event_line, read_events, Event, MAX_LINE_BYTES};
use memory_substrate::frontmatter::parse_document;
use memory_substrate::markdown::read_memory_file;
use memory_substrate::tree::relative_memory_paths;
use memory_substrate::{
    ClassificationOutcome, EventContext, Memory, MemoryId, MemoryQuery, MemoryStatus, RepoPath, Sha256, Substrate,
    WriteMode, WriteRequest,
};
use thiserror::Error;

use crate::dream::fragment_archival::archive_with_deferral;
use crate::dream::rehydration::resolve_repo_relative_file_ref;
use crate::dream::report::{
    cleanup_commit_subject, CleanupFinding, CleanupOperationCounts, CleanupReport, CleanupReportInput,
    DeferredFragment, CLEANUP_BOT_AUTHOR,
};
use crate::dynamics::{load_dynamics_config, DynamicsConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupConfig {
    pub device_id: String,
    pub now: DateTime<Utc>,
    pub fragment_lifetime_days: i64,
    pub candidate_stale_days: i64,
    pub event_compaction_days: i64,
}

#[derive(Debug, Error)]
pub enum CleanupError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("substrate_error: {0}")]
    Substrate(String),
    #[error("git_error: {0}")]
    Git(String),
    #[error("serialization_error: {0}")]
    Serialization(String),
}

pub async fn run_cleanup(substrate: &Substrate, config: CleanupConfig) -> Result<CleanupReport, CleanupError> {
    run_cleanup_with_git(substrate, config, &RealCleanupGit).await
}

async fn run_cleanup_with_git<G: CleanupGit>(
    substrate: &Substrate,
    config: CleanupConfig,
    git: &G,
) -> Result<CleanupReport, CleanupError> {
    validate_config(&config)?;

    let repo = substrate.roots().repo.as_path();
    let mut mutated_files = BTreeSet::new();
    let mut findings = Vec::new();
    let fragment_outcome = archive_expired_fragments(substrate, &config, &mut mutated_files).await?;
    let deferred_fragments = fragment_outcome.deferred_fragments;
    let mut operations = CleanupOperationCounts {
        fragments_archived: fragment_outcome.fragments_archived,
        candidates_archived: archive_stale_candidates(substrate, &config, &mut mutated_files, &mut findings).await?,
        ..CleanupOperationCounts::default()
    };
    findings.extend(collect_memory_findings(repo));
    match rebuild_entity_index(substrate).await {
        Ok((rebuilt, rows)) => {
            operations.entity_index_rebuilt = rebuilt;
            operations.entity_index_rows = rows;
        }
        Err(err) => findings.push(CleanupFinding::new(
            "memory_lint",
            "index",
            None,
            format!("entity index rebuild skipped: {err}"),
        )),
    }
    operations.observed_at_refreshed =
        refresh_observed_at(substrate, &config, &mut mutated_files, &mut findings).await?;
    let event_outcome = compact_event_logs(repo, &config, &mut mutated_files)?;
    operations.events_compacted = event_outcome.events_compacted;
    operations.event_archive_files_written = event_outcome.archive_files_written;

    operations.lint_findings = findings.iter().filter(|finding| finding.kind == "memory_lint").count();
    operations.tombstone_findings = findings.iter().filter(|finding| finding.kind == "tombstone_integrity").count();
    operations.supersession_findings = findings.iter().filter(|finding| finding.kind == "supersession_orphan").count();

    let report_path = cleanup_report_path(&config);
    mutated_files.insert(report_path.clone());
    let mut report = CleanupReport::from_input(CleanupReportInput {
        device_id: config.device_id.clone(),
        generated_at: config.now,
        operations,
        findings: sorted_findings(findings),
        deferred_fragments,
        mutated_files: mutated_files.iter().cloned().collect(),
    });
    report.commit_deferred = has_dirty_user_work(git, repo, &mutated_files)?;
    write_report(repo, &report_path, &report)?;

    if report.commit_deferred {
        stage_paths(git, repo, &mutated_files)?;
        return Ok(report);
    }

    let refreshed = read_report(repo, &report_path)?;
    let commit_outcome = commit_cleanup(git, repo, &refreshed)?;
    if commit_outcome == CommitCleanupOutcome::NoChanges {
        stage_paths(git, repo, &mutated_files)?;
    }
    Ok(report)
}

fn validate_config(config: &CleanupConfig) -> Result<(), CleanupError> {
    for (name, value) in [
        ("fragment_lifetime_days", config.fragment_lifetime_days),
        ("candidate_stale_days", config.candidate_stale_days),
        ("event_compaction_days", config.event_compaction_days),
    ] {
        if value < 1 {
            return Err(CleanupError::Serialization(format!("{name} must be positive")));
        }
    }
    Ok(())
}

/// Outcome of the expired-fragment archival step: the count plus any fragments
/// whose archival the dynamics deferral held back (spec §4).
struct FragmentArchivalStep {
    fragments_archived: usize,
    deferred_fragments: Vec<DeferredFragment>,
}

async fn archive_expired_fragments(
    substrate: &Substrate,
    config: &CleanupConfig,
    mutated_files: &mut BTreeSet<String>,
) -> Result<FragmentArchivalStep, CleanupError> {
    let repo = substrate.roots().repo.as_path();
    // Dynamics config is loaded best-effort: a malformed `dynamics:` block falls
    // back to defaults (deferral on) rather than failing the whole cleanup run.
    let dynamics = load_dynamics_config(repo).unwrap_or_else(|err| {
        tracing::warn!(error = %err, "failed to load dynamics config; defaulting to citation-aware archival");
        DynamicsConfig::default()
    });

    // Dynamics off: archive via the substrate's hard cutoff, byte-identical to
    // pre-dynamics behavior (spec §7 gating rule).
    if !dynamics.enabled {
        let before = snapshot_files(repo, &["substrate"])?;
        let outcome = substrate
            .archive_expired_substrate_fragments(config.now, config.fragment_lifetime_days)
            .await
            .map_err(|err| CleanupError::Substrate(err.to_string()))?;
        collect_changed_files(repo, &before, &["substrate"], mutated_files)?;
        return Ok(FragmentArchivalStep {
            fragments_archived: outcome.fragments_archived,
            deferred_fragments: Vec::new(),
        });
    }

    // Dynamics on: citation-aware selective archival. Cited-and-under-cap
    // fragments are deferred; everything else archives on the base schedule.
    let outcome = archive_with_deferral(repo, &config.device_id, config.now, config.fragment_lifetime_days, &dynamics)
        .map_err(CleanupError::Io)?;
    mutated_files.extend(outcome.mutated_files);
    Ok(FragmentArchivalStep {
        fragments_archived: outcome.fragments_archived,
        deferred_fragments: outcome.deferred_fragments,
    })
}

/// Walk every memory under `repo`, loading each via [`read_memory_at`] and
/// routing path-resolution and read failures into `memory_lint` findings (in
/// directory-walk order). For each successfully loaded memory the body is invoked
/// with its repo path, parsed [`Memory`], and base hash. Shared prologue for the
/// per-memory cleanup passes so the path/read error funnel lives in one place.
fn for_each_loaded_memory<F>(repo: &Path, findings: &mut Vec<CleanupFinding>, mut body: F)
where
    F: FnMut(RepoPath, Memory, Sha256, &mut Vec<CleanupFinding>),
{
    for path in relative_memory_paths(repo) {
        let repo_path = match repo_path_from_relative(&path) {
            Ok(repo_path) => repo_path,
            Err(err) => {
                findings.push(CleanupFinding::new(
                    "memory_lint",
                    path.to_string_lossy().into_owned(),
                    None,
                    err.to_string(),
                ));
                continue;
            }
        };
        let (memory, base_hash) = match read_memory_at(repo, &repo_path) {
            Ok(memory) => memory,
            Err(err) => {
                findings.push(CleanupFinding::new("memory_lint", repo_path.as_str(), None, err.to_string()));
                continue;
            }
        };
        body(repo_path, memory, base_hash, findings);
    }
}

async fn archive_stale_candidates(
    substrate: &Substrate,
    config: &CleanupConfig,
    mutated_files: &mut BTreeSet<String>,
    findings: &mut Vec<CleanupFinding>,
) -> Result<usize, CleanupError> {
    let repo = substrate.roots().repo.as_path();
    let cutoff = config.now - Duration::days(config.candidate_stale_days);
    let mut archived = 0usize;
    let mut pending = Vec::new();
    for_each_loaded_memory(repo, findings, |repo_path, memory, base_hash, _| {
        if is_stale_candidate(&memory, cutoff) {
            pending.push((repo_path, memory, base_hash));
        }
    });
    for (repo_path, mut memory, base_hash) in pending {
        memory.frontmatter.status = MemoryStatus::Archived;
        memory.frontmatter.review_state = Some("archived".to_string());
        memory.frontmatter.updated_at = config.now;
        if let Err(err) =
            write_cleanup_memory(substrate, memory, base_hash, "memoryd cleanup archived a stale dream candidate").await
        {
            findings.push(CleanupFinding::new("memory_lint", repo_path.as_str(), None, err.to_string()));
            continue;
        }
        mutated_files.insert(repo_path.as_str().to_string());
        archived += 1;
    }
    Ok(archived)
}

async fn rebuild_entity_index(substrate: &Substrate) -> Result<(bool, usize), CleanupError> {
    let before =
        substrate.query_memory(MemoryQuery::default()).await.map_err(|err| CleanupError::Substrate(err.to_string()))?;
    let rows = substrate.reindex().await.map_err(|err| CleanupError::Substrate(err.to_string()))?;
    let after =
        substrate.query_memory(MemoryQuery::default()).await.map_err(|err| CleanupError::Substrate(err.to_string()))?;
    Ok((before != after, rows))
}

fn collect_memory_findings(repo: &Path) -> Vec<CleanupFinding> {
    let mut findings = Vec::new();
    let mut ids = BTreeSet::new();
    let mut parsed_memories = Vec::new();

    for relative in relative_memory_paths(repo) {
        let repo_path = match repo_path_from_relative(&relative) {
            Ok(repo_path) => repo_path,
            Err(err) => {
                findings.push(CleanupFinding::new(
                    "memory_lint",
                    relative.to_string_lossy().into_owned(),
                    None,
                    err.to_string(),
                ));
                continue;
            }
        };
        let text = match fs::read_to_string(repo.join(&relative)) {
            Ok(text) => text,
            Err(err) => {
                findings.push(CleanupFinding::new("memory_lint", repo_path.as_str(), None, err.to_string()));
                continue;
            }
        };
        match parse_document(&text, Some(repo_path.clone())) {
            Ok(parsed) => {
                ids.insert(parsed.memory.frontmatter.id.clone());
                parsed_memories.push((repo_path, parsed.memory));
            }
            Err(err) => findings.push(CleanupFinding::new("memory_lint", repo_path.as_str(), None, err.to_string())),
        }
    }

    for (path, memory) in parsed_memories {
        collect_tombstone_findings(&mut findings, &path, &memory);
        collect_supersession_findings(&mut findings, &path, &memory, &ids);
    }

    findings
}

fn collect_tombstone_findings(findings: &mut Vec<CleanupFinding>, path: &RepoPath, memory: &Memory) {
    if memory.frontmatter.status == MemoryStatus::Tombstoned && memory.frontmatter.tombstone_events.is_empty() {
        findings.push(CleanupFinding::new(
            "tombstone_integrity",
            path.as_str(),
            Some(memory.frontmatter.id.as_str().to_string()),
            "tombstoned memory has no tombstone event",
        ));
    }
    if memory.frontmatter.status != MemoryStatus::Tombstoned && !memory.frontmatter.tombstone_events.is_empty() {
        findings.push(CleanupFinding::new(
            "tombstone_integrity",
            path.as_str(),
            Some(memory.frontmatter.id.as_str().to_string()),
            "non-tombstoned memory has tombstone events",
        ));
    }
}

fn collect_supersession_findings(
    findings: &mut Vec<CleanupFinding>,
    path: &RepoPath,
    memory: &Memory,
    ids: &BTreeSet<MemoryId>,
) {
    for id in memory.frontmatter.supersedes.iter().chain(memory.frontmatter.superseded_by.iter()) {
        if !ids.contains(id) {
            findings.push(CleanupFinding::new(
                "supersession_orphan",
                path.as_str(),
                Some(memory.frontmatter.id.as_str().to_string()),
                format!("supersession reference {id} is missing"),
            ));
        }
    }
}

async fn refresh_observed_at(
    substrate: &Substrate,
    config: &CleanupConfig,
    mutated_files: &mut BTreeSet<String>,
    findings: &mut Vec<CleanupFinding>,
) -> Result<usize, CleanupError> {
    let repo = substrate.roots().repo.as_path();
    let mut refreshed = 0usize;
    let mut pending = Vec::new();
    for_each_loaded_memory(repo, findings, |repo_path, memory, base_hash, findings| {
        let Some(source_ref) = memory.frontmatter.source.reference.as_deref() else {
            return;
        };
        let Ok(source_path) = resolve_repo_relative_file_ref(repo, source_ref) else {
            return;
        };
        if !source_path.is_file() {
            return;
        }
        let metadata = match fs::metadata(&source_path) {
            Ok(metadata) => metadata,
            Err(err) => {
                findings.push(CleanupFinding::new(
                    "observed_at_refresh",
                    repo_path.as_str(),
                    Some(memory.frontmatter.id.as_str().to_string()),
                    err.to_string(),
                ));
                return;
            }
        };
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(err) => {
                findings.push(CleanupFinding::new(
                    "observed_at_refresh",
                    repo_path.as_str(),
                    Some(memory.frontmatter.id.as_str().to_string()),
                    err.to_string(),
                ));
                return;
            }
        };
        let mtime = DateTime::<Utc>::from(modified);
        if memory.frontmatter.observed_at == Some(mtime) {
            return;
        }
        pending.push((repo_path, memory, base_hash, mtime));
    });
    for (repo_path, mut memory, base_hash, mtime) in pending {
        memory.frontmatter.observed_at = Some(mtime);
        memory.frontmatter.updated_at = config.now;
        if let Err(err) =
            write_cleanup_memory(substrate, memory, base_hash, "memoryd cleanup refreshed observed_at").await
        {
            findings.push(CleanupFinding::new("observed_at_refresh", repo_path.as_str(), None, err.to_string()));
            continue;
        }
        mutated_files.insert(repo_path.as_str().to_string());
        refreshed += 1;
    }
    Ok(refreshed)
}

async fn write_cleanup_memory(
    substrate: &Substrate,
    memory: Memory,
    base_hash: Sha256,
    reason: &str,
) -> Result<(), CleanupError> {
    // Cleanup follows the opened substrate's durability tier; daemon cleanup may
    // run against best-effort fixtures, so it must explicitly opt into that tier.
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: Some(base_hash),
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-cleanup-bot".to_string()),
                reason: Some(reason.to_string()),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map(|_| ())
        .map_err(|err| CleanupError::Substrate(err.kind.to_string()))
}

#[derive(Default)]
struct EventCompactionOutcome {
    events_compacted: usize,
    archive_files_written: usize,
}

fn compact_event_logs(
    repo: &Path,
    config: &CleanupConfig,
    mutated_files: &mut BTreeSet<String>,
) -> Result<EventCompactionOutcome, CleanupError> {
    let events_dir = repo.join("events");
    if !events_dir.exists() {
        return Ok(EventCompactionOutcome::default());
    }
    let cutoff = config.now - Duration::days(config.event_compaction_days);
    let mut outcome = EventCompactionOutcome::default();
    for entry in fs::read_dir(&events_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") || !path.is_file() {
            continue;
        }
        let events = read_events(&path)?;
        let (old, live): (Vec<_>, Vec<_>) = events.into_iter().partition(|event| event.at <= cutoff);
        if old.is_empty() {
            continue;
        }
        outcome.events_compacted += old.len();
        write_archived_events(repo, old, mutated_files, &mut outcome)?;
        rewrite_live_events(&path, &live)?;
        mutated_files.insert(repo_relative(repo, &path)?);
    }
    Ok(outcome)
}

fn write_archived_events(
    repo: &Path,
    old: Vec<Event>,
    mutated_files: &mut BTreeSet<String>,
    outcome: &mut EventCompactionOutcome,
) -> Result<(), CleanupError> {
    let mut by_month: BTreeMap<String, Vec<Event>> = BTreeMap::new();
    for event in old {
        by_month.entry(event.at.format("%Y-%m").to_string()).or_default().push(event);
    }
    for (month, new_events) in by_month {
        let repo_path = format!("events/archive/{month}.jsonl.zst");
        let absolute = repo.join(&repo_path);
        let mut events = read_zstd_event_archive(&absolute)?;
        let mut seen = events.iter().map(|event| event.id.clone()).collect::<BTreeSet<_>>();
        for event in new_events {
            if seen.insert(event.id.clone()) {
                events.push(event);
            }
        }
        events.sort_by(|left, right| left.id.cmp(&right.id));
        write_zstd_event_archive(&absolute, &events)?;
        mutated_files.insert(repo_path);
        outcome.archive_files_written += 1;
    }
    Ok(())
}

fn read_zstd_event_archive(path: &Path) -> Result<Vec<Event>, CleanupError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = zstd::stream::decode_all(fs::File::open(path)?)?;
    let text = String::from_utf8(bytes).map_err(|err| CleanupError::Serialization(err.to_string()))?;
    let mut events = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value =
            decode_line(line).ok_or_else(|| CleanupError::Serialization("bad archived event line".to_string()))?;
        events.push(serde_json::from_value(value).map_err(|err| CleanupError::Serialization(err.to_string()))?);
    }
    Ok(events)
}

fn write_zstd_event_archive(path: &Path, events: &[Event]) -> Result<(), CleanupError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = Vec::new();
    for event in events {
        let value = serde_json::to_value(event).map_err(|err| CleanupError::Serialization(err.to_string()))?;
        let line = encode_event_line(&value).map_err(|err| CleanupError::Serialization(err.to_string()))?;
        text.extend_from_slice(line.as_bytes());
    }
    let compressed = zstd::stream::encode_all(text.as_slice(), 0)?;
    atomic_write(path, &compressed)?;
    Ok(())
}

fn rewrite_live_events(path: &Path, events: &[Event]) -> Result<(), CleanupError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::new();
    for event in events {
        let value = serde_json::to_value(event).map_err(|err| CleanupError::Serialization(err.to_string()))?;
        let line = encode_event_line(&value).map_err(|err| CleanupError::Serialization(err.to_string()))?;
        if line.len() > MAX_LINE_BYTES {
            return Err(CleanupError::Serialization(format!(
                "event line too long: {} bytes (max {MAX_LINE_BYTES})",
                line.len()
            )));
        }
        bytes.extend_from_slice(line.as_bytes());
    }
    atomic_write(path, &bytes)?;
    Ok(())
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), CleanupError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temp_path_for(path);
    let _ = fs::remove_file(&temp);
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    maybe_cleanup_failpoint(path, CleanupFailpoint::BeforeArchiveRename)?;
    fs::rename(&temp, path)?;
    fsync_parent(path)?;
    maybe_cleanup_failpoint(path, CleanupFailpoint::AfterArchiveRename)?;
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("cleanup-output");
    path.with_file_name(format!("{file_name}.tmp-{}", std::process::id()))
}

fn fsync_parent(path: &Path) -> Result<(), CleanupError> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CleanupFailpoint {
    BeforeArchiveRename,
    AfterArchiveRename,
}

fn maybe_cleanup_failpoint(path: &Path, point: CleanupFailpoint) -> Result<(), CleanupError> {
    if !is_event_archive_path(path) {
        return Ok(());
    }
    let Some(repo) = repo_root_for_cleanup_path(path) else {
        return Ok(());
    };
    let failpoint_path = repo.join(".memorum/cleanup-failpoint");
    let Ok(value) = fs::read_to_string(failpoint_path) else {
        return Ok(());
    };
    let expected = match point {
        CleanupFailpoint::BeforeArchiveRename => "before_archive_rename",
        CleanupFailpoint::AfterArchiveRename => "after_archive_rename",
    };
    if value.trim() == expected {
        return Err(CleanupError::Serialization(format!("cleanup failpoint triggered: {expected}")));
    }
    Ok(())
}

fn is_event_archive_path(path: &Path) -> bool {
    path.components()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0].as_os_str() == "events" && pair[1].as_os_str() == "archive")
}

fn repo_root_for_cleanup_path(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir.file_name().and_then(|name| name.to_str()) == Some("events") {
            return dir.parent().map(Path::to_path_buf);
        }
        current = dir.parent();
    }
    None
}

fn is_stale_candidate(memory: &Memory, cutoff: DateTime<Utc>) -> bool {
    memory.frontmatter.status == MemoryStatus::Candidate
        && memory.frontmatter.updated_at <= cutoff
        && !memory.frontmatter.extras.contains_key("reviewed_at")
        && !memory.frontmatter.extras.contains_key("review_activity_at")
}

fn sorted_findings(mut findings: Vec<CleanupFinding>) -> Vec<CleanupFinding> {
    findings.sort_by(|left, right| {
        (&left.kind, &left.path, &left.id, &left.message).cmp(&(&right.kind, &right.path, &right.id, &right.message))
    });
    findings.dedup_by(|left, right| {
        left.kind == right.kind && left.path == right.path && left.id == right.id && left.message == right.message
    });
    findings
}

fn read_memory_at(repo: &Path, repo_path: &RepoPath) -> Result<(Memory, Sha256), CleanupError> {
    read_memory_file(repo, repo_path).map_err(|err| CleanupError::Serialization(err.to_string()))
}

fn write_report(repo: &Path, report_path: &str, report: &CleanupReport) -> Result<(), CleanupError> {
    let absolute = repo.join(report_path);
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(report).map_err(|err| CleanupError::Serialization(err.to_string()))?;
    fs::write(absolute, format!("{text}\n"))?;
    Ok(())
}

fn read_report(repo: &Path, report_path: &str) -> Result<CleanupReport, CleanupError> {
    serde_json::from_str(&fs::read_to_string(repo.join(report_path))?)
        .map_err(|err| CleanupError::Serialization(err.to_string()))
}

fn cleanup_report_path(config: &CleanupConfig) -> String {
    format!("dreams/cleanup/{}/{}.json", config.device_id, config.now.date_naive())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitCleanupOutcome {
    Committed,
    NoChanges,
}

fn commit_cleanup<G: CleanupGit>(
    git: &G,
    repo: &Path,
    report: &CleanupReport,
) -> Result<CommitCleanupOutcome, CleanupError> {
    let paths = report.mutated_files.iter().cloned().collect::<BTreeSet<_>>();
    stage_paths(git, repo, &paths)?;
    let changed = git.run(repo, &["diff", "--cached", "--name-only"])?;
    if changed.trim().is_empty() {
        return Ok(CommitCleanupOutcome::NoChanges);
    }
    let subject = cleanup_commit_subject(&report.device_id, report.date);
    let summary = report.operations.summary_line();
    git.commit_cleanup(repo, &subject, &summary)
}

fn has_dirty_user_work<G: CleanupGit>(
    git: &G,
    repo: &Path,
    cleanup_paths: &BTreeSet<String>,
) -> Result<bool, CleanupError> {
    let output = git.run(repo, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    Ok(output.lines().filter_map(status_path).any(|path| !cleanup_paths.contains(&path)))
}

fn stage_paths<G: CleanupGit>(git: &G, repo: &Path, paths: &BTreeSet<String>) -> Result<(), CleanupError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git.run(repo, &args)?;
    Ok(())
}

trait CleanupGit {
    fn run(&self, repo: &Path, args: &[&str]) -> Result<String, CleanupError>;

    fn commit_cleanup(&self, repo: &Path, subject: &str, summary: &str) -> Result<CommitCleanupOutcome, CleanupError>;
}

struct RealCleanupGit;

/// Strip inherited Git environment so the cleanup-bot operates against
/// `repo`'s own working tree rather than an ambient `GIT_DIR`/worktree set by
/// a calling git process (e.g. a merge driver or hook).
fn scrub_inherited_git_env(command: &mut Command) {
    command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_NAMESPACE");
}

impl CleanupGit for RealCleanupGit {
    fn run(&self, repo: &Path, args: &[&str]) -> Result<String, CleanupError> {
        let mut command = Command::new("git");
        command.args(args).current_dir(repo);
        scrub_inherited_git_env(&mut command);
        let output = command.output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(CleanupError::Git(String::from_utf8_lossy(&output.stderr).to_string()))
        }
    }

    fn commit_cleanup(&self, repo: &Path, subject: &str, summary: &str) -> Result<CommitCleanupOutcome, CleanupError> {
        let mut command = Command::new("git");
        command.args(["commit", "--author", CLEANUP_BOT_AUTHOR, "-m", subject, "-m", summary]).current_dir(repo);
        scrub_inherited_git_env(&mut command);
        command
            .env("GIT_AUTHOR_NAME", "memoryd cleanup-bot")
            .env("GIT_AUTHOR_EMAIL", "noreply@memoryd.local")
            .env("GIT_COMMITTER_NAME", "memoryd cleanup-bot")
            .env("GIT_COMMITTER_EMAIL", "noreply@memoryd.local");
        let output = command.output()?;
        if output.status.success() {
            Ok(CommitCleanupOutcome::Committed)
        } else {
            Err(CleanupError::Git(String::from_utf8_lossy(&output.stderr).to_string()))
        }
    }
}

fn status_path(line: &str) -> Option<String> {
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"').to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileSignature {
    len: u64,
    modified: Option<SystemTime>,
}

fn snapshot_files(repo: &Path, prefixes: &[&str]) -> Result<BTreeMap<String, FileSignature>, CleanupError> {
    let mut files = BTreeMap::new();
    for prefix in prefixes {
        let root = repo.join(prefix);
        if !root.exists() {
            continue;
        }
        collect_files(repo, &root, &mut files)?;
    }
    Ok(files)
}

fn collect_changed_files(
    repo: &Path,
    before: &BTreeMap<String, FileSignature>,
    prefixes: &[&str],
    changed: &mut BTreeSet<String>,
) -> Result<(), CleanupError> {
    let after = snapshot_files(repo, prefixes)?;
    for path in before.keys().chain(after.keys()) {
        if before.get(path) != after.get(path) {
            changed.insert(path.clone());
        }
    }
    Ok(())
}

fn collect_files(repo: &Path, root: &Path, files: &mut BTreeMap<String, FileSignature>) -> Result<(), CleanupError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(repo, &path, files)?;
        } else if path.is_file() {
            let metadata = fs::metadata(&path)?;
            files.insert(
                repo_relative(repo, &path)?,
                FileSignature { len: metadata.len(), modified: metadata.modified().ok() },
            );
        }
    }
    Ok(())
}

fn repo_relative(repo: &Path, path: &Path) -> Result<String, CleanupError> {
    path.strip_prefix(repo)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|err| CleanupError::Serialization(err.to_string()))
}

fn repo_path_from_relative(path: &Path) -> Result<RepoPath, CleanupError> {
    RepoPath::try_new(path.to_string_lossy().replace('\\', "/")).map_err(CleanupError::Serialization)
}
