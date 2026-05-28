use crate::dream::lease::LeaseError;
use crate::protocol::{ProtocolError, ResponseEnvelope, ResponsePayload, ResponseResult};

pub(crate) fn doctor_cli_exit_code(response: &ResponseEnvelope) -> i32 {
    match &response.result {
        ResponseResult::Success(ResponsePayload::Doctor(doctor)) if !doctor.healthy => 1,
        _ => 0,
    }
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
