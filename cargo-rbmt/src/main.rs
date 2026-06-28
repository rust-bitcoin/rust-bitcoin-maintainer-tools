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
mod version;

use std::process;

use clap::{Parser, Subcommand};
use lock::{GeneratableLockFile, LockFile};
use toolchain::Toolchain;
use xshell::Shell;

#[derive(Parser)]
#[command(name = "cargo-rbmt")]
#[command(about = "Rust Bitcoin Maintainer Tools", long_about = None)]
struct Cli {
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
        /// Lockfile to use for dependencies.
        #[arg(long = "lockfile", alias = "lock-file", value_enum, default_value_t = LockFile::Recent)]
        lockfile: LockFile,
        /// Git ref to use as baseline for semver comparison (tag, branch, or commit).
        #[arg(long)]
        baseline: Option<String>,
        /// Write API snapshot files.
        #[arg(long)]
        snapshot: bool,
    },
    /// Format files using rustfmt with the nightly toolchain.
    Fmt {
        /// Check formatting without modifying files.
        #[arg(long)]
        check: bool,
    },
    /// Run the linter (clippy) for workspace and all crates.
    Lint {
        /// Lockfile to use for dependencies.
        #[arg(long = "lockfile", alias = "lock-file", value_enum, default_value_t = LockFile::Recent)]
        lockfile: LockFile,
    },
    /// Build documentation at rust-bitcoin standards.
    Docs {
        /// Lockfile to use for dependencies.
        #[arg(long = "lockfile", alias = "lock-file", value_enum, default_value_t = LockFile::Recent)]
        lockfile: LockFile,
        /// Build with stable toolchain instead of nightly and skip docs.rs validation.
        #[arg(long)]
        no_docsrs: bool,
        /// Open documentation in browser after building.
        #[arg(long)]
        open: bool,
    },
    /// Run tests with specified toolchain.
    Test {
        /// Lockfile to use for dependencies.
        #[arg(long = "lockfile", alias = "lock-file", value_enum, default_value_t = LockFile::Recent)]
        lockfile: LockFile,
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
    /// Update dependency versions in lockfiles.
    Lock {
        /// Lockfile types to generate.
        #[arg(long, value_delimiter = ',', default_values = ["minimal", "recent"])]
        lockfiles: Vec<GeneratableLockFile>,
    },
    /// Run arbitrary cargo commands with toolchain and lockfile management.
    Run {
        /// Lockfile to use for dependencies.
        #[arg(long = "lockfile", alias = "lock-file", value_enum, default_value_t = LockFile::Recent)]
        lockfile: LockFile,
        /// Toolchain to use: stable, nightly, or msrv.
        #[arg(long, value_enum, default_value_t = Toolchain::Stable)]
        toolchain: Toolchain,
        /// Run the command on every commit between the given baseline ref and HEAD to ensure consistency.
        #[arg(long)]
        baseline: Option<String>,
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

    // Check version requirement early before running any commands
    if let Err(e) = version::check(&sh) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }

    match cli.command {
        Commands::Version => println!("{}", env!("RBMT_BUILD_VERSION")),
        Commands::Api { lockfile, baseline, snapshot } => {
            if let Err(e) = api::run(&sh, lockfile, &cli.packages, baseline.as_deref(), snapshot) {
                eprintln!("Error running API check: {}", e);
                process::exit(1);
            }
        }
        Commands::Fmt { check } =>
            if let Err(e) = fmt::run(&sh, check, &cli.packages) {
                eprintln!("Error running fmt task: {}", e);
                process::exit(1);
            },
        Commands::Lint { lockfile } =>
            if let Err(e) = lint::run(&sh, lockfile, &cli.packages) {
                eprintln!("Error running lint task: {}", e);
                process::exit(1);
            },
        Commands::Docs { lockfile, no_docsrs, open } => {
            let mode = if no_docsrs { docs::DocsMode::Docs } else { docs::DocsMode::DocsRs };
            if let Err(e) = docs::run(&sh, lockfile, &cli.packages, mode, open) {
                eprintln!("Error building docs: {}", e);
                process::exit(1);
            }
        }
        Commands::Test { lockfile, toolchain, baseline, cargo_args } =>
            if let Err(e) =
                test::run(&sh, lockfile, toolchain, baseline.as_deref(), &cli.packages, &cargo_args)
            {
                eprintln!("Error running tests: {}", e);
                process::exit(1);
            },
        Commands::Integration =>
            if let Err(e) = integration::run(&sh, &cli.packages) {
                eprintln!("Error running integration tests: {}", e);
                process::exit(1);
            },
        Commands::Lock { lockfiles } =>
            if let Err(e) = lock::run(&sh, &lockfiles) {
                eprintln!("Error updating lockfiles: {}", e);
                process::exit(1);
            },
        Commands::Run { lockfile, toolchain, baseline, args } =>
            if let Err(e) =
                run::run(&sh, lockfile, toolchain, baseline.as_deref(), &cli.packages, &args)
            {
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
