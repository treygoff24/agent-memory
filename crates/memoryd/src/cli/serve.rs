use chrono::Utc;
use memory_privacy::install_runtime_enforcement;
use memory_substrate::{InitOptions, Roots, Substrate};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use crate::cli::paths::resolve_socket_with_runtime;
use crate::cli::ServeArgs;
use crate::server::{self, ServerOptions};
use crate::state;

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    let roots = Roots::new(args.repo, args.runtime);
    let loaded_config =
        memory_substrate::config::load_config(&roots.repo, &roots.runtime, None).map_err(anyhow::Error::msg)?;
    let enforcement = loaded_config.privacy_enforcement();
    match install_runtime_enforcement(enforcement) {
        Ok(()) => tracing::info!(
            classifier = enforcement.classifier,
            encryption = enforcement.encryption,
            masking = enforcement.masking,
            "privacy enforcement installed"
        ),
        Err(error) => tracing::warn!(%error, "privacy enforcement already installed; keeping first config"),
    }
    let substrate = if args.init {
        if args.force_unsafe_durability {
            tracing::warn!(
                operator = "memoryd serve --init",
                reason = "--force-unsafe-durability supplied",
                "unsafe best-effort durability enabled for substrate init"
            );
        }
        Substrate::init(roots, InitOptions { force_unsafe_durability: args.force_unsafe_durability, device_id: None })
            .await?
    } else {
        Substrate::open(roots).await?
    };
    let runtime_root = substrate.roots().runtime.clone();
    let _daemon_state = state::DaemonState::load(&runtime_root);
    if let Err(error) = state::RcSessionStore::new(&runtime_root).load_if_recent(Utc::now()) {
        eprintln!("warning: failed to recover daemon session state: {error}");
    }

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(install_termination_handler(shutdown_tx));

    let socket = resolve_socket_with_runtime(&args.socket, &runtime_root);
    server::serve_substrate_with(socket, substrate, ServerOptions::default(), shutdown_rx).await?;
    Ok(())
}

/// Wait for the first SIGINT or SIGTERM and signal the daemon to shut down.
async fn install_termination_handler(shutdown: watch::Sender<bool>) {
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(handler) => handler,
        Err(error) => {
            eprintln!("memoryd: failed to install SIGINT handler: {error}");
            return;
        }
    };
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(handler) => handler,
        Err(error) => {
            eprintln!("memoryd: failed to install SIGTERM handler: {error}");
            return;
        }
    };

    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
    let _ = shutdown.send(true);
}
