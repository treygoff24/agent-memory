use std::path::PathBuf;

use super::InitArgs;
use crate::import::discovery::{discover_claude_memory_root, discover_codex_memory_root};

pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    // Default repo and runtime paths mirror `scripts/install-memorum.sh`.
    let default_repo = std::env::var("MEMORUM_REPO")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join("memorum")))
        .unwrap_or_else(|| PathBuf::from("./memorum"));
    let repo = args.repo.clone().unwrap_or(default_repo);
    let runtime = args.runtime.clone().unwrap_or_else(|| repo.join(".memoryd"));
    let socket = runtime.join("memoryd.sock");

    println!("Memorum init");
    println!("  repo:    {}", repo.display());
    println!("  runtime: {}", runtime.display());
    println!("  socket:  {}", socket.display());
    println!();

    let already_initialised = repo.join(".memorum").exists();
    if already_initialised {
        println!("Detected existing Memorum substrate at {}.", repo.display());
        println!("Running detection-only: no re-init, no destructive changes.");
        println!("If you want to re-import harness memory, run `memoryd import` explicitly.");
        println!();
    }

    let claude_root = discover_claude_memory_root(None)?;
    let codex_root = discover_codex_memory_root(None)?;

    let claude_count = match &claude_root {
        Some(root) if root.path.exists() => walkdir::WalkDir::new(&root.path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().and_then(std::ffi::OsStr::to_str) == Some("md"))
            .filter(|entry| entry.path().file_name().and_then(std::ffi::OsStr::to_str) != Some("MEMORY.md"))
            .count(),
        _ => 0,
    };
    let codex_present = match &codex_root {
        Some(root) => root.path.join("MEMORY.md").exists(),
        None => false,
    };

    println!("Detected harness memory:");
    println!("  Claude Code: {claude_count} memory topic file(s)");
    println!("  Codex CLI:   {}", if codex_present { "MEMORY.md present" } else { "not found" });
    println!();

    let any = claude_count > 0 || codex_present;
    if !any {
        println!(
            "Nothing to import. Run `memoryd serve --init --repo \"{}\" --runtime \"{}\"` to start the daemon.",
            repo.display(),
            runtime.display()
        );
        return Ok(());
    }

    if args.non_interactive {
        println!("--non-interactive: skipping import prompt; run `memoryd import` later when ready.");
        return Ok(());
    }

    let proceed = dialoguer::Confirm::new()
        .with_prompt("Would you like to import detected harness memory now?")
        .default(true)
        .interact()
        .unwrap_or(false);

    if !proceed {
        println!("Skipped import. Run `memoryd import` later when ready.");
        return Ok(());
    }

    println!();
    println!("Run this command in a separate shell once the daemon is up:");
    println!("  memoryd import --repo \"{}\" --socket \"{}\"", repo.display(), socket.display(),);
    println!();
    println!("Next steps:");
    println!(
        "  - Start daemon: memoryd serve --init --repo \"{}\" --runtime \"{}\" --socket \"{}\"",
        repo.display(),
        runtime.display(),
        socket.display()
    );
    println!("  - Health check: memoryd doctor --repo \"{}\" --runtime \"{}\"", repo.display(), runtime.display());
    println!("  - Troubleshooting: docs/troubleshooting.md");
    println!("  - Importer details: docs/importer.md");
    Ok(())
}
