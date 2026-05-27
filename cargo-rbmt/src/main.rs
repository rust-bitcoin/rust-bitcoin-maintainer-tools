// SPDX-License-Identifier: MIT AND Apache-2.0

// Allow all other modules to use environment macros.
#[macro_use]
mod environment;

mod api;
mod docs;
mod fmt;
mod generate;
mod git;
mod integration;
mod lint;
mod lock;
mod prerelease;
mod run;
mod test;
mod toolchain;
mod toolchains;
mod tools;

use std::process;

use clap::{Parser, Subcommand};
use lock::LockFile;
use toolchain::Toolchain;
use xshell::Shell;

#[derive(Parser)]
#[command(name = "cargo-rbmt")]
#[command(about = "Rust Bitcoin Maintainer Tools", long_about = None)]
struct Cli {
    /// Lock file to use for dependencies.
    #[arg(long = "lock-file", global = true, value_enum, default_value_t = LockFile::Recent)]
    lockfile: LockFile,

    /// Filter which packages are operated on in the workspace. Can be a package's manifest name or directory.
    #[arg(short = 'p', long = "package", global = true)]
    packages: Vec<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check for public API changes in stabilizing crates.
    Api {
        /// Git ref to use as baseline for semver comparison (tag, branch, or commit).
        #[arg(long)]
        baseline: Option<String>,
    },
    /// Format files using rustfmt with the nightly toolchain.
    Fmt {
        /// Check formatting without modifying files.
        #[arg(long)]
        check: bool,
    },
    /// Run the linter (clippy) for workspace and all crates.
    Lint,
    /// Build documentation with stable toolchain.
    Docs {
        /// Open documentation in browser after building.
        #[arg(long)]
        open: bool,
    },
    /// Build documentation with nightly toolchain for docs.rs.
    Docsrs {
        /// Open documentation in browser after building.
        #[arg(long)]
        open: bool,
    },
    /// Run tests with specified toolchain.
    Test {
        /// Toolchain to use: stable, nightly, or msrv.
        #[arg(long, value_enum, default_value_t = Toolchain::Stable)]
        toolchain: Toolchain,
        /// Test every commit between the given baseline ref and HEAD to verify bisectability.
        #[arg(long)]
        baseline: Option<String>,
        /// Cargo arguments (everything after `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        cargo_args: Vec<String>,
    },
    /// Run bitcoin core integration tests.
    Integration,
    /// Update Cargo-minimal.lock and Cargo-recent.lock files.
    Lock,
    /// Run arbitrary cargo commands with toolchain and lockfile management.
    Run {
        /// Toolchain to use: stable, nightly, or msrv.
        #[arg(long, value_enum, default_value_t = Toolchain::Stable)]
        toolchain: Toolchain,
        /// Cargo command and arguments (everything after `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run pre-release readiness checks.
    Prerelease {
        /// Run checks even for packages that have pre-release checks disabled.
        #[arg(long)]
        force: bool,
        /// Git ref to use as baseline for version bump detection (tag, branch, or commit).
        #[arg(long, default_value = "master")]
        baseline: String,
    },
    /// Install and manage nightly, stable, and MSRV toolchains.
    Toolchains {
        /// Update the nightly toolchain version.
        #[arg(long)]
        update_nightly: bool,
        /// Update the stable toolchain version.
        #[arg(long)]
        update_stable: bool,
        /// Print the workspace MSRV and exit without installing any toolchains.
        #[arg(long)]
        msrv: bool,
        /// Print the nightly toolchain version and exit without installing any toolchains.
        #[arg(long)]
        nightly: bool,
        /// Print the stable toolchain version and exit without installing any toolchains.
        #[arg(long)]
        stable: bool,
    },
    /// Install tools pinned in [workspace.metadata.rbmt.tools].
    Tools {
        /// Install each tool at its latest version and update pins in the root manifest.
        #[arg(long)]
        update: bool,
        /// Only operate on these tools (default: all tools in the manifest).
        tools: Vec<String>,
    },
    /// Print version and build information.
    Version,
    /// Generate files and check for changes.
    Generate,
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

    match cli.command {
        Commands::Version => println!("{}", env!("RBMT_BUILD_VERSION")),
        Commands::Api { baseline } => {
            if let Err(e) = api::run(&sh, cli.lockfile, &cli.packages, baseline.as_deref()) {
                eprintln!("Error running API check: {}", e);
                process::exit(1);
            }
        }
        Commands::Fmt { check } =>
            if let Err(e) = fmt::run(&sh, check, &cli.packages) {
                eprintln!("Error running fmt task: {}", e);
                process::exit(1);
            },
        Commands::Lint =>
            if let Err(e) = lint::run(&sh, cli.lockfile, &cli.packages) {
                eprintln!("Error running lint task: {}", e);
                process::exit(1);
            },
        Commands::Docs { open } =>
            if let Err(e) = docs::run(&sh, cli.lockfile, &cli.packages, open) {
                eprintln!("Error building docs: {}", e);
                process::exit(1);
            },
        Commands::Docsrs { open } =>
            if let Err(e) = docs::run_docsrs(&sh, cli.lockfile, &cli.packages, open) {
                eprintln!("Error building docs.rs docs: {}", e);
                process::exit(1);
            },
        Commands::Test { toolchain, baseline, cargo_args } =>
            if let Err(e) = test::run(
                &sh,
                cli.lockfile,
                toolchain,
                baseline.as_deref(),
                &cli.packages,
                &cargo_args,
            ) {
                eprintln!("Error running tests: {}", e);
                process::exit(1);
            },
        Commands::Integration =>
            if let Err(e) = integration::run(&sh, &cli.packages) {
                eprintln!("Error running integration tests: {}", e);
                process::exit(1);
            },
        Commands::Lock =>
            if let Err(e) = lock::run(&sh) {
                eprintln!("Error updating lock files: {}", e);
                process::exit(1);
            },
        Commands::Run { toolchain, args } =>
            if let Err(e) = run::run(&sh, cli.lockfile, toolchain, args) {
                eprintln!("Error running cargo command: {}", e);
                process::exit(1);
            },
        Commands::Prerelease { force, baseline } =>
            if let Err(e) = prerelease::run(&sh, &cli.packages, force, &baseline) {
                eprintln!("Error running pre-release checks: {}", e);
                process::exit(1);
            },
        Commands::Toolchains { update_nightly, update_stable, msrv, nightly, stable } =>
            if let Err(e) =
                toolchains::run(&sh, update_nightly, update_stable, msrv, nightly, stable)
            {
                eprintln!("Error setting up toolchains: {}", e);
                process::exit(1);
            },
        Commands::Tools { update, tools } =>
            if let Err(e) = tools::run(&sh, update, &tools) {
                eprintln!("Error managing tools: {}", e);
                process::exit(1);
            },
        Commands::Generate =>
            if let Err(e) = generate::run(&sh, &cli.packages) {
                eprintln!("Error running file generation: {}", e);
                process::exit(1);
            },
    }
}
