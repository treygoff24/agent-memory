//! Executable setup-engine steps.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use memory_privacy::FileKeyProvider;
use memory_substrate::{InitOptions, OpenError, Roots, Substrate};

use crate::import::pipeline::{
    run_import_session, ExecuteOptions, ExecuteResult, HarnessFilter, ImportOptions, SocketDaemonClient,
};
use crate::import::project_map::{FixedDispositionBackend, PromptBackend, PromptedDisposition};
use crate::import::state::ImportState;
use crate::protocol::{RequestEnvelope, RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult};
use crate::socket::{probe_live_socket, SocketProbe};

use super::{
    DaemonStrategy, HarnessDetection, HarnessSelection, HarnessTarget, McpServerSpec, NonGitCwdDecision, SetupEngine,
    SetupIo, SetupPlan, SetupReport, SetupStep, SetupStepReport, SetupStepStatus, VerifyDetail, WireMcpSelection,
    WireMode, WireOutcome, WireStatus,
};

pub(crate) async fn run_all(engine: &SetupEngine, plan: &SetupPlan, io: &mut dyn SetupIo, report: &mut SetupReport) {
    let mut runtime = SystemSetupRuntime;
    run_all_with_runtime(engine, plan, io, report, &mut runtime).await;
}

#[allow(clippy::too_many_arguments, reason = "test seam keeps setup dependencies explicit")]
async fn run_all_with_runtime<R: SetupStepRuntime>(
    engine: &SetupEngine,
    plan: &SetupPlan,
    io: &mut dyn SetupIo,
    report: &mut SetupReport,
    runtime: &mut R,
) {
    push_completion(report, ensure_repo_step(engine, plan, runtime).await, io);
    push_completion(report, ensure_daemon_step(engine, plan, runtime).await, io);

    let import = run_import_step(engine, plan, runtime).await;
    if let Some(import_report) = import.import_report {
        report.import_report = Some(import_report);
    }
    push_completion(report, import.completion, io);

    let wire = wire_mcp_step(engine, plan, runtime);
    report.restart_required |= wire.restart_required;
    push_completion(report, wire.completion, io);

    push_completion(report, verify_step(engine, plan, runtime).await, io);
}

#[allow(async_fn_in_trait)]
trait SetupStepRuntime {
    async fn ensure_repo(&mut self, repo: &Path, runtime: &Path) -> Result<String, String>;

    async fn start_background_daemon(&mut self, request: DaemonStepRequest<'_>) -> Result<String, String>;

    fn install_launchd(&mut self, request: DaemonStepRequest<'_>) -> Result<String, String>;

    async fn run_import_session(
        &mut self,
        request: ImportStepRequest<'_>,
        prompts: &mut dyn PromptBackend,
    ) -> Result<ExecuteResult, String>;

    fn wire_mcp(&mut self, target: HarnessTarget, spec: &McpServerSpec, mode: WireMode) -> Result<WireOutcome, String>;

    async fn status_request(&mut self, socket: &Path) -> Result<ResponseEnvelope, String>;

    async fn doctor_request(&mut self, repo: &Path, runtime: &Path) -> Result<ResponseEnvelope, String>;
}

struct DaemonStepRequest<'a> {
    repo: &'a Path,
    runtime: &'a Path,
    socket: &'a Path,
}

struct ImportStepRequest<'a> {
    repo: &'a Path,
    options: ImportOptions,
    socket: &'a Path,
    execute_options: ExecuteOptions,
}

struct SystemSetupRuntime;

impl SetupStepRuntime for SystemSetupRuntime {
    async fn ensure_repo(&mut self, repo: &Path, runtime: &Path) -> Result<String, String> {
        ensure_substrate(repo, runtime, false).await
    }

    async fn start_background_daemon(&mut self, request: DaemonStepRequest<'_>) -> Result<String, String> {
        start_background_daemon(request).await
    }

    fn install_launchd(&mut self, request: DaemonStepRequest<'_>) -> Result<String, String> {
        install_launchd(request)
    }

    async fn run_import_session(
        &mut self,
        request: ImportStepRequest<'_>,
        prompts: &mut dyn PromptBackend,
    ) -> Result<ExecuteResult, String> {
        let mut client = SocketDaemonClient::new(request.socket.to_path_buf());
        run_import_session(request.repo, request.options, prompts, &mut client, request.execute_options)
            .await
            .map_err(|error| error.to_string())
    }

    fn wire_mcp(&mut self, target: HarnessTarget, spec: &McpServerSpec, mode: WireMode) -> Result<WireOutcome, String> {
        super::wire(target, spec, mode).map_err(|error| error.to_string())
    }

    async fn status_request(&mut self, socket: &Path) -> Result<ResponseEnvelope, String> {
        crate::client::request(socket, "setup-status", RequestPayload::Status).await.map_err(|error| error.to_string())
    }

    async fn doctor_request(&mut self, repo: &Path, runtime: &Path) -> Result<ResponseEnvelope, String> {
        let substrate = Substrate::open(Roots::new(repo.to_path_buf(), runtime.to_path_buf()))
            .await
            .map_err(|error| error.to_string())?;
        Ok(crate::handlers::handle_request(&substrate, RequestEnvelope::new("setup-doctor", RequestPayload::Doctor))
            .await)
    }
}

async fn ensure_repo_step<R: SetupStepRuntime>(
    engine: &SetupEngine,
    plan: &SetupPlan,
    runtime: &mut R,
) -> StepCompletion {
    if plan.decisions.print_only {
        return StepCompletion::expected(
            SetupStep::EnsureRepo,
            format!("[dry-run] would initialize Memorum repo at {}", engine.repo().display()),
        );
    }
    match runtime.ensure_repo(engine.repo(), engine.runtime()).await {
        Ok(message) => StepCompletion::succeeded(SetupStep::EnsureRepo, message),
        Err(message) => StepCompletion::failed(SetupStep::EnsureRepo, message),
    }
}

async fn ensure_daemon_step<R: SetupStepRuntime>(
    engine: &SetupEngine,
    plan: &SetupPlan,
    runtime: &mut R,
) -> StepCompletion {
    let socket = &plan.detection.daemon.socket_path;
    let request = DaemonStepRequest { repo: engine.repo(), runtime: engine.runtime(), socket };
    if plan.decisions.print_only {
        let action = match plan.decisions.daemon {
            DaemonStrategy::OnDemand => "leave the daemon on-demand (no background service)".to_string(),
            DaemonStrategy::Background => {
                format!("start a background daemon bound to {}", socket.display())
            }
            DaemonStrategy::Launchd => "install a launchd daemon".to_string(),
            DaemonStrategy::None => "skip daemon setup".to_string(),
        };
        return StepCompletion::expected(SetupStep::EnsureDaemon, format!("[dry-run] would {action}"));
    }
    match plan.decisions.daemon {
        DaemonStrategy::OnDemand => StepCompletion::expected(
            SetupStep::EnsureDaemon,
            "on-demand daemon selected; no background service was started, so the socket may remain absent until memoryd is started".to_string(),
        ),
        DaemonStrategy::Background => match runtime.start_background_daemon(request).await {
            Ok(message) => StepCompletion::succeeded(SetupStep::EnsureDaemon, message),
            Err(message) => StepCompletion::failed(SetupStep::EnsureDaemon, message),
        },
        DaemonStrategy::Launchd => match runtime.install_launchd(request) {
            Ok(message) => StepCompletion::succeeded(SetupStep::EnsureDaemon, message),
            Err(message) => StepCompletion::failed(SetupStep::EnsureDaemon, message),
        },
        DaemonStrategy::None => StepCompletion::skipped(SetupStep::EnsureDaemon, "daemon setup disabled"),
    }
}

async fn run_import_step<R: SetupStepRuntime>(
    engine: &SetupEngine,
    plan: &SetupPlan,
    runtime: &mut R,
) -> ImportStepOutcome {
    if !plan.decisions.import_memories {
        return ImportStepOutcome::without_report(StepCompletion::skipped(SetupStep::Import, "memory import disabled"));
    }

    let SelectedImport::Run { filter, label } = selected_import(plan) else {
        return ImportStepOutcome::without_report(StepCompletion::skipped(
            SetupStep::Import,
            "no import harness selected; pass an explicit harness to import memories",
        ));
    };

    let mut prompts = prompt_backend(plan.decisions.non_git_cwd_default);
    let request = ImportStepRequest {
        repo: engine.repo(),
        options: ImportOptions {
            from_claude: plan.detection.claude.root.clone(),
            from_codex: plan.detection.codex.root.clone(),
            harness_filter: filter,
            state: ImportState::default(),
        },
        socket: &plan.detection.daemon.socket_path,
        execute_options: ExecuteOptions { dry_run: plan.decisions.print_only, verbose_progress: false },
    };

    match runtime.run_import_session(request, prompts.as_mut()).await {
        Ok(result) => ImportStepOutcome {
            completion: StepCompletion::succeeded(SetupStep::Import, format!("import completed for {label}")),
            import_report: Some(result.report),
        },
        Err(message) => ImportStepOutcome::without_report(StepCompletion::failed(SetupStep::Import, message)),
    }
}

fn wire_mcp_step<R: SetupStepRuntime>(engine: &SetupEngine, plan: &SetupPlan, runtime: &mut R) -> WireStepOutcome {
    let SelectedWireTargets::Run(targets) = selected_wire_targets(plan) else {
        return WireStepOutcome::without_restart(StepCompletion::skipped(
            SetupStep::WireMcp,
            "no MCP harness selected; pass an explicit harness to wire configs",
        ));
    };

    let spec = match mcp_server_spec(engine, plan) {
        Ok(spec) => spec,
        Err(message) => return WireStepOutcome::without_restart(StepCompletion::failed(SetupStep::WireMcp, message)),
    };
    let mode = if plan.decisions.print_only { WireMode::PrintOnly } else { WireMode::Apply };
    let outcomes = targets.into_iter().map(|target| runtime.wire_mcp(target, &spec, mode)).collect::<Vec<_>>();

    wire_outcome_summary(outcomes)
}

async fn verify_step<R: SetupStepRuntime>(engine: &SetupEngine, plan: &SetupPlan, runtime: &mut R) -> StepCompletion {
    if plan.decisions.print_only {
        // A dry-run created no substrate and started no daemon, so the live
        // probes have nothing to verify. Report the step as expected rather than
        // failing on the deliberately-absent substrate/socket.
        return StepCompletion::expected(
            SetupStep::Verify,
            "[dry-run] would probe daemon status and run the in-process doctor check".to_string(),
        )
        .with_verify(VerifyDetail {
            status_probe: SetupStepStatus::Expected,
            doctor_probe: SetupStepStatus::Expected,
        });
    }
    let status = verify_status(plan, runtime).await;
    let doctor = verify_doctor(engine, runtime).await;
    combine_verification(status, doctor)
}

async fn verify_status<R: SetupStepRuntime>(plan: &SetupPlan, runtime: &mut R) -> VerificationSignal {
    match runtime.status_request(&plan.detection.daemon.socket_path).await {
        Ok(response) => status_response_signal(response),
        Err(message) if plan.decisions.daemon == DaemonStrategy::OnDemand => VerificationSignal::expected(format!(
            "daemon socket is not live yet as expected for on-demand setup: {message}"
        )),
        Err(message) => VerificationSignal::failed(format!("status socket check failed: {message}")),
    }
}

async fn verify_doctor<R: SetupStepRuntime>(engine: &SetupEngine, runtime: &mut R) -> VerificationSignal {
    match runtime.doctor_request(engine.repo(), engine.runtime()).await {
        Ok(response) => doctor_response_signal(response),
        Err(message) => VerificationSignal::failed(format!("doctor check failed: {message}")),
    }
}

fn status_response_signal(response: ResponseEnvelope) -> VerificationSignal {
    match response.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            VerificationSignal::succeeded(format!("status transport ok: {}", status.state))
        }
        ResponseResult::Success(_) => VerificationSignal::failed("status transport returned an unexpected payload"),
        ResponseResult::Error(error) => {
            VerificationSignal::failed(format!("status transport returned {}: {}", error.code, error.message))
        }
    }
}

fn doctor_response_signal(response: ResponseEnvelope) -> VerificationSignal {
    match response.result {
        ResponseResult::Success(ResponsePayload::Doctor(doctor)) => VerificationSignal::succeeded(format!(
            "doctor transport ok: healthy={}, findings={}",
            doctor.healthy,
            doctor.findings.len()
        )),
        ResponseResult::Success(_) => VerificationSignal::failed("doctor transport returned an unexpected payload"),
        ResponseResult::Error(error) => {
            VerificationSignal::failed(format!("doctor transport returned {}: {}", error.code, error.message))
        }
    }
}

fn combine_verification(status: VerificationSignal, doctor: VerificationSignal) -> StepCompletion {
    let message = format!("{}; {}", status.message, doctor.message);
    let detail = VerifyDetail { status_probe: status.status, doctor_probe: doctor.status };
    let completion = if status.status == SetupStepStatus::Failed || doctor.status == SetupStepStatus::Failed {
        StepCompletion::failed(SetupStep::Verify, message)
    } else if status.status == SetupStepStatus::Expected || doctor.status == SetupStepStatus::Expected {
        StepCompletion::expected(SetupStep::Verify, message)
    } else {
        StepCompletion::succeeded(SetupStep::Verify, message)
    };
    completion.with_verify(detail)
}

async fn ensure_substrate(repo: &Path, runtime: &Path, force_unsafe_durability: bool) -> Result<String, String> {
    let roots = Roots::new(repo.to_path_buf(), runtime.to_path_buf());
    let message = match Substrate::open(roots.clone()).await {
        Ok(_) => format!("Memorum repo already initialized at {}", repo.display()),
        Err(OpenError::NotAMemorumSubstrate { .. } | OpenError::DeviceIdentityMissing { .. }) => {
            Substrate::init(roots, InitOptions { force_unsafe_durability, device_id: None })
                .await
                .map(|_| format!("initialized Memorum repo at {}", repo.display()))
                .map_err(|error| error.to_string())?
        }
        Err(error) => return Err(error.to_string()),
    };

    ensure_privacy_key(runtime)?;
    Ok(message)
}

/// Provision the local Stream D age key if it is absent.
///
/// Every write through the daemon — including harness imports — passes the
/// privacy filter, which loads `FileKeyProvider::runtime_default(runtime)`. Until
/// this ran during setup, the key file did not exist and the first write failed
/// with `privacy key unavailable`, so `init --import` landed nothing. This
/// mirrors `memoryd device onboard`. Idempotent on purpose: a key already on disk
/// is left untouched — re-provisioning would generate a new identity and orphan
/// the prior one, breaking decryption of anything already encrypted.
fn ensure_privacy_key(runtime: &Path) -> Result<(), String> {
    let provider = FileKeyProvider::runtime_default(runtime);
    if provider.path().exists() {
        return Ok(());
    }
    provider.onboard_local_file().map(|_| ()).map_err(|error| error.to_string())
}

async fn start_background_daemon(request: DaemonStepRequest<'_>) -> Result<String, String> {
    if matches!(probe_live_socket(request.socket), SocketProbe::Live) {
        return Ok(format!("daemon already live at {}", request.socket.display()));
    }

    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut child = Command::new(exe)
        .arg("serve")
        .arg("--repo")
        .arg(request.repo)
        .arg("--runtime")
        .arg(request.runtime)
        .arg("--init")
        .arg("--socket")
        .arg(request.socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let pid = child.id();

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if matches!(probe_live_socket(request.socket), SocketProbe::Live) {
            return Ok(format!("started background daemon pid {pid} at {}", request.socket.display()));
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("background daemon exited before readiness: {status}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(format!("background daemon did not become ready within 10s at {}", request.socket.display()))
}

fn install_launchd(request: DaemonStepRequest<'_>) -> Result<String, String> {
    let script = std::env::current_dir().map_err(|error| error.to_string())?.join("scripts/install-launchd.sh");
    let output = Command::new("bash")
        .arg(&script)
        .arg("--repo")
        .arg(request.repo)
        .arg("--runtime")
        .arg(request.runtime)
        .arg("--daemon")
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        return Ok(format!("installed launchd daemon using {}", script.display()));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!("{}{}", stderr.trim(), stdout.trim()))
}

fn selected_import(plan: &SetupPlan) -> SelectedImport {
    match plan.decisions.harnesses {
        HarnessSelection::None => SelectedImport::Skip,
        HarnessSelection::All => SelectedImport::Run { filter: None, label: "all harnesses".to_string() },
        HarnessSelection::Claude => {
            SelectedImport::Run { filter: Some(HarnessFilter::Claude), label: "Claude Code".to_string() }
        }
        HarnessSelection::Codex => {
            SelectedImport::Run { filter: Some(HarnessFilter::Codex), label: "Codex".to_string() }
        }
        HarnessSelection::Current => match detected_harnesses(plan).as_slice() {
            [HarnessTarget::Claude] => SelectedImport::Run {
                filter: Some(HarnessFilter::Claude),
                label: "current harness (Claude Code)".to_string(),
            },
            [HarnessTarget::Codex] => {
                SelectedImport::Run { filter: Some(HarnessFilter::Codex), label: "current harness (Codex)".to_string() }
            }
            _ => SelectedImport::Skip,
        },
    }
}

fn selected_wire_targets(plan: &SetupPlan) -> SelectedWireTargets {
    match plan.decisions.wire_mcp {
        WireMcpSelection::None => SelectedWireTargets::Skip,
        WireMcpSelection::Claude => SelectedWireTargets::Run(vec![HarnessTarget::Claude]),
        WireMcpSelection::Codex => SelectedWireTargets::Run(vec![HarnessTarget::Codex]),
        WireMcpSelection::All => SelectedWireTargets::Run(vec![HarnessTarget::Claude, HarnessTarget::Codex]),
        WireMcpSelection::Current => match detected_harnesses(plan).as_slice() {
            [target] => SelectedWireTargets::Run(vec![*target]),
            _ => SelectedWireTargets::Skip,
        },
    }
}

fn detected_harnesses(plan: &SetupPlan) -> Vec<HarnessTarget> {
    let mut targets = Vec::new();
    if harness_detected(&plan.detection.claude) {
        targets.push(HarnessTarget::Claude);
    }
    if harness_detected(&plan.detection.codex) {
        targets.push(HarnessTarget::Codex);
    }
    targets
}

fn harness_detected(harness: &HarnessDetection) -> bool {
    harness.root.is_some() || harness.candidates > 0
}

fn prompt_backend(decision: NonGitCwdDecision) -> Box<dyn PromptBackend> {
    Box::new(FixedDispositionBackend::new(match decision {
        NonGitCwdDecision::Skip => PromptedDisposition::Skip,
        NonGitCwdDecision::Me => PromptedDisposition::DropToMe,
        NonGitCwdDecision::Generate => PromptedDisposition::GenerateProjectYaml,
    }))
}

fn mcp_server_spec(engine: &SetupEngine, plan: &SetupPlan) -> Result<McpServerSpec, String> {
    let socket = absolute_path(&plan.detection.daemon.socket_path)?;
    let mut args = vec!["mcp".to_string(), "--socket".to_string(), socket.to_string_lossy().into_owned()];
    if plan.decisions.daemon == DaemonStrategy::OnDemand {
        args.extend([
            "--repo".to_string(),
            absolute_path(engine.repo())?.to_string_lossy().into_owned(),
            "--runtime".to_string(),
            absolute_path(engine.runtime())?.to_string_lossy().into_owned(),
            "--auto-start".to_string(),
            "true".to_string(),
        ]);
    }

    Ok(McpServerSpec::new("memorum", "memoryd", args))
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir().map_err(|error| error.to_string())?.join(path))
}

fn wire_outcome_summary(outcomes: Vec<Result<WireOutcome, String>>) -> WireStepOutcome {
    let mut messages = Vec::new();
    let mut failed = false;
    let mut restart_required = false;

    for outcome in outcomes {
        match outcome {
            Ok(outcome) => {
                restart_required |= matches!(outcome.status, WireStatus::Wired | WireStatus::Updated);
                messages.push(format!("{:?}: {:?}", outcome.target, outcome.status));
            }
            Err(message) => {
                failed = true;
                messages.push(message);
            }
        }
    }

    let message = messages.join("; ");
    let completion = if failed {
        StepCompletion::failed(SetupStep::WireMcp, message)
    } else {
        StepCompletion::succeeded(SetupStep::WireMcp, message)
    };
    WireStepOutcome { completion, restart_required }
}

fn push_completion(report: &mut SetupReport, completion: StepCompletion, io: &mut dyn SetupIo) {
    if matches!(completion.status, SetupStepStatus::Expected | SetupStepStatus::Failed) {
        let _ = io.note(&completion.message);
    }
    let mut entry = SetupStepReport::new(completion.step, completion.status).with_message(completion.message);
    if let Some(verify) = completion.verify {
        entry = entry.with_verify(verify);
    }
    report.push_step(entry);
}

struct ImportStepOutcome {
    completion: StepCompletion,
    import_report: Option<crate::import::report::ImportReport>,
}

impl ImportStepOutcome {
    fn without_report(completion: StepCompletion) -> Self {
        Self { completion, import_report: None }
    }
}

struct WireStepOutcome {
    completion: StepCompletion,
    restart_required: bool,
}

impl WireStepOutcome {
    fn without_restart(completion: StepCompletion) -> Self {
        Self { completion, restart_required: false }
    }
}

struct StepCompletion {
    step: SetupStep,
    status: SetupStepStatus,
    message: String,
    verify: Option<VerifyDetail>,
}

impl StepCompletion {
    fn succeeded(step: SetupStep, message: impl Into<String>) -> Self {
        Self { step, status: SetupStepStatus::Succeeded, message: message.into(), verify: None }
    }

    fn failed(step: SetupStep, message: impl Into<String>) -> Self {
        Self { step, status: SetupStepStatus::Failed, message: message.into(), verify: None }
    }

    fn skipped(step: SetupStep, message: impl Into<String>) -> Self {
        Self { step, status: SetupStepStatus::Skipped, message: message.into(), verify: None }
    }

    fn expected(step: SetupStep, message: impl Into<String>) -> Self {
        Self { step, status: SetupStepStatus::Expected, message: message.into(), verify: None }
    }

    fn with_verify(mut self, verify: VerifyDetail) -> Self {
        self.verify = Some(verify);
        self
    }
}

enum SelectedImport {
    Skip,
    Run { filter: Option<HarnessFilter>, label: String },
}

enum SelectedWireTargets {
    Skip,
    Run(Vec<HarnessTarget>),
}

struct VerificationSignal {
    status: SetupStepStatus,
    message: String,
}

impl VerificationSignal {
    fn succeeded(message: impl Into<String>) -> Self {
        Self { status: SetupStepStatus::Succeeded, message: message.into() }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self { status: SetupStepStatus::Failed, message: message.into() }
    }

    fn expected(message: impl Into<String>) -> Self {
        Self { status: SetupStepStatus::Expected, message: message.into() }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use memory_substrate::{Roots, Substrate};
    use tokio::net::UnixStream;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;
    use tokio::time::{sleep, timeout};

    use super::*;
    use crate::import::report::ImportReport;
    use crate::protocol::{DoctorResponse, StatusResponse};
    use crate::server::{serve_substrate_with, ServerOptions};
    use crate::setup::{FlagDrivenIo, SetupDecisions, SetupDetectionOptions};

    #[tokio::test]
    async fn ensure_repo_initializes_real_substrate_idempotently() {
        let fixture = SetupFixture::new("repo-idempotent");
        let decisions =
            SetupDecisions { daemon: DaemonStrategy::None, wire_mcp: WireMcpSelection::None, ..Default::default() };
        let mut first_io = FlagDrivenIo::new(decisions.clone());
        let mut second_io = FlagDrivenIo::new(decisions);
        let engine = fixture.engine();

        let first = engine
            .run_with_options(&mut first_io, fixture.detection_options())
            .await
            .expect("first setup run succeeds");
        let second = engine
            .run_with_options(&mut second_io, fixture.detection_options())
            .await
            .expect("second setup run succeeds");

        assert!(fixture.repo.join(".memorum").exists());
        assert!(fixture.runtime.join("local-device.yaml").exists());
        assert_step(&first, SetupStep::EnsureRepo, SetupStepStatus::Succeeded);
        assert_step(&second, SetupStep::EnsureRepo, SetupStepStatus::Succeeded);
    }

    #[tokio::test]
    async fn on_demand_dead_socket_is_expected_not_failed() {
        let fixture = SetupFixture::new("on-demand-dead-socket");
        let decisions =
            SetupDecisions { daemon: DaemonStrategy::OnDemand, wire_mcp: WireMcpSelection::None, ..Default::default() };
        let mut io = FlagDrivenIo::new(decisions);

        let report = fixture
            .engine()
            .run_with_options(&mut io, fixture.detection_options())
            .await
            .expect("setup engine run succeeds");

        assert_step(&report, SetupStep::EnsureDaemon, SetupStepStatus::Expected);
        assert_step(&report, SetupStep::Verify, SetupStepStatus::Expected);
        assert!(!report.restart_required);
    }

    /// Under `--daemon none` the absent socket downgrades the status probe, but a
    /// failing doctor probe must still mark the `Verify` step `Failed` and carry
    /// `doctor_probe == Failed` so the agent frontend can treat it as fatal.
    #[tokio::test]
    async fn daemon_none_doctor_failure_is_recorded_as_failed_verify() {
        let fixture = SetupFixture::new("daemon-none-doctor-failure");
        let decisions =
            SetupDecisions { daemon: DaemonStrategy::None, wire_mcp: WireMcpSelection::None, ..Default::default() };
        let mut io = FlagDrivenIo::new(decisions);
        let mut runtime =
            ScriptedRuntime { doctor_error: Some("substrate repair required".to_string()), ..Default::default() };
        let detection = crate::setup::SetupDetection::run_with_options(fixture.detection_options()).expect("detect");
        let decisions = crate::setup::collect_setup_decisions(&mut io, &detection).expect("decisions");
        let plan = SetupPlan { detection: detection.clone(), decisions: decisions.clone() };
        let mut report = SetupReport::new(detection, decisions);
        report.push_step(SetupStepReport::new(SetupStep::Detect, SetupStepStatus::Succeeded));

        run_all_with_runtime(&fixture.engine(), &plan, &mut io, &mut report, &mut runtime).await;

        assert_step(&report, SetupStep::Verify, SetupStepStatus::Failed);
        let verify = report
            .steps
            .iter()
            .find(|entry| entry.step == SetupStep::Verify)
            .and_then(|entry| entry.verify)
            .expect("verify step carries a per-probe breakdown");
        assert_eq!(verify.doctor_probe, SetupStepStatus::Failed);
    }

    /// `--print-only` must not touch disk or spawn a daemon: `EnsureRepo`,
    /// `EnsureDaemon`, and `Verify` all report as `Expected` dry-run steps and
    /// the substrate is never created.
    #[tokio::test]
    async fn print_only_does_not_initialize_repo_or_start_daemon() {
        let fixture = SetupFixture::new("print-only-no-side-effects");
        let decisions = SetupDecisions {
            daemon: DaemonStrategy::Background,
            wire_mcp: WireMcpSelection::None,
            print_only: true,
            ..Default::default()
        };
        let mut io = FlagDrivenIo::new(decisions);
        let mut runtime = ScriptedRuntime::default();
        let detection = crate::setup::SetupDetection::run_with_options(fixture.detection_options()).expect("detect");
        let decisions = crate::setup::collect_setup_decisions(&mut io, &detection).expect("decisions");
        let plan = SetupPlan { detection: detection.clone(), decisions: decisions.clone() };
        let mut report = SetupReport::new(detection, decisions);
        report.push_step(SetupStepReport::new(SetupStep::Detect, SetupStepStatus::Succeeded));

        run_all_with_runtime(&fixture.engine(), &plan, &mut io, &mut report, &mut runtime).await;

        assert_step(&report, SetupStep::EnsureRepo, SetupStepStatus::Expected);
        assert_step(&report, SetupStep::EnsureDaemon, SetupStepStatus::Expected);
        assert_step(&report, SetupStep::Verify, SetupStepStatus::Expected);
        assert_eq!(runtime.background_calls, 0, "print-only must not spawn a daemon");
        assert!(!fixture.repo.join(".memorum").exists(), "print-only must not initialize the substrate");
    }

    #[test]
    fn on_demand_mcp_spec_carries_auto_start_roots_and_absolute_socket() {
        let fixture = SetupFixture::new("on-demand-mcp-spec");
        let detection = crate::setup::SetupDetection::run_with_options(fixture.detection_options()).expect("detect");
        let decisions = SetupDecisions { daemon: DaemonStrategy::OnDemand, ..Default::default() };
        let plan = SetupPlan { detection, decisions };

        let spec = mcp_server_spec(&fixture.engine(), &plan).expect("mcp spec");

        assert_eq!(spec.command, PathBuf::from("memoryd"));
        assert_eq!(spec.args[0], "mcp");
        assert!(Path::new(&spec.args[2]).is_absolute(), "socket must be absolute: {:?}", spec.args);
        assert!(has_arg_pair(&spec.args, "--repo", &fixture.repo.to_string_lossy()));
        assert!(has_arg_pair(&spec.args, "--runtime", &fixture.runtime.to_string_lossy()));
        assert!(has_arg_pair(&spec.args, "--auto-start", "true"));
    }

    #[tokio::test]
    async fn background_verify_uses_socket_status_and_in_process_doctor() {
        let fixture = SetupFixture::new("background-live-socket");
        let decisions = SetupDecisions {
            daemon: DaemonStrategy::Background,
            wire_mcp: WireMcpSelection::None,
            ..Default::default()
        };
        let mut io = FlagDrivenIo::new(decisions);
        let mut runtime = ScriptedRuntime { start_live_server_on_background: true, ..Default::default() };
        let detection = crate::setup::SetupDetection::run_with_options(fixture.detection_options()).expect("detect");
        let decisions = crate::setup::collect_setup_decisions(&mut io, &detection).expect("decisions");
        let plan = SetupPlan { detection: detection.clone(), decisions: decisions.clone() };
        let mut report = SetupReport::new(detection, decisions);
        report.push_step(SetupStepReport::new(SetupStep::Detect, SetupStepStatus::Succeeded));

        run_all_with_runtime(&fixture.engine(), &plan, &mut io, &mut report, &mut runtime).await;
        runtime.shutdown_background().await;

        assert_eq!(runtime.background_calls, 1);
        assert_step(&report, SetupStep::EnsureDaemon, SetupStepStatus::Succeeded);
        assert_step(&report, SetupStep::Verify, SetupStepStatus::Succeeded);
    }

    #[tokio::test]
    async fn import_failure_is_recorded_and_mcp_wiring_still_sets_restart_required() {
        let fixture = SetupFixture::new("nonfatal-import-failure");
        let decisions = SetupDecisions {
            import_memories: true,
            harnesses: HarnessSelection::All,
            wire_mcp: WireMcpSelection::Codex,
            daemon: DaemonStrategy::None,
            ..Default::default()
        };
        let mut io = FlagDrivenIo::new(decisions);
        let mut runtime =
            ScriptedRuntime { import_error: Some("socket unavailable".to_string()), ..Default::default() };
        let detection = crate::setup::SetupDetection::run_with_options(fixture.detection_options()).expect("detect");
        let decisions = crate::setup::collect_setup_decisions(&mut io, &detection).expect("decisions");
        let plan = SetupPlan { detection: detection.clone(), decisions: decisions.clone() };
        let mut report = SetupReport::new(detection, decisions);
        report.push_step(SetupStepReport::new(SetupStep::Detect, SetupStepStatus::Succeeded));

        run_all_with_runtime(&fixture.engine(), &plan, &mut io, &mut report, &mut runtime).await;

        assert_eq!(runtime.import_calls, 1);
        assert_eq!(runtime.wire_calls, 1);
        assert_step(&report, SetupStep::Import, SetupStepStatus::Failed);
        assert_step(&report, SetupStep::WireMcp, SetupStepStatus::Succeeded);
        assert!(report.restart_required);
    }

    #[tokio::test]
    async fn print_only_import_forwards_dry_run_to_import_runner() {
        let fixture = SetupFixture::new("print-only-import");
        let decisions = SetupDecisions {
            import_memories: true,
            harnesses: HarnessSelection::All,
            wire_mcp: WireMcpSelection::None,
            daemon: DaemonStrategy::None,
            print_only: true,
            ..Default::default()
        };
        let mut io = FlagDrivenIo::new(decisions);
        let mut runtime = ScriptedRuntime::default();
        let detection = crate::setup::SetupDetection::run_with_options(fixture.detection_options()).expect("detect");
        let decisions = crate::setup::collect_setup_decisions(&mut io, &detection).expect("decisions");
        let plan = SetupPlan { detection: detection.clone(), decisions: decisions.clone() };
        let mut report = SetupReport::new(detection, decisions);
        report.push_step(SetupStepReport::new(SetupStep::Detect, SetupStepStatus::Succeeded));

        run_all_with_runtime(&fixture.engine(), &plan, &mut io, &mut report, &mut runtime).await;

        assert_eq!(runtime.import_calls, 1);
        assert_eq!(runtime.last_import_dry_run, Some(true));
        assert_step(&report, SetupStep::Import, SetupStepStatus::Succeeded);
    }

    #[test]
    fn only_mutating_wire_outcomes_require_restart() {
        let wired = wire_outcome_summary(vec![Ok(WireOutcome {
            target: HarnessTarget::Codex,
            status: WireStatus::Wired,
            message: None,
        })]);
        assert!(wired.restart_required);

        let updated = wire_outcome_summary(vec![Ok(WireOutcome {
            target: HarnessTarget::Codex,
            status: WireStatus::Updated,
            message: None,
        })]);
        assert!(updated.restart_required);

        let already_current = wire_outcome_summary(vec![Ok(WireOutcome {
            target: HarnessTarget::Codex,
            status: WireStatus::AlreadyCurrent,
            message: None,
        })]);
        assert!(!already_current.restart_required);
    }

    #[tokio::test]
    async fn launchd_and_none_daemon_modes_are_reported_without_verification_short_circuit() {
        let fixture = SetupFixture::new("daemon-modes");
        let launchd_report = run_with_scripted_daemon(&fixture, DaemonStrategy::Launchd).await;
        assert_step(&launchd_report, SetupStep::EnsureDaemon, SetupStepStatus::Succeeded);
        assert_step_present(&launchd_report, SetupStep::Verify);

        let none_report = run_with_scripted_daemon(&fixture, DaemonStrategy::None).await;
        assert_step(&none_report, SetupStep::EnsureDaemon, SetupStepStatus::Skipped);
        assert_step_present(&none_report, SetupStep::Verify);
    }

    async fn run_with_scripted_daemon(fixture: &SetupFixture, daemon: DaemonStrategy) -> SetupReport {
        let decisions = SetupDecisions { daemon, wire_mcp: WireMcpSelection::None, ..Default::default() };
        let mut io = FlagDrivenIo::new(decisions);
        let mut runtime = ScriptedRuntime::default();
        let detection = crate::setup::SetupDetection::run_with_options(fixture.detection_options()).expect("detect");
        let decisions = crate::setup::collect_setup_decisions(&mut io, &detection).expect("decisions");
        let plan = SetupPlan { detection: detection.clone(), decisions: decisions.clone() };
        let mut report = SetupReport::new(detection, decisions);
        report.push_step(SetupStepReport::new(SetupStep::Detect, SetupStepStatus::Succeeded));
        run_all_with_runtime(&fixture.engine(), &plan, &mut io, &mut report, &mut runtime).await;
        report
    }

    struct SetupFixture {
        _temp: tempfile::TempDir,
        repo: PathBuf,
        runtime: PathBuf,
        socket: PathBuf,
        claude_root: PathBuf,
        codex_root: PathBuf,
    }

    impl SetupFixture {
        fn new(name: &str) -> Self {
            let temp = tempfile::tempdir().expect("tempdir");
            let repo = temp.path().join("repo");
            let runtime = temp.path().join("runtime");
            let socket = unique_socket_path(name);
            let claude_root = temp.path().join("claude");
            let codex_root = temp.path().join("codex");
            std::fs::create_dir_all(&claude_root).expect("claude root");
            std::fs::create_dir_all(&codex_root).expect("codex root");
            Self { _temp: temp, repo, runtime, socket, claude_root, codex_root }
        }

        fn engine(&self) -> SetupEngine {
            SetupEngine::new(&self.repo, &self.runtime)
        }

        fn detection_options(&self) -> SetupDetectionOptions {
            SetupDetectionOptions {
                claude_root_override: Some(self.claude_root.clone()),
                codex_root_override: Some(self.codex_root.clone()),
                socket_path: Some(self.socket.clone()),
            }
        }
    }

    #[derive(Default)]
    struct ScriptedRuntime {
        import_calls: usize,
        wire_calls: usize,
        background_calls: usize,
        launchd_calls: usize,
        import_error: Option<String>,
        doctor_error: Option<String>,
        last_import_dry_run: Option<bool>,
        start_live_server_on_background: bool,
        background_server: Option<(watch::Sender<bool>, JoinHandle<anyhow::Result<()>>, PathBuf)>,
    }

    impl ScriptedRuntime {
        async fn shutdown_background(&mut self) {
            let Some((shutdown, server, socket)) = self.background_server.take() else {
                return;
            };
            shutdown.send(true).expect("shutdown signal lands");
            timeout(Duration::from_secs(2), server)
                .await
                .expect("server stops before timeout")
                .expect("server task joins")
                .expect("server returns Ok");
            let _ = std::fs::remove_file(socket);
        }
    }

    impl SetupStepRuntime for ScriptedRuntime {
        async fn ensure_repo(&mut self, repo: &Path, runtime: &Path) -> Result<String, String> {
            ensure_substrate(repo, runtime, true).await
        }

        async fn start_background_daemon(&mut self, request: DaemonStepRequest<'_>) -> Result<String, String> {
            self.background_calls += 1;
            if self.start_live_server_on_background {
                let substrate = Substrate::open(Roots::new(request.repo.to_path_buf(), request.runtime.to_path_buf()))
                    .await
                    .map_err(|error| error.to_string())?;
                let (shutdown_tx, shutdown_rx) = watch::channel(false);
                let socket = request.socket.to_path_buf();
                let server = tokio::spawn(serve_substrate_with(
                    socket.clone(),
                    substrate,
                    ServerOptions { idle_frame_timeout: Duration::from_secs(5) },
                    shutdown_rx,
                ));
                wait_for_socket(&socket).await;
                self.background_server = Some((shutdown_tx, server, socket.clone()));
                return Ok(format!("test daemon live at {}", socket.display()));
            }
            Ok("background daemon start scripted".to_string())
        }

        fn install_launchd(&mut self, _request: DaemonStepRequest<'_>) -> Result<String, String> {
            self.launchd_calls += 1;
            Ok("launchd install scripted".to_string())
        }

        async fn run_import_session(
            &mut self,
            _request: ImportStepRequest<'_>,
            _prompts: &mut dyn PromptBackend,
        ) -> Result<ExecuteResult, String> {
            self.import_calls += 1;
            self.last_import_dry_run = Some(_request.execute_options.dry_run);
            if let Some(error) = self.import_error.take() {
                return Err(error);
            }
            Ok(ExecuteResult { report: empty_import_report(), state: ImportState::default() })
        }

        fn wire_mcp(
            &mut self,
            target: HarnessTarget,
            _spec: &McpServerSpec,
            _mode: WireMode,
        ) -> Result<WireOutcome, String> {
            self.wire_calls += 1;
            Ok(WireOutcome { target, status: WireStatus::Wired, message: Some("wired by test".to_string()) })
        }

        async fn status_request(&mut self, socket: &Path) -> Result<ResponseEnvelope, String> {
            crate::client::request(socket, "test-status", RequestPayload::Status)
                .await
                .map_err(|error| error.to_string())
        }

        async fn doctor_request(&mut self, repo: &Path, runtime: &Path) -> Result<ResponseEnvelope, String> {
            if let Some(error) = self.doctor_error.take() {
                return Err(error);
            }
            let substrate = Substrate::open(Roots::new(repo.to_path_buf(), runtime.to_path_buf()))
                .await
                .map_err(|error| error.to_string())?;
            Ok(crate::handlers::handle_request(&substrate, RequestEnvelope::new("test-doctor", RequestPayload::Doctor))
                .await)
        }
    }

    fn empty_import_report() -> ImportReport {
        ImportReport {
            schema_version: 1,
            harnesses: BTreeMap::new(),
            refusals: Vec::new(),
            dedups: Vec::new(),
            unresolved_back_edges: Vec::new(),
            cwd_dispositions: Vec::new(),
            project_yaml_writes: Vec::new(),
            parse_errors: Vec::new(),
        }
    }

    async fn wait_for_socket(socket: &Path) {
        for _ in 0..200 {
            if UnixStream::connect(socket).await.is_ok() {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("daemon did not bind socket at {}", socket.display());
    }

    fn unique_socket_path(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock after epoch").as_nanos();
        let dir = PathBuf::from(format!("/tmp/memd-t03-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("short socket directory");
        dir.join(format!("{test_name}-{nonce}.sock"))
    }

    fn assert_step(report: &SetupReport, step: SetupStep, status: SetupStepStatus) {
        let actual = report.steps.iter().find(|entry| entry.step == step).expect("step report");
        assert_eq!(actual.status, status, "{}", actual.message.as_deref().unwrap_or(""));
    }

    fn assert_step_present(report: &SetupReport, step: SetupStep) {
        assert!(report.steps.iter().any(|entry| entry.step == step), "missing {step:?} step");
    }

    fn has_arg_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|pair| pair[0] == flag && pair[1] == value)
    }

    #[test]
    fn response_signal_helpers_accept_expected_payloads() {
        assert_eq!(
            status_response_signal(ResponseEnvelope::success(
                "status",
                ResponsePayload::Status(StatusResponse::default())
            ))
            .status,
            SetupStepStatus::Succeeded
        );
        assert_eq!(
            doctor_response_signal(ResponseEnvelope::success(
                "doctor",
                ResponsePayload::Doctor(DoctorResponse {
                    healthy: true,
                    findings: Vec::new(),
                    guidance: String::new()
                }),
            ))
            .status,
            SetupStepStatus::Succeeded
        );
    }
}
