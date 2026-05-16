use std::num::NonZeroU64;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use memorum_theme::{Charset, ColorCapability};
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
    tick_ms: Option<NonZeroU64>,
    #[arg(long)]
    daemon_poll_ms: Option<NonZeroU64>,
    #[arg(long)]
    theme: Option<String>,
    #[arg(long)]
    theme_config: Option<PathBuf>,
    #[arg(long)]
    charset: Option<String>,
    #[arg(long)]
    no_motion: bool,
    #[arg(long)]
    color_capability: Option<String>,
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

    let config = resolve_config(args)?;

    #[cfg(debug_assertions)]
    if config.inject_panic_mid_render {
        return app::run_with_mid_render_panic(config.ui).await;
    }
    app::run(config.ui).await
}

struct ResolvedConfig {
    ui: UiConfig,
    #[cfg(debug_assertions)]
    inject_panic_mid_render: bool,
}

fn resolve_config(args: Args) -> Result<ResolvedConfig> {
    let mut config = match args.config {
        Some(path) => UiConfig::from_config_yaml(path)?,
        None => UiConfig::default(),
    };
    if let Some(socket) = args.socket {
        config.socket_path = socket;
    }
    if let Some(tick_ms) = args.tick_ms {
        config.tick_interval = Duration::from_millis(tick_ms.get());
    }
    if let Some(daemon_poll_ms) = args.daemon_poll_ms {
        config.daemon_poll_interval = Duration::from_millis(daemon_poll_ms.get());
    }
    if let Some(theme) = args.theme {
        config.theme = theme;
    }
    if let Some(theme_config) = args.theme_config {
        config.theme_config = Some(theme_config);
    }
    if let Some(charset) = args.charset {
        config.charset = parse_charset(&charset)?;
    }
    config.no_motion |= args.no_motion;
    if let Some(capability) = args.color_capability {
        config.color_capability = Some(parse_color_capability(&capability)?);
    }

    #[cfg(debug_assertions)]
    {
        Ok(ResolvedConfig { ui: config, inject_panic_mid_render: args.inject_panic_mid_render })
    }
    #[cfg(not(debug_assertions))]
    {
        Ok(ResolvedConfig { ui: config })
    }
}

fn parse_charset(value: &str) -> Result<Charset> {
    match value {
        "full" => Ok(Charset::Full),
        "extended" => Ok(Charset::Extended),
        "minimal" => Ok(Charset::Minimal),
        other => anyhow::bail!("unsupported charset `{other}`"),
    }
}

fn parse_color_capability(value: &str) -> Result<ColorCapability> {
    match value {
        "truecolor" | "24bit" => Ok(ColorCapability::TrueColor),
        "256" | "256color" => Ok(ColorCapability::Indexed256),
        "16" | "ansi" => Ok(ColorCapability::Indexed16),
        "mono" | "monochrome" => Ok(ColorCapability::Monochrome),
        other => anyhow::bail!("unsupported color capability `{other}`"),
    }
}

fn install_panic_terminal_restore_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        app::restore_terminal_blocking();
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_rejects_zero_tick_interval() {
        let error = Args::try_parse_from(["memoryd-tui", "--tick-ms", "0"]).expect_err("zero tick rejected");
        assert!(error.to_string().contains("invalid value"));
    }

    #[test]
    fn cli_rejects_zero_daemon_poll_interval() {
        let error = Args::try_parse_from(["memoryd-tui", "--daemon-poll-ms", "0"]).expect_err("zero poll rejected");
        assert!(error.to_string().contains("invalid value"));
    }

    #[test]
    fn cli_accepts_positive_intervals() {
        let args = Args::try_parse_from(["memoryd-tui", "--tick-ms", "1", "--daemon-poll-ms", "2"])
            .expect("positive intervals parse");

        assert_eq!(args.tick_ms.map(NonZeroU64::get), Some(1));
        assert_eq!(args.daemon_poll_ms.map(NonZeroU64::get), Some(2));
    }

    #[test]
    fn config_theme_survives_when_theme_arg_is_omitted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("config.yaml");
        std::fs::write(&config_path, "ui:\n  theme: default-light\n").expect("write config");
        let args = Args::try_parse_from(["memoryd-tui", "--config", config_path.to_str().expect("utf8 path")])
            .expect("args parse");

        let config = resolve_config(args).expect("resolve config").ui;

        assert_eq!(config.theme, "default-light");
    }

    #[test]
    fn explicit_theme_arg_overrides_config_theme() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("config.yaml");
        std::fs::write(&config_path, "ui:\n  theme: default-light\n").expect("write config");
        let args = Args::try_parse_from([
            "memoryd-tui",
            "--config",
            config_path.to_str().expect("utf8 path"),
            "--theme",
            "kanagawa",
        ])
        .expect("args parse");

        let config = resolve_config(args).expect("resolve config").ui;

        assert_eq!(config.theme, "kanagawa");
    }
}
