use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use memoryd_web::{run_with_state, WebConfig, WebState};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse(std::env::args_os().skip(1))?;
    run_with_state(
        WebConfig { enabled: true, bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST), port: args.port },
        WebState::daemon(args.socket).with_policy_dir(args.repo.join("policies")),
        shutdown_signal(),
    )
    .await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
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
