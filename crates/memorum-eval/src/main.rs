use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;
use memorum_eval::orchestrator::{self, EvalOrchestrator, OutputFormat};
use memorum_eval::EvalCli;

fn main() -> ExitCode {
    let cli = EvalCli::parse();
    if cli.list {
        print!("{}", memorum_eval::orchestrator::format_catalog());
        return ExitCode::SUCCESS;
    }

    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("memorum-eval orchestrator error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: EvalCli) -> Result<u8, Box<dyn std::error::Error>> {
    let output_format = cli.output.unwrap_or_else(default_output_format);
    let output_file = cli.output_file.clone();
    let report = EvalOrchestrator.run_with_config(cli.run_config())?;
    let json = orchestrator::report_to_json(&report);

    if let Some(path) = output_file {
        std::fs::write(path, &json)?;
    }

    match output_format {
        OutputFormat::Json => print!("{json}"),
        OutputFormat::Text => print!("{}", orchestrator::report_to_text(&report)),
    }

    Ok(orchestrator::exit_code_for_report(&report))
}

fn default_output_format() -> OutputFormat {
    if std::io::stdout().is_terminal() {
        OutputFormat::Text
    } else {
        OutputFormat::Json
    }
}
