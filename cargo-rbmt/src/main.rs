mod bench;
mod docs;
mod environment;
mod integration;
mod lint;
mod lock;
mod prerelease;
mod test;
mod toolchain;

use clap::{Parser, Subcommand};
use std::process;
use xshell::Shell;

use environment::{change_to_repo_root, configure_log_level};
use lock::LockFile;
use toolchain::Toolchain;

#[derive(Parser)]
#[command(name = "cargo-rbmt")]
#[command(about = "Rust Bitcoin Maintainer Tools", long_about = None)]
struct Cli {
    /// Lock file to use for dependencies (defaults to recent).
    #[arg(long, global = true, value_enum)]
    lock_file: Option<LockFile>,

    /// Filter to specific package (can be specified multiple times).
    #[arg(short = 'p', long = "package", global = true)]
    packages: Vec<String>,

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
        /// Disable debug assertions in compiled code.
        #[arg(long)]
        no_debug_assertions: bool,
    },
    /// Run bitcoin core integration tests.
    Integration,
    /// Update Cargo-minimal.lock and Cargo-recent.lock files.
    Lock,
    /// Run pre-release readiness checks.
    Prerelease,
}

fn main() {
    // Cargo automatically adds the subcommand name as an extra argument.
    // `cargo rbmt test` becomes `cargo-rbmt rbmt test`, so filter it out.
    let args = std::env::args()
        .enumerate()
        .filter(|(i, arg)| !(*i == 1 && arg == "rbmt"))
        .map(|(_, arg)| arg);

    let cli = Cli::parse_from(args);

    let sh = Shell::new().unwrap();
    configure_log_level(&sh);
    change_to_repo_root(&sh);

    // Restore the specified lock file before running any command (except Lock and Integration).
    // Integration tests use their own lock files in the integration package directory.
    if let Some(lock_file) = cli.lock_file {
        if !matches!(cli.command, Commands::Lock | Commands::Integration) {
            if let Err(e) = lock::restore_lock_file(&sh, lock_file) {
                eprintln!("Error restoring lock file: {}", e);
                process::exit(1);
            }
        }
    }

    match cli.command {
        Commands::Lint => {
            if let Err(e) = lint::run(&sh, &cli.packages) {
                eprintln!("Error running lint task: {}", e);
                process::exit(1);
            }
        }
        Commands::Docs => {
            if let Err(e) = docs::run(&sh, &cli.packages) {
                eprintln!("Error building docs: {}", e);
                process::exit(1);
            }
        }
        Commands::Docsrs => {
            if let Err(e) = docs::run_docsrs(&sh, &cli.packages) {
                eprintln!("Error building docs.rs docs: {}", e);
                process::exit(1);
            }
        }
        Commands::Bench => {
            if let Err(e) = bench::run(&sh, &cli.packages) {
                eprintln!("Error running bench tests: {}", e);
                process::exit(1);
            }
        }
        Commands::Test {
            toolchain,
            no_debug_assertions,
        } => {
            if let Err(e) = test::run(&sh, toolchain, no_debug_assertions, &cli.packages) {
                eprintln!("Error running tests: {}", e);
                process::exit(1);
            }
        }
        Commands::Integration => {
            if let Err(e) = integration::run(&sh, &cli.packages) {
                eprintln!("Error running integration tests: {}", e);
                process::exit(1);
            }
        }
        Commands::Lock => {
            if let Err(e) = lock::run(&sh) {
                eprintln!("Error updating lock files: {}", e);
                process::exit(1);
            }
        }
        Commands::Prerelease => {
            if let Err(e) = prerelease::run(&sh, &cli.packages) {
                eprintln!("Error running pre-release checks: {}", e);
                process::exit(1);
            }
        }
    }
}
