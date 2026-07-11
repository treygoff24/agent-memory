use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use memory_substrate::config::{DreamsConfig, SubstrateConfig};
use memory_substrate::Substrate;

use crate::handlers::HandlerState;
use crate::protocol::{DoctorFinding, DoctorResponse, DoctorSeverity, PassStatus};

fn fatal_finding(code: &str, message: String, repair: Option<String>) -> DoctorFinding {
    DoctorFinding { code: code.to_string(), message, repair, severity: DoctorSeverity::Fatal }
}

fn advisory_finding(code: &str, message: String, repair: Option<String>) -> DoctorFinding {
    DoctorFinding { code: code.to_string(), message, repair, severity: DoctorSeverity::Advisory }
}

pub(super) async fn doctor_response(substrate: &Substrate, state: &HandlerState) -> DoctorResponse {
    let report = substrate.doctor().await;
    let mut findings = report
        .warnings
        .into_iter()
        .map(|message| fatal_finding("warning", message, None))
        .chain(report.repairs_required.into_iter().map(|message| {
            fatal_finding(
                "repair_required",
                message,
                Some("Run substrate repair before relying on daemon recall.".to_string()),
            )
        }))
        .collect::<Vec<_>>();
    if let Ok(health) = substrate.events_log_mirror_health() {
        let stale_count = health.lag.max(health.missing_count);
        if stale_count > 0 {
            let plural = if stale_count == 1 { "" } else { "s" };
            findings.push(fatal_finding(
                "events_log_mirror_lag",
                format!(
                    "{stale_count} event{plural} not mirrored to SQLite - drift scoring may be stale; run `memoryd doctor --reindex`"
                ),
                Some("memoryd doctor --reindex".to_string()),
            ));
        }
    }
    // Embedding-pipeline findings are advisory: recall still works via FTS bm25
    // without vectors (a freshly-initialized substrate legitimately has a backlog
    // before the worker first drains).
    findings.extend(embedding_health_findings(substrate, state).await);
    findings.extend(index_schema_findings(substrate));
    let (merge_proposal_counts, merge_findings) = merge_health(substrate);
    findings.extend(merge_findings);
    // Foundation-loop checks (F4 D1-D4): dream freshness (advisory), sync/quarantine
    // (fatal), stale uncommitted substrate (fatal), recall budget pressure (advisory).
    findings.extend(foundation_loop_findings(substrate, state).await);
    let registry = crate::dream::registry::HarnessCliRegistry::builtin_v0_2();
    let mut enabled_harness_count = 0usize;
    let mut authenticated_harness_count = 0usize;
    for (name, adapter) in registry.adapters() {
        enabled_harness_count += 1;
        let probe = adapter.auth_probe().await;
        if probe.is_ok() {
            authenticated_harness_count += 1;
        } else {
            findings.push(advisory_finding(
                "harness_cli_warning",
                probe.operator_message(name),
                Some(harness_repair_hint(name, &probe)),
            ));
        }
    }
    DoctorResponse {
        healthy: doctor_is_healthy(&findings, enabled_harness_count, authenticated_harness_count),
        findings,
        guidance: "Doctor reflects Memorum substrate validation, repair state, dreaming harness availability, and the runtime loop (dream freshness, sync, uncommitted substrate, recall budget)."
            .to_string(),
        embedding_counts: embedding_row_kind_counts(substrate),
        merge_proposal_counts,
    }
}

fn merge_health(substrate: &Substrate) -> (std::collections::BTreeMap<String, u64>, Vec<DoctorFinding>) {
    let store = crate::dream::merge::MergeProposalStore::new(&substrate.roots().runtime);
    let proposals = match store.list() {
        Ok(proposals) => proposals,
        Err(error) => {
            return (
                Default::default(),
                vec![fatal_finding(
                    "merge_proposal_store",
                    error.to_string(),
                    Some("Inspect the device-local merge proposal store.".into()),
                )],
            )
        }
    };
    let mut counts = std::collections::BTreeMap::new();
    for proposal in &proposals {
        *counts.entry(format!("{:?}", proposal.status).to_lowercase()).or_insert(0) += 1;
    }
    let mut findings = Vec::new();
    if let Some(count) = counts.get("quarantined") {
        findings.push(fatal_finding(
            "merge_proposal_quarantined",
            format!("{count} merge proposal(s) require operator repair"),
            Some("Inspect with `memoryd review merges list`, then repair or reject the proposal.".into()),
        ));
    }
    // A stuck Applying journal older than one reconcile cycle is a doctor error;
    // a fresh Applying proposal is reported informationally. A missing or
    // unreadable journal is never "fresh" — if we have no evidence the apply
    // completed, it is stale (fatal).
    let threshold = chrono::Duration::seconds(60);
    let now = chrono::Utc::now();
    let mut stale_applying = 0_u64;
    let mut fresh_applying = 0_u64;
    for proposal in proposals.iter().filter(|p| p.status == crate::dream::merge::MergeProposalStatus::Applying) {
        let mtime = match store.journal_mtime(&proposal.proposal_id) {
            Ok(Some(mtime)) => Some(chrono::DateTime::<chrono::Utc>::from(mtime)),
            Ok(None) => None,
            Err(_) => None,
        };
        match mtime {
            Some(mtime) if now.signed_duration_since(mtime) <= threshold => fresh_applying += 1,
            _ => stale_applying += 1,
        }
    }
    if stale_applying > 0 {
        findings.push(fatal_finding(
            "merge_proposal_stuck_applying",
            format!("{stale_applying} merge proposal(s) remained Applying after reconciliation"),
            Some("Restart memoryd once; if still Applying, inspect the proposal journal.".into()),
        ));
    }
    if fresh_applying > 0 {
        findings.push(advisory_finding(
            "merge_proposal_applying",
            format!("{fresh_applying} merge proposal(s) are still Applying"),
            Some("Reconciliation is in progress; revisit if this persists.".into()),
        ));
    }
    (counts, findings)
}

fn index_schema_findings(substrate: &Substrate) -> Vec<DoctorFinding> {
    let result = substrate.with_index(|index| {
        let version: i64 = index.connection().query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations", [], |row| row.get(0),
        )?;
        let mut mismatched = Vec::new();
        for table in ["memory_abstractions", "memory_cues", "aux_embedding_meta", "aux_pending_embedding_jobs"] {
            let exists: i64 = index.connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |row| row.get(0),
            )?;
            if (version >= 6) != (exists != 0) { mismatched.push(table); }
        }
        let trigger_tables: i64 = index.connection().query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND (name='memory_triggers' OR name LIKE 'memory_trigger_%')",
            [], |row| row.get(0),
        )?;
        Ok((mismatched, trigger_tables))
    });
    match result {
        Ok((mismatched, 0)) if mismatched.is_empty() => Vec::new(),
        Ok((mismatched, triggers)) => vec![fatal_finding(
            "index_schema_v6_inconsistent",
            format!("schema-6 table mismatch: mismatched={mismatched:?}, pre-W4 trigger tables={triggers}"),
            Some("Restore the pre-migration SQLite copy or run `memoryd doctor --reindex`.".to_string()),
        )],
        Err(error) => vec![fatal_finding("index_schema_probe_failed", error.to_string(), None)],
    }
}

fn embedding_row_kind_counts(substrate: &Substrate) -> std::collections::BTreeMap<String, u64> {
    substrate
        .active_embedding_triple()
        .ok()
        .and_then(|triple| {
            substrate.embedding_row_kind_counts(crate::embedding::embedding_lane_eligibility(&triple)).ok()
        })
        .unwrap_or_default()
}

/// F4 foundation-loop checks D1-D4. D5 (capture freshness) is deferred to v3.0-P2.
async fn foundation_loop_findings(substrate: &Substrate, state: &HandlerState) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    // Config errors are already surfaced by `substrate.doctor()`; if it cannot load,
    // skip the threshold-driven checks rather than guess.
    let Ok(config) = memory_substrate::config::load_config(&substrate.roots().repo, &substrate.roots().runtime, None)
    else {
        return findings;
    };
    let now = Utc::now();
    let repo = substrate.roots().repo.as_path();
    let runtime = substrate.roots().runtime.as_path();
    findings.extend(dream_freshness_finding(repo, &config.synced.dreams, now)); // D1
    findings.extend(sync_conflict_finding(substrate, repo, runtime).await); // D2
    findings.extend(stale_uncommitted_finding(repo, &config.synced.substrate, now)); // D3
    findings.extend(budget_pressure_finding(state, &config.synced.dreams)); // D4
    findings
}

/// D1 (advisory): dreaming has missed `doctor_missed_threshold` consecutive scheduled
/// runs, or its last successful run is older than 48h. A fresh install with no runs is
/// NOT a finding.
fn dream_freshness_finding(repo: &Path, dreams: &DreamsConfig, now: DateTime<Utc>) -> Option<DoctorFinding> {
    let summaries = crate::dream::status::collect_last_runs(repo).ok()?;
    if summaries.is_empty() {
        return None;
    }
    let max_missed = summaries.iter().map(|summary| summary.consecutive_missed_runs).max().unwrap_or(0);
    let latest_success = summaries
        .iter()
        .filter(|summary| summary.last_run_outcome == Some(PassStatus::Success))
        .filter_map(|summary| summary.last_run_at)
        .max();
    let stale_success = latest_success.is_some_and(|at| now - at > Duration::hours(48));
    if max_missed < dreams.doctor_missed_threshold && !stale_success {
        return None;
    }
    let detail = if max_missed >= dreams.doctor_missed_threshold {
        format!("{max_missed} consecutive scheduled run(s) missed (threshold {})", dreams.doctor_missed_threshold)
    } else {
        "no successful dream run in over 48h".to_string()
    };
    Some(advisory_finding(
        "dream_stale",
        format!("Dreaming may be stalled: {detail}. Check `memoryd dream status` and the launchd dream schedule."),
        Some("Run `memoryd dream now` and check daemon/launchd logs.".to_string()),
    ))
}

/// Marker filename written by
/// `memory_substrate::runtime::reconcile::write_startup_marker` (mirrored here; the
/// substrate hardcodes the same literal in `reconcile.rs` and exports no constant to
/// reuse). A repair-cascade recovery can leave this marker set with NO `MERGE_HEAD`
/// and NO live quarantine, so D2 MUST check it — otherwise a recovered-but-not-yet-
/// reconciled tree reports silently green (I-F4.1).
const STARTUP_RECONCILE_MARKER: &str = "startup-reconcile.required";

/// D2 (fatal): an active blocking conflict (live quarantined memories), a stranded
/// in-progress merge, or a pending startup-reconcile recovery marker. `recovery_required`
/// (I-F4.1) is `MERGE_HEAD || startup-reconcile.required`; checking only the merge head
/// left the marker-set repair-cascade state silently green. Reads LIVE state, not the
/// stale `Substrate::open` snapshot, so an in-daemon `quarantine resolve`, a manual
/// merge-abort, or a completed reconcile clears it.
async fn sync_conflict_finding(substrate: &Substrate, repo: &Path, runtime: &Path) -> Option<DoctorFinding> {
    // Count via the shared `status OR trust_level == Quarantined` predicate so D2 can never
    // disagree with `quarantine list`/`status.conflicts_count`; a status-only count here would
    // silently miss a trust-level-only quarantine and re-open the seam I-F4.1 forbids.
    let quarantined =
        super::quarantine::blocking_conflict_paths(substrate).await.map(|paths| paths.len() as u64).unwrap_or(0);
    sync_conflict_finding_from_state(repo, runtime, quarantined)
}

/// Pure filesystem + count core of D2, split from the `Substrate`-coupled wrapper so the
/// marker/merge seam (I-F4.1) is unit-testable without standing up a live `Substrate`.
fn sync_conflict_finding_from_state(repo: &Path, runtime: &Path, quarantined: u64) -> Option<DoctorFinding> {
    let merge_head = repo.join(".git").join("MERGE_HEAD").exists();
    let recovery_marker = runtime.join(STARTUP_RECONCILE_MARKER).exists();
    if quarantined == 0 && !merge_head && !recovery_marker {
        return None;
    }
    let mut parts = Vec::new();
    if quarantined > 0 {
        let plural = if quarantined == 1 { "y" } else { "ies" };
        parts.push(format!("{quarantined} quarantined memor{plural}"));
    }
    if merge_head {
        parts.push("a stranded in-progress git merge (.git/MERGE_HEAD)".to_string());
    }
    if recovery_marker {
        parts.push("a pending startup-reconcile recovery marker (startup-reconcile.required)".to_string());
    }
    Some(fatal_finding(
        "sync_blocked",
        format!(
            "Sync is blocked: {}. Merge-conflict quarantines: `memoryd quarantine list`/`resolve`; governance quarantines: `memoryd review approve`/`reject` (or finish/abort the merge, then re-run startup reconciliation).",
            parts.join(" and ")
        ),
        Some("memoryd quarantine list".to_string()),
    ))
}

/// D3 (fatal): daemon-managed substrate has been uncommitted longer than
/// `commit_debounce_ms + commit_stale_grace_ms` — F1's commit worker is not keeping up.
/// Measures the OLDEST uncommitted path's mtime so it does not flap during the normal
/// debounce window.
fn stale_uncommitted_finding(repo: &Path, substrate: &SubstrateConfig, now: DateTime<Utc>) -> Option<DoctorFinding> {
    let paths = memory_substrate::git::uncommitted_substrate_paths(repo).ok()?;
    if paths.is_empty() {
        return None;
    }
    let threshold =
        Duration::milliseconds(i64::from(substrate.commit_debounce_ms) + i64::from(substrate.commit_stale_grace_ms));
    let oldest = paths
        .iter()
        .filter_map(|path| std::fs::metadata(repo.join(path)).ok())
        .filter_map(|meta| meta.modified().ok())
        .map(DateTime::<Utc>::from)
        .min()?;
    if now - oldest <= threshold {
        return None;
    }
    let age_seconds = (now - oldest).num_seconds();
    Some(fatal_finding(
        "substrate_uncommitted_stale",
        format!(
            "{} daemon-managed file(s) uncommitted for {age_seconds}s (> debounce+grace {}ms) - the substrate commit worker is not committing writes.",
            paths.len(),
            threshold.num_milliseconds()
        ),
        Some("Check daemon logs for substrate commit worker errors and that `memoryd serve` is running.".to_string()),
    ))
}

/// D4 (advisory): cumulative recall budget exhaustion for any section exceeds
/// `doctor_budget_exhausted_threshold` (since daemon start).
fn budget_pressure_finding(state: &HandlerState, dreams: &DreamsConfig) -> Option<DoctorFinding> {
    let snapshot = state.recall.snapshot();
    let over = snapshot
        .budget_exhausted_total
        .iter()
        .filter(|(_, &count)| count > dreams.doctor_budget_exhausted_threshold)
        .map(|(section, &count)| format!("{section}={count}"))
        .collect::<Vec<_>>();
    if over.is_empty() {
        return None;
    }
    Some(advisory_finding(
        "recall_budget_pressure",
        format!(
            "Recall budget exhausted repeatedly (cumulative, threshold {}): {}. Recall is dropping content under budget pressure.",
            dreams.doctor_budget_exhausted_threshold,
            over.join(", ")
        ),
        None,
    ))
}

/// Actionable repair guidance for a harness that failed its auth probe.
///
/// The Claude adapter has two distinct, easily-confused failure modes under
/// launchd: the binary is not on the daemon's PATH (the daemon does not inherit
/// the user's shell PATH), or it is found but no logged-in profile is selected.
/// Generic "install/authenticate" guidance hides both.
fn harness_repair_hint(name: &str, probe: &crate::dream::harness::AuthProbeResult) -> String {
    use crate::dream::harness::AuthProbeResult;
    if name == "claude" {
        return match probe {
            AuthProbeResult::CliMissing { .. } => {
                "`claude` is not on the daemon's PATH (the launchd daemon does not inherit your shell PATH). Reinstall via scripts/install-launchd.sh so the plist PATH includes the claude binary directory (e.g. ~/.local/bin).".to_string()
            }
            _ => {
                "Authenticate Claude (`claude auth login`), or set CLAUDE_CONFIG_DIR in the daemon environment (launchd plist) to a logged-in profile directory such as ~/.claude-personal, then re-run `memoryd doctor`.".to_string()
            }
        };
    }
    format!("Install/authenticate `{name}` or remove it from dream CLI priority.")
}

fn doctor_is_healthy(
    findings: &[DoctorFinding],
    enabled_harness_count: usize,
    authenticated_harness_count: usize,
) -> bool {
    findings.iter().all(|finding| finding.severity != DoctorSeverity::Fatal)
        && (enabled_harness_count == 0 || authenticated_harness_count > 0)
}

/// Embedding-pipeline health: pending-job backlog and an empty active-triple
/// vector table.
///
/// These are the two signals that the production embedding worker is not
/// keeping up (or is not running at all — model never loaded). Both are
/// warnings, not repair-required: recall still works via FTS bm25, just without
/// vector similarity. A persistent backlog with zero vectors is the strong
/// "embeddings are not being produced" signal worth surfacing prominently.
async fn embedding_health_findings(substrate: &Substrate, state: &HandlerState) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    let active = substrate.active_embedding_triple();
    let (backlog, held_local_jobs) = match &active {
        Ok(triple) => {
            let eligibility = crate::embedding::embedding_lane_eligibility(triple);
            let backlog = match substrate.pending_embedding_job_count(eligibility) {
                Ok(count) => count,
                Err(_) => return findings,
            };
            let held_local_jobs = match substrate.held_local_embedding_job_count(eligibility) {
                Ok(count) => count,
                Err(_) => return findings,
            };
            (backlog, held_local_jobs)
        }
        Err(_) => return findings,
    };
    let vector_count = match &active {
        Ok(triple) => substrate.vector_count(triple.clone()).await.ok(),
        Err(_) => None,
    };

    if let Ok(triple) = &active {
        if !crate::embedding::is_fastembed_candle_triple(triple) && !crate::embedding::is_gemini_api_triple(triple) {
            findings.push(DoctorFinding {
                code: "embedding_provider_unsupported".to_string(),
                message: format!(
                    "active embedding provider `{}` is unsupported by this daemon; expected `{}` or `{}`. The embedding worker will not start, so vector recall is unavailable for this triple.",
                    triple.provider,
                    crate::embedding::FASTEMBED_CANDLE_PROVIDER,
                    crate::embedding::GEMINI_API_PROVIDER
                ),
                repair: Some("Switch active_embedding.provider to a supported lane or run a daemon that supports the configured provider.".to_string()),
                severity: DoctorSeverity::Advisory,
            });
        }
        if crate::embedding::is_api_embedding_lane(triple) && held_local_jobs > 0 {
            let plural = if held_local_jobs == 1 { "" } else { "s" };
            findings.push(DoctorFinding {
                code: "embedding_api_lane_held_local".to_string(),
                message: format!(
                    "{held_local_jobs} memory embedding job{plural} held local-only under the API embedding lane because its sensitivity tier is not plaintext-eligible to transit the API; vector recall for those memories is FTS-only."
                ),
                repair: Some(
                    "Switch to the local embedding lane if you need vector recall for sensitive memories.".to_string(),
                ),
                severity: DoctorSeverity::Advisory,
            });
        }
    }

    let lifecycle = state.embedding_provider_slot().snapshot();
    if let Ok(triple) = &active {
        if crate::embedding::is_api_embedding_lane(triple) {
            let repo = substrate.roots().repo.as_path();
            let runtime = substrate.roots().runtime.as_path();
            if !memory_substrate::config::load_api_embedding_consent(repo) {
                findings.push(fatal_finding(
                    "embedding_api_consent_missing",
                    "the Gemini API embedding lane is active but API consent is not recorded; the daemon will not start the API provider".to_string(),
                    Some("Run `memoryd config embedding-lane --lane gemini-api` to record consent, or switch back to the local lane.".to_string()),
                ));
            }
            let env_key_present =
                std::env::var("MEMORUM_GEMINI_API_KEY").ok().is_some_and(|key| !key.trim().is_empty());
            match crate::embedding::read_gemini_api_key(runtime).map(|key| env_key_present || key.is_some()) {
                Ok(false) => findings.push(fatal_finding(
                    "embedding_api_key_missing",
                    "the Gemini API embedding lane is active but no usable API key was found".to_string(),
                    Some("Set `MEMORUM_GEMINI_API_KEY` or configure the key with the `memoryd` CLI.".to_string()),
                )),
                // An unreadable key FILE is only fatal when the env var doesn't
                // already supply the key — env-only deployments are healthy.
                Err(error) if !env_key_present => findings.push(fatal_finding(
                    "embedding_api_key_missing",
                    format!("the Gemini API embedding lane cannot read its API key: {error}"),
                    Some("Set `MEMORUM_GEMINI_API_KEY` or configure the key with the `memoryd` CLI.".to_string()),
                )),
                Ok(true) | Err(_) => {}
            }

            // Drain-tick failures are the primary signal (the provider slot's
            // last_error only records LOAD failures); fall back to the slot.
            let last_error = crate::embedding::drain_failure()
                .or_else(|| lifecycle.last_error.clone())
                .unwrap_or_default()
                .to_ascii_lowercase();
            // `EmbeddingError::RateLimit` Displays as "embedding API rate-limited: ...".
            if backlog > 0
                && (last_error.contains("rate-limited")
                    || last_error.contains("rate limit")
                    || last_error.contains("429"))
            {
                findings.push(advisory_finding(
                    "embedding_api_rate_limited",
                    format!(
                        "the Gemini API embedding drain has {backlog} pending job(s) and its latest provider error reports rate limiting; doctor has no duration telemetry to prove how long this has persisted"
                    ),
                    Some("Wait for the provider backoff to clear, then re-run `memoryd doctor` and inspect daemon logs.".to_string()),
                ));
            }
            if backlog > 0
                && (last_error.contains("transport")
                    || last_error.contains("network")
                    || last_error.contains("offline")
                    || last_error.contains("connect"))
            {
                findings.push(advisory_finding(
                    "embedding_api_offline",
                    format!(
                        "the Gemini API embedding lane has {backlog} pending job(s) and its latest provider error indicates network unreachability; doctor made no network request"
                    ),
                    Some("Restore network access and re-run `memoryd doctor`; recall remains available through FTS while the backlog drains.".to_string()),
                ));
            }

            if let Some(finding) = orphaned_vector_table_finding(substrate, triple) {
                findings.push(finding);
            }
        }
    }
    let load_error = if lifecycle.state == "failed" {
        lifecycle.last_error.or_else(crate::embedding::model_load_failure)
    } else {
        crate::embedding::model_load_failure()
    };
    if let Some(error) = load_error {
        // F7: distinguish intentional disable (MEMORUM_DISABLE_EMBEDDING_WORKER)
        // from transient load failures. The disabled path is a permanent,
        // intentional opt-out — retry guidance is wrong for it.
        let is_intentionally_disabled = error.contains("MEMORUM_DISABLE_EMBEDDING_WORKER");
        let (message, repair) = if is_intentionally_disabled {
            (
                "embedding worker is intentionally disabled (MEMORUM_DISABLE_EMBEDDING_WORKER); vector recall is FTS-only. This is a permanent opt-out, not a transient failure.".to_string(),
                Some("Unset MEMORUM_DISABLE_EMBEDDING_WORKER and restart memoryd to enable the embedding worker.".to_string()),
            )
        } else {
            (
                format!(
                    "embedding model load is failing and the daemon is retrying on a slow backoff; vector recall is FTS-only until a retry succeeds. Last error: {error}"
                ),
                Some("Check network/model-cache availability and daemon logs; no restart is required after connectivity recovers.".to_string()),
            )
        };
        findings.push(DoctorFinding {
            code: "embedding_model_load_failed".to_string(),
            message,
            repair,
            severity: DoctorSeverity::Advisory,
        });
    }

    let exhausted = crate::embedding::worker::exhausted_retry_budget_job_count();
    if exhausted > 0 {
        let plural = if exhausted == 1 { "" } else { "s" };
        findings.push(DoctorFinding {
            code: "embedding_retry_budget_exhausted".to_string(),
            message: format!(
                "{exhausted} embedding job{plural} exhausted this daemon process's retry budget and will be skipped until restart so newer jobs can drain."
            ),
            repair: Some("Inspect daemon logs for the poisoned chunk ids; restart memoryd to retry them after fixing the cause.".to_string()),
            severity: DoctorSeverity::Advisory,
        });
    }

    // F1: `embedding_worker_idle` fires only when the lifecycle is in a
    // terminal-degraded state (failed or no loader configured). A fresh or
    // restarted daemon with pending jobs is in dormant/loading/active — that is
    // healthy per design amendment F4, and the `embedding_backlog` advisory
    // below covers the transient backlog. Without this gate, the finding
    // false-alarms during the normal dormant→loading→first-drain window.
    let loader_configured = state.embedding_provider_slot().has_loader_configured();
    let worker_idle = backlog > 0 && vector_count == Some(0) && (lifecycle.state == "failed" || !loader_configured);

    if worker_idle {
        // Backlog exists but nothing has ever been embedded for the active
        // triple — the worker is down (model load failed, disabled, or the
        // daemon was started without it). This is the headline finding.
        let model = active.as_ref().map(|t| t.model_ref.clone()).unwrap_or_else(|_| "<unknown>".to_string());
        findings.push(DoctorFinding {
            code: "embedding_worker_idle".to_string(),
            message: format!(
                "{backlog} embedding job(s) pending and the active-triple ({model}) vector table is empty - the embedding worker is not producing vectors; recall is FTS-only. Check daemon logs for model-load retries or provider-lane guards."
            ),
            repair: Some("Start `memoryd serve` and check daemon logs for an embedding model load or provider-lane error.".to_string()),
            severity: DoctorSeverity::Advisory,
        });
    } else if backlog > 0 {
        let plural = if backlog == 1 { "" } else { "s" };
        findings.push(DoctorFinding {
            code: "embedding_backlog".to_string(),
            message: format!(
                "{backlog} embedding job{plural} pending - vector recall is incomplete until the background worker drains them."
            ),
            repair: None,
            severity: DoctorSeverity::Advisory,
        });
    }
    findings
}

fn orphaned_vector_table_finding(
    substrate: &Substrate,
    active: &memory_substrate::EmbeddingTriple,
) -> Option<DoctorFinding> {
    let active_table = memory_substrate::index::sqlite_vec::vector_table_name(active);
    let tables = substrate
        .with_index(|index| {
            let mut statement = index
                .connection()
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'vec_%'")?;
            let names =
                statement.query_map([], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(names)
        })
        .ok()?;
    orphaned_vector_table_finding_from_names(&tables, active_table.as_ref())
}

fn orphaned_vector_table_finding_from_names(tables: &[String], active_table: &str) -> Option<DoctorFinding> {
    let active_base = active_table.to_string();
    let orphan_tables = tables
        .iter()
        .map(|table| table.splitn(3, '_').take(2).collect::<Vec<_>>().join("_"))
        .filter(|table: &String| table != &active_base)
        .collect::<std::collections::BTreeSet<_>>();
    let orphan_count = orphan_tables.len();
    (orphan_count > 0).then(|| advisory_finding(
        "embedding_orphaned_triples",
        format!(
            "{orphan_count} vector table(s) remain for embedding triples other than the active API triple; old lane tables are retained by design"
        ),
        Some("Use the embedding drop-triple maintenance surface to remove an old triple when it is no longer needed.".to_string()),
    ))
}

#[cfg(test)]
mod tests {
    use super::doctor_is_healthy;
    use memory_substrate::Sensitivity;

    use crate::dream::merge::{memory, MergeProposal, MergeProposalStatus, MergeProposalStore};
    use crate::protocol::{DoctorFinding, DoctorSeverity};

    fn merge_store(temp: &tempfile::TempDir) -> MergeProposalStore {
        MergeProposalStore::new(&temp.path().join("runtime"))
    }

    fn finding(severity: DoctorSeverity) -> DoctorFinding {
        DoctorFinding { code: "t".to_string(), message: String::new(), repair: None, severity }
    }

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn remove(name: &'static str) -> Self {
            let previous = std::env::var_os(name);
            std::env::remove_var(name);
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    async fn substrate_with_active_embedding(
        triple: memory_substrate::EmbeddingTriple,
        device_id: &str,
    ) -> TestSubstrate {
        let temp = tempfile::tempdir().expect("tempdir"); // expect-justified: test setup
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&repo).expect("repo dir"); // expect-justified: test setup
        std::fs::write(
            repo.join("config.yaml"),
            format!(
                "schema_version: 1\nactive_embedding:\n  provider: {}\n  model_ref: {}\n  dimension: {}\n",
                triple.provider, triple.model_ref, triple.dimension
            ),
        )
        .expect("write config"); // expect-justified: test setup

        let substrate = memory_substrate::Substrate::init(
            memory_substrate::Roots::new(&repo, &runtime),
            memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_string()) },
        )
        .await
        .expect("substrate init"); // expect-justified: test setup

        TestSubstrate { _temp: temp, substrate }
    }

    struct TestSubstrate {
        _temp: tempfile::TempDir,
        substrate: memory_substrate::Substrate,
    }

    #[tokio::test]
    async fn embedding_provider_unsupported_is_lane_aware() {
        let state = crate::handlers::HandlerState::new();
        let gemini = substrate_with_active_embedding(
            memory_substrate::EmbeddingTriple {
                provider: crate::embedding::GEMINI_API_PROVIDER.to_string(),
                model_ref: "gemini-embedding-2".to_string(),
                dimension: 768,
            },
            "dev_doctorgemini",
        )
        .await;

        let gemini_findings = super::embedding_health_findings(&gemini.substrate, &state).await;
        assert!(
            !gemini_findings.iter().any(|finding| finding.code == "embedding_provider_unsupported"),
            "gemini-api is a supported lane and must not raise embedding_provider_unsupported: {gemini_findings:?}"
        );

        let bogus = substrate_with_active_embedding(
            memory_substrate::EmbeddingTriple {
                provider: "acme-whatever".to_string(),
                model_ref: "not-real".to_string(),
                dimension: 3,
            },
            "dev_doctorbogus",
        )
        .await;

        let bogus_findings = super::embedding_health_findings(&bogus.substrate, &state).await;
        assert!(
            bogus_findings.iter().any(|finding| finding.code == "embedding_provider_unsupported"),
            "unknown providers still raise embedding_provider_unsupported: {bogus_findings:?}"
        );
    }

    #[tokio::test]
    async fn embedding_api_lane_held_local_advisory_does_not_inflate_backlog() {
        let state = crate::handlers::HandlerState::new();
        let (_temp, substrate) = crate::embedding::lane_test_support::init_substrate_with_active_embedding(
            crate::embedding::lane_test_support::gemini_test_triple(),
            "dev_doctorapilane",
        )
        .await;
        crate::embedding::lane_test_support::seed_indexed_memory(
            &substrate,
            "mem_20260709_dddddddddddddddd_000001",
            Sensitivity::Internal,
            "doctor api lane drainable body",
        );
        crate::embedding::lane_test_support::seed_indexed_memory(
            &substrate,
            "mem_20260709_dddddddddddddddd_000002",
            Sensitivity::Confidential,
            "doctor api lane held local body",
        );

        let findings = super::embedding_health_findings(&substrate, &state).await;

        let held_local = findings
            .iter()
            .find(|finding| finding.code == "embedding_api_lane_held_local")
            .expect("held-local advisory");
        assert_eq!(held_local.severity, DoctorSeverity::Advisory);
        assert!(held_local.message.contains("1 memory embedding job"), "{held_local:?}");
        assert!(
            held_local.repair.as_deref().is_some_and(|repair| repair.contains("local embedding lane")),
            "{held_local:?}"
        );

        let backlog = findings
            .iter()
            .find(|finding| finding.code == "embedding_worker_idle" || finding.code == "embedding_backlog")
            .expect("drainable backlog finding");
        assert!(backlog.message.contains("1 embedding job"), "{backlog:?}");
        assert!(
            !backlog.message.contains("2 embedding job"),
            "held-local jobs must not inflate drainable backlog: {backlog:?}"
        );
    }

    #[tokio::test]
    async fn embedding_held_local_advisory_is_api_lane_only() {
        let state = crate::handlers::HandlerState::new();
        let (_temp, substrate) = crate::embedding::lane_test_support::init_substrate_with_active_embedding(
            crate::embedding::lane_test_support::local_test_triple(),
            "dev_doctorlocal",
        )
        .await;
        crate::embedding::lane_test_support::seed_indexed_memory(
            &substrate,
            "mem_20260709_dddddddddddddddd_000003",
            Sensitivity::Confidential,
            "doctor local lane sensitive body",
        );

        let findings = super::embedding_health_findings(&substrate, &state).await;

        assert!(
            !findings.iter().any(|finding| finding.code == "embedding_api_lane_held_local"),
            "local lane must not report API held-local advisory: {findings:?}"
        );
    }

    #[tokio::test]
    async fn doctor_reports_embedding_counts_for_each_row_kind() {
        let (_temp, substrate) = crate::embedding::lane_test_support::init_substrate_with_active_embedding(
            crate::embedding::lane_test_support::local_test_triple(),
            "dev_doctorrowkinds",
        )
        .await;
        let counts = super::embedding_row_kind_counts(&substrate);
        for key in [
            "chunk_indexed",
            "chunk_pending",
            "chunk_held_local",
            "abstraction_indexed",
            "abstraction_pending",
            "abstraction_held_local",
            "cue_indexed",
            "cue_pending",
            "cue_held_local",
        ] {
            assert!(counts.contains_key(key), "missing {key}: {counts:?}");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn api_lane_missing_key_is_fatal_and_key_presence_clears_it() {
        let _api_key = EnvVarGuard::remove("MEMORUM_GEMINI_API_KEY");
        let state = crate::handlers::HandlerState::new();
        let missing = crate::embedding::lane_test_support::init_substrate_with_active_embedding(
            crate::embedding::lane_test_support::gemini_test_triple(),
            "dev_doctorkeymissing",
        )
        .await;
        let missing_findings = super::embedding_health_findings(&missing.1, &state).await;
        let key_finding = missing_findings.iter().find(|finding| finding.code == "embedding_api_key_missing");
        assert_eq!(key_finding.map(|finding| finding.severity), Some(DoctorSeverity::Fatal));

        crate::embedding::write_gemini_api_key(missing.1.roots().runtime.as_path(), "test-key")
            .expect("write test key");
        let present_findings = super::embedding_health_findings(&missing.1, &state).await;
        assert!(!present_findings.iter().any(|finding| finding.code == "embedding_api_key_missing"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn api_only_findings_do_not_fire_for_local_lane() {
        let _api_key = EnvVarGuard::remove("MEMORUM_GEMINI_API_KEY");
        let state = crate::handlers::HandlerState::new();
        let local = crate::embedding::lane_test_support::init_substrate_with_active_embedding(
            crate::embedding::lane_test_support::local_test_triple(),
            "dev_doctorlocalfindings",
        )
        .await;
        let findings = super::embedding_health_findings(&local.1, &state).await;
        assert!(!findings.iter().any(|finding| {
            matches!(
                finding.code.as_str(),
                "embedding_api_key_missing" | "embedding_orphaned_triples" | "embedding_api_consent_missing"
            )
        }));
    }

    #[test]
    fn orphaned_vector_table_finding_fires_only_for_non_active_tables() {
        let finding = super::orphaned_vector_table_finding_from_names(
            &[
                "vec_active".to_string(),
                "vec_active_data".to_string(),
                "vec_old".to_string(),
                "vec_old_data".to_string(),
            ],
            "vec_active",
        )
        .expect("orphaned table finding");
        assert_eq!(finding.code, "embedding_orphaned_triples");
        assert_eq!(finding.severity, DoctorSeverity::Advisory);

        assert!(super::orphaned_vector_table_finding_from_names(&["vec_active".to_string()], "vec_active").is_none());
    }

    #[test]
    fn doctor_health_requires_no_fatal_finding_and_available_harness() {
        let advisory = [finding(DoctorSeverity::Advisory)];
        let fatal = [finding(DoctorSeverity::Fatal)];
        assert!(doctor_is_healthy(&advisory, 2, 1), "advisory-only with an authenticated harness is healthy");
        assert!(!doctor_is_healthy(&advisory, 2, 0), "no authenticated harness is unhealthy");
        assert!(!doctor_is_healthy(&fatal, 2, 2), "a fatal finding is unhealthy regardless of harnesses");
        assert!(!doctor_is_healthy(&fatal, 0, 0), "a fatal finding is unhealthy even with an empty registry");
        assert!(doctor_is_healthy(&[], 0, 0), "no findings + empty registry is trivially healthy");
    }

    #[test]
    fn d3_stale_uncommitted_fires_only_after_debounce_plus_grace() {
        use std::process::Command;

        use chrono::Duration;
        use memory_substrate::config::SubstrateConfig;
        use memory_substrate::tree::bootstrap_repo_tree;

        let repo = tempfile::tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        Command::new("git").args(["init"]).current_dir(repo.path()).output().expect("git init"); // expect-justified: test setup
        std::fs::create_dir_all(repo.path().join("me/identity")).expect("dir"); // expect-justified: test setup
        std::fs::write(repo.path().join("me/identity/fact.md"), "---\nsummary: f\n---\nbody\n").expect("write"); // expect-justified: test setup

        let cfg = SubstrateConfig { commit_debounce_ms: 2000, commit_stale_grace_ms: 5000 };
        let mtime: chrono::DateTime<chrono::Utc> = std::fs::metadata(repo.path().join("me/identity/fact.md"))
            .expect("meta") // expect-justified: test setup
            .modified()
            .expect("mtime") // expect-justified: test setup
            .into();

        // Within debounce+grace (7s): no D3 finding — must not flap during the normal window.
        assert!(super::stale_uncommitted_finding(repo.path(), &cfg, mtime + Duration::seconds(1)).is_none());
        // Past the threshold: a fatal D3 finding.
        let finding = super::stale_uncommitted_finding(repo.path(), &cfg, mtime + Duration::seconds(10))
            .expect("stale uncommitted fires past the threshold"); // expect-justified: test assertion
        assert_eq!(finding.code, "substrate_uncommitted_stale");
        assert_eq!(finding.severity, DoctorSeverity::Fatal);
    }

    /// D1 advisory: a dream landed but the last success is older than 48h.
    #[test]
    fn doctor_sees_dead_dream() {
        use chrono::Duration;
        use memory_substrate::config::DreamsConfig;

        let repo = tempfile::tempdir().expect("tempdir"); // expect-justified: test setup
        let journal = repo.path().join("dreams/journal/me");
        std::fs::create_dir_all(&journal).expect("journal dir"); // expect-justified: test setup
        let entry = journal.join("2026-06-28.md");
        std::fs::write(&entry, "# dream\n").expect("journal file"); // expect-justified: test setup
        let mtime: chrono::DateTime<chrono::Utc> =
            std::fs::metadata(&entry).expect("meta").modified().expect("mtime").into(); // expect-justified: test setup
        let dreams = DreamsConfig::default();

        // A successful run older than 48h is a stalled-dream advisory (never fatal).
        let finding = super::dream_freshness_finding(repo.path(), &dreams, mtime + Duration::hours(49))
            .expect("dead dream fires past 48h"); // expect-justified: test assertion
        assert_eq!(finding.code, "dream_stale");
        assert_eq!(finding.severity, DoctorSeverity::Advisory);
        // A fresh run is not a finding.
        assert!(super::dream_freshness_finding(repo.path(), &dreams, mtime + Duration::hours(1)).is_none());
    }

    /// D2 fatal: covers BOTH `recovery_required` triggers — a MERGE_HEAD/quarantine and
    /// the `startup-reconcile.required` marker. The marker case is the silently-green
    /// seam I-F4.1 forbids: a repair-cascade recovery sets it with no merge and no live
    /// quarantine, and D2 must still flip `healthy` to false.
    #[test]
    fn doctor_sees_blocking_conflict() {
        let temp = tempfile::tempdir().expect("tempdir"); // expect-justified: test setup
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(repo.join(".git")).expect("git dir"); // expect-justified: test setup
        std::fs::create_dir_all(&runtime).expect("runtime dir"); // expect-justified: test setup

        // No seam: no finding.
        assert!(super::sync_conflict_finding_from_state(&repo, &runtime, 0).is_none());

        // Trigger 1a — a stranded in-progress merge (.git/MERGE_HEAD): fatal.
        std::fs::write(repo.join(".git/MERGE_HEAD"), "deadbeef\n").expect("merge head"); // expect-justified: test setup
        let merge = super::sync_conflict_finding_from_state(&repo, &runtime, 0).expect("merge head fires"); // expect-justified: test assertion
        assert_eq!(merge.code, "sync_blocked");
        assert_eq!(merge.severity, DoctorSeverity::Fatal);
        std::fs::remove_file(repo.join(".git/MERGE_HEAD")).expect("rm merge head"); // expect-justified: test setup

        // Trigger 1b — live quarantined memories with no merge and no marker: still fatal.
        let quarantine = super::sync_conflict_finding_from_state(&repo, &runtime, 1).expect("quarantine fires"); // expect-justified: test assertion
        assert_eq!(quarantine.code, "sync_blocked");
        assert_eq!(quarantine.severity, DoctorSeverity::Fatal);

        // Trigger 2 — the startup-reconcile recovery marker ALONE (no merge, no quarantine).
        // This is the seam that silently reported healthy before FIX 1.
        std::fs::write(runtime.join("startup-reconcile.required"), "recovery").expect("marker"); // expect-justified: test setup
        let marker = super::sync_conflict_finding_from_state(&repo, &runtime, 0).expect("marker fires"); // expect-justified: test assertion
        assert_eq!(marker.code, "sync_blocked");
        assert_eq!(marker.severity, DoctorSeverity::Fatal);
        assert!(marker.message.contains("startup-reconcile"), "the marker seam is named in the finding message");
    }

    /// D4 advisory: cumulative-since-daemon-start budget exhaustion past the threshold.
    #[test]
    fn doctor_sees_budget_pressure() {
        use memory_substrate::config::DreamsConfig;

        let state = crate::handlers::HandlerState::new();
        let dreams = DreamsConfig { doctor_budget_exhausted_threshold: 2, ..DreamsConfig::default() };

        // At-threshold (cumulative 2, not > 2): no finding.
        state.recall.record_budget_exhausted("recent-memory");
        state.recall.record_budget_exhausted("recent-memory");
        assert!(super::budget_pressure_finding(&state, &dreams).is_none());

        // One more pushes the cumulative count over the threshold.
        state.recall.record_budget_exhausted("recent-memory");
        let finding = super::budget_pressure_finding(&state, &dreams).expect("budget pressure fires"); // expect-justified: test assertion
        assert_eq!(finding.code, "recall_budget_pressure");
        assert_eq!(finding.severity, DoctorSeverity::Advisory);
    }

    /// Keystone: a write→commit→dream→observe state with NO active seam yields
    /// `healthy: true` (D1–D4; capture/D5 excluded until v3.0-P2).
    #[test]
    fn doctor_foundation_loop_green() {
        use std::process::Command;

        use chrono::Duration;
        use memory_substrate::config::{DreamsConfig, SubstrateConfig};
        use memory_substrate::tree::bootstrap_repo_tree;

        let dir = tempfile::tempdir().expect("tempdir"); // expect-justified: test setup
        let repo = dir.path().join("repo");
        let runtime = dir.path().join("runtime");
        bootstrap_repo_tree(&repo).expect("bootstrap"); // expect-justified: test setup
        std::fs::create_dir_all(&runtime).expect("runtime dir"); // expect-justified: test setup

        // A dream landed (write→dream): one journal entry under the `me` scope.
        let journal = repo.join("dreams/journal/me");
        std::fs::create_dir_all(&journal).expect("journal dir"); // expect-justified: test setup
        let entry = journal.join("2026-06-28.md");
        std::fs::write(&entry, "# dream\n").expect("journal file"); // expect-justified: test setup

        // write→commit: the substrate is committed, so D3 sees no uncommitted paths.
        Command::new("git").args(["init"]).current_dir(&repo).output().expect("git init"); // expect-justified: test setup
        Command::new("git").args(["add", "-A"]).current_dir(&repo).output().expect("git add"); // expect-justified: test setup
        Command::new("git")
            .args(["-c", "user.email=test@example.com", "-c", "user.name=test", "commit", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit"); // expect-justified: test setup

        let mtime: chrono::DateTime<chrono::Utc> =
            std::fs::metadata(&entry).expect("meta").modified().expect("mtime").into(); // expect-justified: test setup
        let now = mtime + Duration::hours(1); // recent dream, well within 48h
        let dreams = DreamsConfig::default();
        let substrate_cfg = SubstrateConfig::default();
        let state = crate::handlers::HandlerState::new();

        let mut findings = Vec::new();
        findings.extend(super::dream_freshness_finding(&repo, &dreams, now)); // D1
        findings.extend(super::sync_conflict_finding_from_state(&repo, &runtime, 0)); // D2
        findings.extend(super::stale_uncommitted_finding(&repo, &substrate_cfg, now)); // D3
        findings.extend(super::budget_pressure_finding(&state, &dreams)); // D4

        assert!(findings.is_empty(), "a closed loop with no seam has no D1-D4 findings: {findings:?}");
        assert!(
            super::doctor_is_healthy(&findings, 1, 1),
            "no active seam plus an authenticated harness yields healthy:true"
        );
    }

    #[tokio::test]
    async fn merge_health_quarantined_is_fatal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = merge_store(&temp);
        let proposal = MergeProposal::new(
            vec![memory_substrate::MemoryId::new("mem_20260711_aaaaaaaaaaaaaaaa_000201")],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000202"),
            Vec::new(),
            "doctor-test",
        )
        .expect("valid proposal");
        store.create(&proposal).expect("create proposal");
        let mut proposal = store.load(&proposal.proposal_id).expect("load proposal");
        proposal.status = MergeProposalStatus::Quarantined;
        store.save(&proposal).expect("save quarantined");

        let (_counts, findings) = super::merge_health(
            &memory_substrate::Substrate::init(
                memory_substrate::Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
                memory_substrate::InitOptions {
                    force_unsafe_durability: true,
                    device_id: Some("dev_mergehealth".into()),
                },
            )
            .await
            .expect("substrate init"),
        );

        let fatal = findings.iter().find(|f| f.code == "merge_proposal_quarantined").expect("quarantined finding");
        assert_eq!(fatal.severity, DoctorSeverity::Fatal);
    }

    #[tokio::test]
    async fn merge_health_fresh_applying_is_advisory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = merge_store(&temp);
        let proposal = MergeProposal::new(
            vec![memory_substrate::MemoryId::new("mem_20260711_aaaaaaaaaaaaaaaa_000203")],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000204"),
            Vec::new(),
            "doctor-test",
        )
        .expect("valid proposal");
        store.create(&proposal).expect("create proposal");
        let mut proposal = store.load(&proposal.proposal_id).expect("load proposal");
        proposal.status = MergeProposalStatus::Applying;
        store.save(&proposal).expect("save applying");

        // A fresh Applying proposal must have a readable journal to be considered
        // in-progress; missing/unreadable journals are now treated as stale.
        let journal_path = temp
            .path()
            .join("runtime")
            .join("governance/merge-proposals")
            .join(&proposal.proposal_id)
            .join("journal.jsonl");
        std::fs::create_dir_all(journal_path.parent().expect("journal parent")).expect("journal dir");
        std::fs::write(&journal_path, b"").expect("journal file");

        let substrate = memory_substrate::Substrate::init(
            memory_substrate::Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
            memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some("dev_mergehealth".into()) },
        )
        .await
        .expect("substrate init");

        let (_counts, findings) = super::merge_health(&substrate);
        let advisory = findings.iter().find(|f| f.code == "merge_proposal_applying").expect("applying finding");
        assert_eq!(advisory.severity, DoctorSeverity::Advisory);
    }

    #[tokio::test]
    async fn merge_health_stale_applying_is_fatal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = merge_store(&temp);
        let proposal = MergeProposal::new(
            vec![memory_substrate::MemoryId::new("mem_20260711_aaaaaaaaaaaaaaaa_000205")],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000206"),
            Vec::new(),
            "doctor-test",
        )
        .expect("valid proposal");
        store.create(&proposal).expect("create proposal");
        let mut proposal = store.load(&proposal.proposal_id).expect("load proposal");
        proposal.status = MergeProposalStatus::Applying;
        store.save(&proposal).expect("save applying");

        // Create a journal file with an old mtime to cross the freshness threshold.
        let journal_path = temp
            .path()
            .join("runtime")
            .join("governance/merge-proposals")
            .join(&proposal.proposal_id)
            .join("journal.jsonl");
        std::fs::create_dir_all(journal_path.parent().expect("journal parent")).expect("journal dir");
        std::fs::write(&journal_path, b"").expect("journal file");
        let output = std::process::Command::new("touch")
            .args(["-t", "202401010000.00", journal_path.to_str().expect("journal path")])
            .output()
            .expect("touch command");
        assert!(output.status.success(), "touch failed: {}", String::from_utf8_lossy(&output.stderr));

        let substrate = memory_substrate::Substrate::init(
            memory_substrate::Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
            memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some("dev_mergehealth".into()) },
        )
        .await
        .expect("substrate init");

        let (_counts, findings) = super::merge_health(&substrate);
        let fatal =
            findings.iter().find(|f| f.code == "merge_proposal_stuck_applying").expect("stale applying finding");
        assert_eq!(fatal.severity, DoctorSeverity::Fatal);
    }

    #[tokio::test]
    async fn merge_health_missing_journal_for_applying_is_fatal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = merge_store(&temp);
        let proposal = MergeProposal::new(
            vec![memory_substrate::MemoryId::new("mem_20260711_aaaaaaaaaaaaaaaa_000207")],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000208"),
            Vec::new(),
            "doctor-test",
        )
        .expect("valid proposal");
        store.create(&proposal).expect("create proposal");
        let mut proposal = store.load(&proposal.proposal_id).expect("load proposal");
        proposal.status = MergeProposalStatus::Applying;
        store.save(&proposal).expect("save applying");

        // Deliberately no journal file.

        let substrate = memory_substrate::Substrate::init(
            memory_substrate::Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
            memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some("dev_mergehealth".into()) },
        )
        .await
        .expect("substrate init");

        let (_counts, findings) = super::merge_health(&substrate);
        let fatal = findings
            .iter()
            .find(|f| f.code == "merge_proposal_stuck_applying")
            .expect("stale applying finding for missing journal");
        assert_eq!(fatal.severity, DoctorSeverity::Fatal);
    }
}
