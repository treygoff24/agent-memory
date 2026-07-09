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
    }
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
    let backlog = match substrate.pending_embedding_job_count(memory_substrate::EmbeddingLaneEligibility::AllTiers) {
        Ok(count) => count,
        Err(_) => return findings,
    };
    let active = substrate.active_embedding_triple();
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
    }

    let lifecycle = state.embedding_provider_slot().snapshot();
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

#[cfg(test)]
mod tests {
    use super::doctor_is_healthy;
    use crate::protocol::{DoctorFinding, DoctorSeverity};

    fn finding(severity: DoctorSeverity) -> DoctorFinding {
        DoctorFinding { code: "t".to_string(), message: String::new(), repair: None, severity }
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
}
