mod environment;
mod lint;
mod toolchain;

use clap::{Parser, Subcommand};
use std::process;
use xshell::Shell;

use environment::{change_to_repo_root, configure_log_level};

#[derive(Parser)]
#[command(name = "rbmt")]
#[command(about = "Rust Bitcoin Maintainer Tools", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the linter (clippy) for workspace and all crates.
    Lint,
}

fn main() {
    let cli = Cli::parse();
    let sh = Shell::new().unwrap();
    configure_log_level(&sh);
    change_to_repo_root(&sh);

    match cli.command {
        Commands::Lint => {
            if let Err(e) = lint::run(&sh) {
                eprintln!("Error running lint task: {}", e);
                process::exit(1);
            }
        }
    }
}
