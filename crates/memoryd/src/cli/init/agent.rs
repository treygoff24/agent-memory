//! Non-interactive / agent frontend for `memoryd init`.
//!
//! This path drives the shared [`SetupEngine`] from pre-parsed CLI flags via
//! [`FlagDrivenIo`] and emits a machine-readable [`SetupReport`] to stdout.
//!
//! Hard invariant: stdout carries JSON and nothing else. Every diagnostic —
//! engine notes, detection summaries, errors — goes to stderr. This is the
//! `stdout-JSON-purity` contract that lets an orchestrating agent pipe stdout
//! straight into a JSON parser.
//!
//! One contract edge: a *pre-report* fatal error — one that aborts detection or
//! decision collection before any [`SetupReport`] exists — produces an empty
//! stdout, a human-readable message on stderr, and a non-zero exit. Once the
//! report has been emitted, a fatal setup step is signaled by exit code alone;
//! stdout still carries the full parseable report. So orchestrators should read
//! the exit code first: a non-zero exit with empty stdout means the run failed
//! before producing a report (reason on stderr), while a non-zero exit with a
//! JSON body on stdout means a setup step failed fatally (details in the body).

use crate::cli::{DaemonMode, HarnessTargetArg, InitArgs, NonGitCwdDefault};
use crate::setup::{
    DaemonStrategy, FlagDrivenIo, HarnessSelection, NonGitCwdDecision, SetupDecisions, SetupDetection, SetupEngine,
    SetupReport, SetupStep, SetupStepReport, SetupStepStatus,
};

use super::resolve_repo_runtime;

/// Run the non-interactive setup path.
///
/// Emits JSON to stdout and exits with a non-zero code only when a setup step
/// fails fatally. Detection-only runs short-circuit before any decisions or
/// steps and therefore never mutate the filesystem.
pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    if args.detect_only {
        return run_detect_only(&args).await;
    }
    run_setup(args).await
}

/// Detection-only: run [`SetupDetection`] and emit its JSON. Zero side effects.
async fn run_detect_only(args: &InitArgs) -> anyhow::Result<()> {
    let (_repo, runtime) = resolve_repo_runtime(args);
    let options = crate::setup::SetupDetectionOptions {
        socket_path: Some(crate::socket::resolve_socket_path(&runtime)),
        ..Default::default()
    };

    let detection = SetupDetection::run_with_options(options)?;
    print_json(&detection)?;
    Ok(())
}

/// Full setup: collect decisions from flags, run the engine, emit the report.
async fn run_setup(args: InitArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    let decisions = decisions_from_args(&args);

    let mut io = FlagDrivenIo::new(decisions);
    let engine = SetupEngine::new(repo, runtime);
    let report = engine.run(&mut io).await?;

    // Engine notes (expected/failed step messages) are diagnostics: stderr only.
    for note in io.notes() {
        eprintln!("{note}");
    }

    print_json(&report)?;

    if has_fatal_step(&report) {
        // stdout already carries the full JSON report; exit non-zero so callers
        // see the failure without parsing the body.
        std::process::exit(1);
    }
    Ok(())
}

/// Map parsed CLI flags onto the engine's owned decision bundle.
///
/// Omitted selectors take the documented non-interactive defaults (`current`
/// harness/wiring, `skip` for non-git cwds, `on-demand` daemon) — on this path
/// there is no prompt to fall back to.
fn decisions_from_args(args: &InitArgs) -> SetupDecisions {
    SetupDecisions {
        import_memories: args.import,
        harnesses: args.harness.unwrap_or(HarnessTargetArg::Current).into(),
        non_git_cwd_default: args.non_git_cwd_default.unwrap_or(NonGitCwdDefault::Project).into(),
        wire_mcp: args.wire_mcp.unwrap_or(HarnessTargetArg::Current).into(),
        wire_hooks: args.wire_hooks.unwrap_or(HarnessTargetArg::Current).into(),
        daemon: args.daemon.unwrap_or(DaemonMode::OnDemand).into(),
        print_only: args.print_only,
    }
}

// The CLI harness-target enum is 1:1 with its engine counterpart (same variants,
// same order). The `From` impl keeps the mapping at the type boundary and stays
// total-match, so adding a variant on either side without updating the other is
// a compile error. `--harness`, `--wire-mcp`, and `--wire-hooks` all share it
// because they target the same `HarnessSelection` semantics.

impl From<HarnessTargetArg> for HarnessSelection {
    fn from(target: HarnessTargetArg) -> Self {
        match target {
            HarnessTargetArg::Current => Self::Current,
            HarnessTargetArg::Claude => Self::Claude,
            HarnessTargetArg::Codex => Self::Codex,
            HarnessTargetArg::All => Self::All,
            HarnessTargetArg::None => Self::None,
        }
    }
}

impl From<NonGitCwdDefault> for NonGitCwdDecision {
    fn from(default: NonGitCwdDefault) -> Self {
        match default {
            NonGitCwdDefault::Skip => Self::Skip,
            NonGitCwdDefault::Me => Self::Me,
            NonGitCwdDefault::Generate => Self::Generate,
            NonGitCwdDefault::Project => Self::DeriveProject,
        }
    }
}

impl From<DaemonMode> for DaemonStrategy {
    fn from(mode: DaemonMode) -> Self {
        match mode {
            DaemonMode::OnDemand => Self::OnDemand,
            DaemonMode::Background => Self::Background,
            DaemonMode::Launchd => Self::Launchd,
            DaemonMode::None => Self::None,
        }
    }
}

/// Whether the report contains a step failure that should fail the process.
///
/// Any `Failed` step is fatal, with one narrow carve-out for the `Verify` step.
/// `Verify` combines two probes: a daemon-socket status check and an in-process
/// doctor check. When the user did not ask for a running daemon (`--daemon
/// none`) the socket is intentionally absent, so the status probe fails by
/// design — that alone should not fail a correct, daemon-less bootstrap. (The
/// `on-demand` strategy downgrades the same socket-transport probe to `Expected`
/// upstream, so it never reaches this branch.)
///
/// The carve-out is scoped to the *status* probe only. The doctor check runs
/// in-process and is independent of the daemon socket, so a failed doctor probe
/// (substrate corruption, repair-required, a doctor transport error) stays fatal
/// regardless of daemon mode. A genuinely broken `background`/`launchd` daemon
/// likewise still surfaces a fatal `Verify` via its status probe.
fn has_fatal_step(report: &SetupReport) -> bool {
    let daemon = report.decisions.daemon;
    report.steps.iter().any(|step| {
        if step.status != SetupStepStatus::Failed {
            return false;
        }
        if step.step == SetupStep::Verify {
            return verify_failure_is_fatal(step, daemon);
        }
        true
    })
}

/// Decide whether a `Failed` `Verify` step is fatal.
///
/// A daemon-less bootstrap (`--daemon none`) is excused only when the failure is
/// confined to the status probe; a failed doctor probe is always fatal. When the
/// per-probe breakdown is missing (older reports) we conservatively treat the
/// failure as fatal except under `none`, preserving prior behavior.
fn verify_failure_is_fatal(step: &SetupStepReport, daemon: DaemonStrategy) -> bool {
    if daemon != DaemonStrategy::None {
        return true;
    }
    match &step.verify {
        Some(detail) => detail.doctor_probe == SetupStepStatus::Failed,
        None => false,
    }
}

/// Serialize `value` as pretty JSON to stdout. This is the only writer to
/// stdout on the agent path.
fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{json}");
    Ok(())
}
