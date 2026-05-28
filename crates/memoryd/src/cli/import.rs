use std::path::Path;

use crate::cli::paths::resolve_socket_arg;
use crate::cli::{ImportArgs, ImportHarness};
use crate::import::pipeline::{ExecuteOptions, HarnessFilter, ImportEngine, ImportOptions, SocketDaemonClient};
use crate::import::project_map::{InteractivePromptBackend, PromptBackend, PromptResult, PromptedDisposition};
use crate::import::state::{ImportLockGuard, ImportState};

pub async fn run(args: ImportArgs) -> anyhow::Result<()> {
    let harness_filter = match args.harness {
        ImportHarness::All => None,
        ImportHarness::Claude => Some(HarnessFilter::Claude),
        ImportHarness::Codex => Some(HarnessFilter::Codex),
    };

    let engine = ImportEngine::new(&args.repo);
    // Acquire the importer lock so concurrent invocations fail fast with a
    // clear AnotherImportInProgress error.
    let _lock = ImportLockGuard::acquire(&engine.state_path)
        .map_err(|error| anyhow::anyhow!("acquire import lock: {error}"))?;
    let state = ImportState::load(&engine.state_path).map_err(|error| anyhow::anyhow!("load import state: {error}"))?;

    // Non-interactive callers (e.g. `memoryd init --non-interactive`) get a
    // default-skip prompt backend; everyone else gets the dialoguer-backed one.
    let mut prompts: Box<dyn PromptBackend> = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        Box::new(InteractivePromptBackend)
    } else {
        Box::new(DefaultSkipPrompts)
    };

    let plan = engine
        .plan(
            ImportOptions {
                from_claude: args.from_claude.clone(),
                from_codex: args.from_codex.clone(),
                harness_filter,
                state,
            },
            prompts.as_mut(),
        )
        .await
        .map_err(|error| anyhow::anyhow!("plan import: {error}"))?;

    if !args.quiet {
        eprintln!(
            "Planned {} write(s): claude={} codex={}",
            plan.actions.len(),
            plan.source_discovery_summary.claude_candidates,
            plan.source_discovery_summary.codex_candidates,
        );
    }

    let socket = resolve_socket_arg(&args.socket);
    let mut client = SocketDaemonClient::new(socket);
    let execute_opts = ExecuteOptions { dry_run: args.dry_run, verbose_progress: !args.quiet };
    let result = engine
        .execute(plan, execute_opts, &mut client)
        .await
        .map_err(|error| anyhow::anyhow!("execute import: {error}"))?;

    println!("{}", result.report.to_text());
    if let Some(path) = &args.report {
        std::fs::write(path, result.report.to_json()?)?;
        if !args.quiet {
            eprintln!("Report written to {}", path.display());
        }
    }
    Ok(())
}

/// Non-interactive fallback prompt backend: defaults every non-git cwd to
/// "skip" so an unattended run never blocks on stdin.
struct DefaultSkipPrompts;

impl PromptBackend for DefaultSkipPrompts {
    fn prompt_non_git_cwd(&mut self, _cwd: &Path, _synced_dir: Option<&'static str>) -> PromptResult {
        PromptResult { disposition: PromptedDisposition::Skip, synced_dir_confirmed: None }
    }
}
