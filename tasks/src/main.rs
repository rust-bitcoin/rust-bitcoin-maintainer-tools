mod bench;
mod docs;
mod environment;
mod lint;
mod lock;
mod test;
mod toolchain;

use clap::{Parser, Subcommand};
use std::process;
use xshell::Shell;

use environment::{change_to_repo_root, configure_log_level};
use lock::LockFile;
use toolchain::Toolchain;

#[derive(Parser)]
#[command(name = "rbmt")]
#[command(about = "Rust Bitcoin Maintainer Tools", long_about = None)]
struct Cli {
    /// Lock file to use for dependencies (defaults to recent).
    #[arg(long, global = true, value_enum)]
    lock_file: Option<LockFile>,
    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the linter (clippy) for workspace and all crates.
    Lint,
    /// Build documentation with stable toolchain.
    Docs,
    /// Build documentation with nightly toolchain for docs.rs.
    Docsrs,
    /// Run benchmark tests for all crates.
    Bench,
    /// Run tests with specified toolchain.
    Test {
        /// Toolchain to use: stable, nightly, or msrv.
        #[arg(value_enum)]
        toolchain: Toolchain,
    },
    /// Update Cargo-minimal.lock and Cargo-recent.lock files.
    Lock,
}

fn main() {
    let cli = Cli::parse();
    let sh = Shell::new().unwrap();
    configure_log_level(&sh);
    change_to_repo_root(&sh);

    // Restore the specified lock file before running any command (except Lock itself).
    if let Some(lock_file) = cli.lock_file {
        if !matches!(cli.command, Commands::Lock) {
            if let Err(e) = lock::restore_lock_file(&sh, lock_file) {
                eprintln!("Error restoring lock file: {}", e);
                process::exit(1);
            }
        }
    }

    match cli.command {
        Commands::Lint => {
            if let Err(e) = lint::run(&sh) {
                eprintln!("Error running lint task: {}", e);
                process::exit(1);
            }
        }
        Commands::Docs => {
            if let Err(e) = docs::run(&sh) {
                eprintln!("Error building docs: {}", e);
                process::exit(1);
            }
        }
        Commands::Docsrs => {
            if let Err(e) = docs::run_docsrs(&sh) {
                eprintln!("Error building docs.rs docs: {}", e);
                process::exit(1);
            }
        }
        Commands::Bench => {
            if let Err(e) = bench::run(&sh) {
                eprintln!("Error running bench tests: {}", e);
                process::exit(1);
            }
        }
        Commands::Test { toolchain } => {
            if let Err(e) = test::run(&sh, toolchain) {
                eprintln!("Error running tests: {}", e);
                process::exit(1);
            }
        }
        Commands::Lock => {
            if let Err(e) = lock::run(&sh) {
                eprintln!("Error updating lock files: {}", e);
                process::exit(1);
            }
        }
    }
}
