//! Dream request handlers: dream-status reporting and the dream-now trigger
//! (with its request struct, CLI-override validation, and error mapping).

use super::*;

pub(crate) async fn dream_status_response(substrate: &Substrate) -> Result<ResponsePayload, HandlerError> {
    crate::dream::status::build_dream_status_report(&substrate.roots().repo, &substrate.roots().runtime)
        .await
        .map(|report| ResponsePayload::DreamStatus(Box::new(report)))
        .map_err(HandlerError::substrate)
}

pub(crate) struct DreamNowRequest {
    pub(crate) scope: String,
    pub(crate) force: bool,
    pub(crate) cli_override: Option<String>,
}

pub(crate) async fn dream_now_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: DreamNowRequest,
) -> Result<ResponsePayload, HandlerError> {
    let DreamNowRequest { scope, force, cli_override } = request;
    let config = memory_substrate::config::load_config(&substrate.roots().repo, &substrate.roots().runtime, None)
        .map_err(HandlerError::invalid_request)?;
    if !config.synced.dreams.enabled
        || crate::dream::status::disabled_sentinel_path(&substrate.roots().runtime).exists()
    {
        return Err(HandlerError::dream_disabled("dreaming is disabled on this device"));
    }
    let scope = crate::dream::scope::DreamScope::parse(&scope).map_err(HandlerError::from_dream)?;
    validate_dream_cli_override(cli_override.as_deref())?;
    let now = chrono::Utc::now();
    let acquired = crate::dream::lease::acquire_manual_lease(crate::dream::lease::LeaseAcquireRequest {
        repo: substrate.roots().repo.clone(),
        runtime: substrate.roots().runtime.clone(),
        scope: scope.as_str(),
        force,
        now,
        lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
        cli_used: cli_override.clone(),
    })
    .map_err(HandlerError::from_lease)?;

    let result = async {
        let build = crate::dream::orchestration::build_dream_run(
            substrate,
            crate::dream::orchestration::DreamRunBuildRequest {
                scope: scope.clone(),
                run_id: acquired.record.run_id,
                run_date: now.date_naive(),
                prompt_version: config.synced.dreams.prompt_version,
                notifications: Some(state.notifications.clone()),
                pass_timeout: std::time::Duration::from_secs(u64::from(config.synced.dreams.per_pass_timeout_seconds)),
                pass_2_max_candidates: config.synced.dreams.pass_2_max_candidates as usize,
                pass_1_window_days: config.synced.dreams.pass_1_window_days,
            },
        )
        .await
        .map_err(HandlerError::from_dream)?;
        let harness = crate::dream::orchestration::select_harness(
            cli_override.as_deref(),
            &config.synced.dreams.default_cli_priority,
            &build.options,
        )
        .await
        .map_err(dream_error_to_handler)?;
        crate::dream::run::DreamRunner::new(build.options.with_harness(harness), build.writer)
            .run()
            .await
            .map(|report| ResponsePayload::DreamNow(Box::new(report)))
            .map_err(HandlerError::from_dream)
    }
    .await;

    if result.is_err() {
        let _ = crate::dream::lease::release_manual_lease(crate::dream::lease::LeaseAcquireRequest {
            repo: substrate.roots().repo.clone(),
            runtime: substrate.roots().runtime.clone(),
            scope: scope.as_str(),
            force: false,
            now: chrono::Utc::now(),
            lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
            cli_used: cli_override,
        });
    }

    result
}

fn dream_error_to_handler(error: crate::dream::types::DreamError) -> HandlerError {
    match error {
        crate::dream::types::DreamError::Unavailable { message } => HandlerError::dream_unavailable(message),
        other => HandlerError::from_dream(other),
    }
}

fn validate_dream_cli_override(cli_override: Option<&str>) -> Result<(), HandlerError> {
    let Some(name) = cli_override else {
        return Ok(());
    };
    if name == "echo" && crate::dream::orchestration::echo_cli_override_enabled() {
        return Ok(());
    }
    let registry = crate::dream::registry::HarnessCliRegistry::builtin_v0_2();
    if registry.get(name).is_some() || registry.disabled_adapters().any(|adapter| adapter.name == name) {
        Ok(())
    } else {
        Err(HandlerError::invalid_request(format!("unknown harness CLI override `{name}`")))
    }
}
