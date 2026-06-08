use clap::Parser;

use memoryd::cli::{self, Cli, Command};

// Multi-threaded runtime so a slow synchronous substrate call (a large vector
// ANN scan, a reindex, or — once SyncManager wires real git — a network push)
// runs on a worker thread instead of blocking the single executor that also
// drives the accept/dispatch loop and every other in-flight connection. The
// `rt-multi-thread` feature is already enabled workspace-wide; tokio defaults
// the worker count to the available parallelism.
#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => cli::serve::run(args).await?,
        Command::Mcp(args) => cli::daemon::run_mcp(args).await?,
        Command::Status(args) => cli::daemon::run_status(args).await?,
        Command::Doctor(args) => cli::daemon::run_doctor(args).await?,
        Command::Search(args) => cli::memory::run_search(args).await?,
        Command::Get(args) => cli::memory::run_get(args).await?,
        Command::WriteNote(args) => cli::memory::run_write_note(args).await?,
        Command::Write(args) => cli::memory::run_write(args).await?,
        Command::Source(args) => cli::source::run(args).await?,
        Command::Supersede(args) => cli::memory::run_supersede(args).await?,
        Command::Forget(args) => cli::memory::run_forget(args).await?,
        Command::Review(args) => cli::review::run(args).await?,
        Command::Recall(args) => cli::recall::run(args).await?,
        Command::Dream(args) => cli::dream::run(args).await?,
        Command::Peer(args) => cli::peer::run(args).await?,
        Command::Ui(args) => cli::ui::run(args)?,
        Command::Web(args) => cli::web::run(args).await?,
        Command::RealityCheck(args) => cli::reality_check::run(args).await?,
        Command::Privacy(args) => cli::privacy::run_privacy(args).await?,
        Command::PrivacyFilter(args) => cli::privacy::run_privacy_filter(args).await?,
        Command::Device(args) => cli::privacy::run_device(args).await?,
        Command::Export(args) => {
            if let Err(err) = memoryd::export::run_export(args).await {
                eprintln!("error: {err}");
                std::process::exit(err.exit_code());
            }
        }
        Command::Import(args) => cli::import::run(args).await?,
        Command::Init(args) => cli::init::run(args).await?,
    }
    Ok(())
}
