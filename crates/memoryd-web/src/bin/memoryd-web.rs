use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use memoryd_web::{run_with_state, DashboardAuthToken, WebConfig, WebState, DASHBOARD_AUTH_ENV};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse(std::env::args_os().skip(1))?;
    let auth = dashboard_auth_token()?;
    // A daemon-spawned child inherits the token via the env var and the daemon
    // surfaces the launch URL itself. When the binary is run standalone the token
    // is freshly generated, so we must print its launch URL here or the operator
    // has no way to authenticate — a silent lockout.
    if let DashboardAuth::Generated(token) = &auth {
        eprintln!("memoryd-web: open http://localhost:{}/?auth={}", args.port, token.as_str());
    }
    let state = WebState::daemon(args.socket)
        .with_policy_dir(args.repo.join("policies"))
        .with_dashboard_auth_token(auth.into_token());
    run_with_state(
        WebConfig { enabled: true, bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST), port: args.port },
        state,
        shutdown_signal(),
    )
    .await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// The dashboard auth token together with how it was obtained. The provenance
/// matters: a supplied token came from the spawning daemon (which prints its own
/// launch URL), whereas a generated one is only known to this process and must be
/// surfaced to the operator.
enum DashboardAuth {
    Supplied(DashboardAuthToken),
    Generated(DashboardAuthToken),
}

impl DashboardAuth {
    fn into_token(self) -> DashboardAuthToken {
        match self {
            Self::Supplied(token) | Self::Generated(token) => token,
        }
    }
}

fn dashboard_auth_token() -> anyhow::Result<DashboardAuth> {
    match std::env::var(DASHBOARD_AUTH_ENV) {
        Ok(value) => DashboardAuthToken::from_hex(value)
            .map(DashboardAuth::Supplied)
            .ok_or_else(|| anyhow::anyhow!("{DASHBOARD_AUTH_ENV} must be a 64-character hex token")),
        Err(std::env::VarError::NotPresent) => Ok(DashboardAuth::Generated(DashboardAuthToken::generate())),
        Err(error) => Err(error.into()),
    }
}

struct Args {
    socket: PathBuf,
    port: u16,
    repo: PathBuf,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = std::ffi::OsString>) -> anyhow::Result<Self> {
        let mut socket = PathBuf::from("/tmp/memoryd.sock");
        let mut port = 7137_u16;
        let mut repo = std::env::current_dir()?;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.to_string_lossy().as_ref() {
                "--socket" => {
                    let value = args.next().ok_or_else(|| anyhow::anyhow!("--socket requires a value"))?;
                    socket = PathBuf::from(value);
                }
                "--port" => {
                    let value = args.next().ok_or_else(|| anyhow::anyhow!("--port requires a value"))?;
                    port = value.to_string_lossy().parse()?;
                }
                "--repo" => {
                    let value = args.next().ok_or_else(|| anyhow::anyhow!("--repo requires a value"))?;
                    repo = PathBuf::from(value);
                }
                other => anyhow::bail!("unknown argument {other}"),
            }
        }
        if port < 1024 {
            anyhow::bail!("--port must be in 1024..=65535");
        }
        Ok(Self { socket, port, repo })
    }
}
