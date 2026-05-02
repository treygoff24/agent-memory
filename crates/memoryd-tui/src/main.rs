use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use memoryd_tui::app;
use memoryd_tui::config::UiConfig;

#[derive(Debug, Parser)]
#[command(name = "memoryd-tui", about = "Memorum daemon terminal dashboard")]
struct Args {
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    tick_ms: Option<u64>,
    #[arg(long)]
    daemon_poll_ms: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut config = match args.config {
        Some(path) => UiConfig::from_config_yaml(path)?,
        None => UiConfig::default(),
    };

    if let Some(socket) = args.socket {
        config.socket_path = socket;
    }
    if let Some(tick_ms) = args.tick_ms {
        config.tick_interval = std::time::Duration::from_millis(tick_ms);
    }
    if let Some(daemon_poll_ms) = args.daemon_poll_ms {
        config.daemon_poll_interval = std::time::Duration::from_millis(daemon_poll_ms);
    }

    app::run(config).await
}
