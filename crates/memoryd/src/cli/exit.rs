use crate::dream::lease::LeaseError;
use crate::protocol::{ProtocolError, ResponseEnvelope, ResponsePayload, ResponseResult};

/// Contract v1 exit-code dictionary for the enveloped agent commands. See
/// `docs/api/memoryd-cli-contract-v1.md` §2. Named constants so the crosswalk and
/// its tests never traffic in magic numbers.
pub(crate) const EXIT_INVALID_INPUT: i32 = 65;
pub(crate) const EXIT_NOT_FOUND: i32 = 66;
pub(crate) const EXIT_INTERNAL: i32 = 70;
pub(crate) const EXIT_TRANSIENT: i32 = 75;
pub(crate) const EXIT_CLIENT_GATE: i32 = 77;
// Reserved by the contract for pre-connect config problems; no covered command
// surfaces it yet (socket resolution never fails), so it is documented-not-emitted.
#[allow(dead_code)]
pub(crate) const EXIT_CONFIG: i32 = 78;

/// Crosswalk a daemon error-code string to a contract exit code, or `None` if the
/// code is unmapped. The public wrapper defaults unmapped codes to the internal
/// exit, but the crosswalk enumeration test asserts every code in
/// `handlers::DAEMON_ERROR_CODES` maps to `Some`, so drift fails the gate rather
/// than silently collapsing to 70.
fn agent_exit_code_opt(daemon_code: &str) -> Option<i32> {
    let code = match daemon_code {
        "invalid_request"
        | "privacy_error"
        | "unsupported"
        | "dream_disabled"
        | "port_in_use"
        | "grounding_rehydration_failed" => EXIT_INVALID_INPUT,
        "not_found" => EXIT_NOT_FOUND,
        "substrate_error"
        | "source_capture_failed"
        | "trust_artifact_error"
        | "embedding_backlog"
        | "embedding_worker_idle"
        | "embedding_retry_budget_exhausted"
        | "recall_unavailable"
        | "dream_unavailable"
        | "web_unavailable" => EXIT_TRANSIENT,
        "embedding_model_load_failed"
        | "embedding_provider_unsupported"
        | "not_implemented"
        | "method_not_allowed_on_mcp" => EXIT_INTERNAL,
        _ => return None,
    };
    Some(code)
}

/// Map a daemon error code to a contract exit code, defaulting unmapped codes to
/// the internal-error exit.
pub(crate) fn agent_exit_code(daemon_code: &str) -> i32 {
    agent_exit_code_opt(daemon_code).unwrap_or(EXIT_INTERNAL)
}

/// The full `daemon error code → exit code` crosswalk, in registry order. Used by
/// the `schema` command to publish the mapping.
pub(crate) fn agent_exit_crosswalk() -> Vec<(&'static str, i32)> {
    crate::handlers::error::DAEMON_ERROR_CODES.iter().map(|code| (*code, agent_exit_code(code))).collect()
}

pub(crate) fn exit_protocol_error(error: ProtocolError) -> ! {
    eprintln!("{}: {}", error.code, error.message);
    std::process::exit(recall_exit_code(&error.code));
}

pub(crate) fn exit_recall_unavailable(error: anyhow::Error) -> ! {
    eprintln!("recall_unavailable: {error:#}");
    std::process::exit(2);
}

pub(crate) fn exit_dream_error(error: LeaseError) -> ! {
    eprintln!("{}: {}", error.code(), error);
    std::process::exit(error.cli_exit_code());
}

pub(crate) fn recall_exit_code(code: &str) -> i32 {
    match code {
        "invalid_request" => 1,
        "dream_disabled" => 1,
        "substrate_error" | "recall_unavailable" | "dream_unavailable" => 2,
        "privacy_error" => 3,
        "not_implemented" | "dream_pass_failed" => 4,
        "lease_held" | "lease_unavailable" | "lease_dirty_tree" => 5,
        _ => 1,
    }
}

pub(crate) fn doctor_cli_exit_code(response: &ResponseEnvelope) -> i32 {
    match &response.result {
        ResponseResult::Success(ResponsePayload::Doctor(doctor)) if !doctor.healthy => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_daemon_error_code_maps_to_a_contract_exit() {
        for code in crate::handlers::error::DAEMON_ERROR_CODES {
            assert!(
                agent_exit_code_opt(code).is_some(),
                "daemon error code `{code}` has no crosswalk entry in cli::exit::agent_exit_code_opt; \
                 add it there and in docs/api/memoryd-cli-contract-v1.md §3"
            );
        }
    }

    #[test]
    fn crosswalk_pins_load_bearing_codes() {
        assert_eq!(agent_exit_code("invalid_request"), EXIT_INVALID_INPUT);
        assert_eq!(agent_exit_code("not_found"), EXIT_NOT_FOUND);
        assert_eq!(agent_exit_code("substrate_error"), EXIT_TRANSIENT);
        assert_eq!(agent_exit_code("embedding_provider_unsupported"), EXIT_INTERNAL);
        // Unknown codes default to internal rather than panicking.
        assert_eq!(agent_exit_code("a_brand_new_unmapped_code"), EXIT_INTERNAL);
    }
}
