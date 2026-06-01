use crate::cli::paths::resolve_socket_arg;
use crate::cli::{ImportArgs, ImportHarness, NonGitCwdDefault};
use crate::import::pipeline::{run_import_session, ExecuteOptions, HarnessFilter, ImportOptions, SocketDaemonClient};
use crate::import::project_map::{
    FixedDispositionBackend, InteractivePromptBackend, PromptBackend, PromptedDisposition,
};

pub async fn run(args: ImportArgs) -> anyhow::Result<()> {
    let harness_filter = match args.harness {
        ImportHarness::All => None,
        ImportHarness::Claude => Some(HarnessFilter::Claude),
        ImportHarness::Codex => Some(HarnessFilter::Codex),
    };

    // Non-interactive callers (e.g. `memoryd init --non-interactive`) get a
    // default-skip prompt backend; everyone else gets the dialoguer-backed one.
    let mut prompts = prompt_backend(args.non_git_cwd_default);

    let socket = resolve_socket_arg(&args.socket);
    let mut client = SocketDaemonClient::new(socket);
    let execute_opts = ExecuteOptions { dry_run: args.dry_run, verbose_progress: !args.quiet };
    let result = run_import_session(
        &args.repo,
        ImportOptions {
            from_claude: args.from_claude.clone(),
            from_codex: args.from_codex.clone(),
            harness_filter,
            state: None,
            plan_only: args.dry_run,
        },
        prompts.as_mut(),
        &mut client,
        execute_opts,
    )
    .await
    .map_err(|error| anyhow::anyhow!("run import: {error}"))?;

    println!("{}", result.report.to_text());
    if let Some(path) = &args.report {
        std::fs::write(path, result.report.to_json()?)?;
        if !args.quiet {
            eprintln!("Report written to {}", path.display());
        }
    }
    Ok(())
}

fn prompt_backend(default: Option<NonGitCwdDefault>) -> Box<dyn PromptBackend> {
    if let Some(default) = default {
        return Box::new(FixedDispositionBackend::new(match default {
            NonGitCwdDefault::Skip => PromptedDisposition::Skip,
            NonGitCwdDefault::Me => PromptedDisposition::DropToMe,
            NonGitCwdDefault::Generate => PromptedDisposition::GenerateProjectYaml,
        }));
    }

    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        Box::new(InteractivePromptBackend)
    } else {
        Box::new(FixedDispositionBackend::new(PromptedDisposition::Skip))
    }
}
