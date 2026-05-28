use std::io::IsTerminal;

use super::UiArgs;

pub fn run(args: UiArgs) -> anyhow::Result<()> {
    run_tui(args)
}

fn run_tui(args: UiArgs) -> anyhow::Result<()> {
    if let Err(error) = crate::cli::validate_ui_stdin(std::io::stdin().is_terminal()) {
        eprintln!("{}", error.message());
        std::process::exit(error.exit_code());
    }

    let current_exe = std::env::current_exe()?;
    let path_env = std::env::var_os("PATH");
    let binary = match crate::cli::resolve_memoryd_tui_binary(&current_exe, path_env.as_deref()) {
        Ok(binary) => binary,
        Err(error) => {
            eprintln!("{}", error.message());
            std::process::exit(error.exit_code());
        }
    };

    let status = std::process::Command::new(binary).args(crate::cli::ui_subprocess_args(&args)).status()?;
    std::process::exit(status.code().unwrap_or(1));
}
