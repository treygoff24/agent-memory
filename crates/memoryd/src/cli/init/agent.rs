//! Non-interactive / agent frontend for `memoryd init`.
//!
//! This path drives the shared [`SetupEngine`] from pre-parsed CLI flags via
//! [`FlagDrivenIo`] and emits a machine-readable [`SetupReport`] to stdout.
//!
//! Hard invariant: stdout carries JSON and nothing else. Every diagnostic —
//! engine notes, detection summaries, errors — goes to stderr. This is the
//! `stdout-JSON-purity` contract that lets an orchestrating agent pipe stdout
//! straight into a JSON parser.

use crate::cli::{DaemonMode, InitArgs, InitHarness, NonGitCwdDefault, WireMcpMode};
use crate::setup::{
    DaemonStrategy, FlagDrivenIo, HarnessSelection, NonGitCwdDecision, SetupDecisions, SetupDetection, SetupEngine,
    SetupReport, SetupStep, SetupStepStatus, WireMcpSelection,
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

    if has_fatal_step(&report, args.daemon) {
        // stdout already carries the full JSON report; exit non-zero so callers
        // see the failure without parsing the body.
        std::process::exit(1);
    }
    Ok(())
}

/// Map parsed CLI flags onto the engine's owned decision bundle.
fn decisions_from_args(args: &InitArgs) -> SetupDecisions {
    SetupDecisions {
        import_memories: args.import,
        harnesses: harness_selection(args.harness),
        non_git_cwd_default: non_git_cwd_decision(args.non_git_cwd_default),
        wire_mcp: wire_mcp_selection(args.wire_mcp),
        daemon: daemon_strategy(args.daemon),
        print_only: args.print_only,
    }
}

fn harness_selection(harness: InitHarness) -> HarnessSelection {
    match harness {
        InitHarness::Current => HarnessSelection::Current,
        InitHarness::Claude => HarnessSelection::Claude,
        InitHarness::Codex => HarnessSelection::Codex,
        InitHarness::All => HarnessSelection::All,
        InitHarness::None => HarnessSelection::None,
    }
}

fn non_git_cwd_decision(default: NonGitCwdDefault) -> NonGitCwdDecision {
    match default {
        NonGitCwdDefault::Skip => NonGitCwdDecision::Skip,
        NonGitCwdDefault::Me => NonGitCwdDecision::Me,
        NonGitCwdDefault::Generate => NonGitCwdDecision::Generate,
    }
}

fn wire_mcp_selection(mode: WireMcpMode) -> WireMcpSelection {
    match mode {
        WireMcpMode::Current => WireMcpSelection::Current,
        WireMcpMode::Claude => WireMcpSelection::Claude,
        WireMcpMode::Codex => WireMcpSelection::Codex,
        WireMcpMode::All => WireMcpSelection::All,
        WireMcpMode::None => WireMcpSelection::None,
    }
}

fn daemon_strategy(mode: DaemonMode) -> DaemonStrategy {
    match mode {
        DaemonMode::OnDemand => DaemonStrategy::OnDemand,
        DaemonMode::Background => DaemonStrategy::Background,
        DaemonMode::Launchd => DaemonStrategy::Launchd,
        DaemonMode::None => DaemonStrategy::None,
    }
}

/// Whether the report contains a step failure that should fail the process.
///
/// Any `Failed` step is fatal, with one carve-out: a `Verify` failure when the
/// user did not ask for a running daemon (`--daemon none`). In that mode the
/// substrate is created and import runs, but the daemon socket is intentionally
/// absent, so the status probe inside `Verify` cannot succeed by design. (The
/// `on-demand` strategy already downgrades that probe to `Expected` upstream.)
/// Treating this verify failure as fatal would punish a correct, daemon-less
/// bootstrap; a genuinely broken `background`/`launchd` daemon still surfaces a
/// fatal `Verify`.
fn has_fatal_step(report: &SetupReport, daemon: DaemonMode) -> bool {
    report.steps.iter().any(|step| {
        if step.status != SetupStepStatus::Failed {
            return false;
        }
        if step.step == SetupStep::Verify && daemon == DaemonMode::None {
            return false;
        }
        true
    })
}

/// Serialize `value` as pretty JSON to stdout. This is the only writer to
/// stdout on the agent path.
fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{json}");
    Ok(())
}
