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
