use clap::Parser;

use memoryd::cli::{Cli, Command};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => {
            println!("memoryd serve socket={}", args.socket.display());
        }
        Command::Status(args) => {
            println!("memoryd status socket={}", args.socket.display());
        }
        Command::Doctor(args) => {
            println!("memoryd doctor repo={} runtime={}", args.repo.display(), args.runtime.display());
        }
        Command::Search(args) => {
            println!("memoryd search query={}", args.query);
        }
        Command::Get(args) => {
            println!("memoryd get id={}", args.id);
        }
        Command::WriteNote(args) => {
            println!("memoryd write-note text={}", args.text);
        }
    }
    Ok(())
}
