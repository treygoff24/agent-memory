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
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    inject_panic: bool,
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    inject_panic_mid_render: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    install_panic_terminal_restore_hook();
    let args = Args::parse();
    #[cfg(debug_assertions)]
    if args.inject_panic {
        panic!("injected memoryd-tui panic");
    }

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

    #[cfg(debug_assertions)]
    if args.inject_panic_mid_render {
        return app::run_with_mid_render_panic(config).await;
    }

    app::run(config).await
}

fn install_panic_terminal_restore_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        app::restore_terminal_blocking();
        default_hook(info);
    }));
}
