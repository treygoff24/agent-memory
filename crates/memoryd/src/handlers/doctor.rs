use memory_substrate::Substrate;

use crate::protocol::{DoctorFinding, DoctorResponse};

pub(super) async fn doctor_response(substrate: &Substrate) -> DoctorResponse {
    let report = substrate.doctor().await;
    let mut findings = report
        .warnings
        .into_iter()
        .map(|message| DoctorFinding { code: "warning".to_string(), message, repair: None })
        .chain(report.repairs_required.into_iter().map(|message| DoctorFinding {
            code: "repair_required".to_string(),
            message,
            repair: Some("Run substrate repair before relying on daemon recall.".to_string()),
        }))
        .collect::<Vec<_>>();
    if let Ok(health) = substrate.events_log_mirror_health() {
        let stale_count = health.lag.max(health.missing_count);
        if stale_count > 0 {
            let plural = if stale_count == 1 { "" } else { "s" };
            findings.push(DoctorFinding {
                code: "events_log_mirror_lag".to_string(),
                message: format!(
                    "{stale_count} event{plural} not mirrored to SQLite - drift scoring may be stale; run `memoryd doctor --reindex`"
                ),
                repair: Some("memoryd doctor --reindex".to_string()),
            });
        }
    }
    let has_substrate_findings = !findings.is_empty();
    // Embedding-pipeline findings are advisory: recall still works via FTS bm25
    // without vectors, so a backlog or empty vector table is surfaced but does
    // not flip doctor unhealthy (a freshly-initialized substrate legitimately
    // has a backlog before the worker first drains).
    findings.extend(embedding_health_findings(substrate).await);
    let registry = crate::dream::registry::HarnessCliRegistry::builtin_v0_2();
    let mut enabled_harness_count = 0usize;
    let mut authenticated_harness_count = 0usize;
    for (name, adapter) in registry.adapters() {
        enabled_harness_count += 1;
        let probe = adapter.auth_probe().await;
        if probe.is_ok() {
            authenticated_harness_count += 1;
        } else {
            findings.push(DoctorFinding {
                code: "harness_cli_warning".to_string(),
                message: probe.operator_message(name),
                repair: Some(format!("Install/authenticate `{name}` or remove it from dream CLI priority.")),
            });
        }
    }
    DoctorResponse {
        healthy: doctor_is_healthy(has_substrate_findings, enabled_harness_count, authenticated_harness_count),
        findings,
        guidance: "Doctor reflects Memorum substrate validation, repair state, and dreaming harness availability."
            .to_string(),
    }
}

fn doctor_is_healthy(
    has_substrate_findings: bool,
    enabled_harness_count: usize,
    authenticated_harness_count: usize,
) -> bool {
    !has_substrate_findings && (enabled_harness_count == 0 || authenticated_harness_count > 0)
}

/// Embedding-pipeline health: pending-job backlog and an empty active-triple
/// vector table.
///
/// These are the two signals that the production embedding worker is not
/// keeping up (or is not running at all — model never loaded). Both are
/// warnings, not repair-required: recall still works via FTS bm25, just without
/// vector similarity. A persistent backlog with zero vectors is the strong
/// "embeddings are not being produced" signal worth surfacing prominently.
async fn embedding_health_findings(substrate: &Substrate) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    let backlog = match substrate.pending_embedding_job_count() {
        Ok(count) => count,
        Err(_) => return findings,
    };
    let active = substrate.active_embedding_triple();
    let vector_count = match &active {
        Ok(triple) => substrate.vector_count(triple.clone()).await.ok(),
        Err(_) => None,
    };

    if let Ok(triple) = &active {
        if !crate::embedding::is_fastembed_candle_triple(triple) {
            findings.push(DoctorFinding {
                code: "embedding_provider_unsupported".to_string(),
                message: format!(
                    "active embedding provider `{}` is unsupported by this daemon; expected `{}`. The embedding worker will not start, so vector recall is unavailable for this triple.",
                    triple.provider,
                    crate::embedding::FASTEMBED_CANDLE_PROVIDER
                ),
                repair: Some("Switch active_embedding.provider to the fastembed candle lane or run a daemon that supports the configured provider.".to_string()),
            });
        }
    }

    if let Some(error) = crate::embedding::model_load_failure() {
        findings.push(DoctorFinding {
            code: "embedding_model_load_failed".to_string(),
            message: format!(
                "embedding model load is failing and the daemon is retrying on a slow backoff; vector recall is FTS-only until a retry succeeds. Last error: {error}"
            ),
            repair: Some("Check network/model-cache availability and daemon logs; no restart is required after connectivity recovers.".to_string()),
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
        });
    }

    if backlog > 0 && vector_count == Some(0) {
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
        });
    } else if backlog > 0 {
        let plural = if backlog == 1 { "" } else { "s" };
        findings.push(DoctorFinding {
            code: "embedding_backlog".to_string(),
            message: format!(
                "{backlog} embedding job{plural} pending - vector recall is incomplete until the background worker drains them."
            ),
            repair: None,
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::doctor_is_healthy;

    #[test]
    fn doctor_health_requires_clean_substrate_and_available_harness() {
        assert!(doctor_is_healthy(false, 2, 1), "one authenticated enabled harness keeps doctor healthy");
        assert!(!doctor_is_healthy(false, 2, 0), "zero authenticated enabled harnesses is unhealthy");
        assert!(!doctor_is_healthy(true, 2, 2), "substrate findings are unhealthy regardless of harnesses");
        assert!(!doctor_is_healthy(true, 0, 0), "substrate findings are unhealthy even with empty registry");
        assert!(doctor_is_healthy(false, 0, 0), "empty registry is trivially healthy when substrate is clean");
    }
}
