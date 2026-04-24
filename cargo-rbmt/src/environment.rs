use std::path::{Path, PathBuf};
use std::{env, fs};

use xshell::{Cmd, Shell};

/// Environment variable to control output verbosity.
const LOG_LEVEL_ENV_VAR: &str = "RBMT_LOG_LEVEL";

/// Controls how much output is shown during command execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    /// Show all output from commands (default).
    Verbose,
    /// Suppress tool stderr, but show progress for interactive use.
    Progress,
    /// Suppress all stderr.
    Quiet,
}

impl OutputMode {
    /// Determine output mode from `RBMT_LOG_LEVEL` environment variable.
    pub fn from_env() -> Self {
        match env::var(LOG_LEVEL_ENV_VAR).as_deref() {
            Ok("progress") => Self::Progress,
            Ok("quiet") => Self::Quiet,
            _ => Self::Verbose,
        }
    }
}

/// Extension trait for commands with conditional output and release support.
pub trait CmdExt {
    /// Run command and show output only in Verbose mode, but always show on failure.
    fn run_verbose(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    /// Conditionally append `--release` flag.
    fn set_release(self, release: bool) -> Self;
}

impl CmdExt for Cmd<'_> {
    fn run_verbose(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Unconditionally grab stdout/stderr and ignore the exit status
        // since we handle piping it out below based on if a build
        // or test command fails.
        self.set_ignore_stdout(false);
        self.set_ignore_stderr(false);
        self.set_ignore_status(true);

        // Run command and capture output.
        let output = self.output()?;

        // Pipe out stderr and stdout in verbose mode or on failure.
        if matches!(OutputMode::from_env(), OutputMode::Verbose) || !output.status.success() {
            eprint!("{}", String::from_utf8(output.stderr)?);
            print!("{}", String::from_utf8(output.stdout)?);
        }

        // Err on command failure.
        if !output.status.success() {
            return Err(format!("Command failed: {}", output.status).into());
        }

        Ok(())
    }

    fn set_release(self, release: bool) -> Self {
        if release {
            self.arg("--release")
        } else {
            self
        }
    }
}

/// Guard that clears the progress line on stderr when dropped if in Progress mode.
pub struct ProgressGuard {
    disabled: bool,
}

impl ProgressGuard {
    /// Create a new guard that will clear the progress line on drop if in Progress mode.
    pub fn new() -> Self { Self { disabled: false } }

    /// Disable the guard, clearing the progress line.
    ///
    /// Useful when handling newlines externally, like when printing a summary on stdout.
    pub fn disable(&mut self) {
        self.disabled = true;
        if OutputMode::from_env() == OutputMode::Progress {
            eprintln!();
        }
    }
}

impl Default for ProgressGuard {
    fn default() -> Self { Self::new() }
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        if !self.disabled && OutputMode::from_env() == OutputMode::Progress {
            eprintln!();
        }
    }
}

/// A workspace package: its manifest name, directory path, and unique identifier.
#[derive(Clone, Debug)]
pub struct Package {
    /// The package name from the manifest.
    pub name: String,
    /// The directory path where the package is located.
    pub dir: PathBuf,
    /// The unique package identifier.
    pub id: String,
}

/// Wrap commands to respect rbmt output mode.
#[macro_export]
macro_rules! rbmt_cmd {
    ($sh:expr, $($arg:tt)*) => {{
        let mut cmd = xshell::cmd!($sh, $($arg)*);
        match $crate::environment::OutputMode::from_env() {
            $crate::environment::OutputMode::Verbose => {},
            $crate::environment::OutputMode::Progress | $crate::environment::OutputMode::Quiet => {
                // Do not print command and eat stderr.
                cmd = cmd.quiet().ignore_stderr();
            }
        }
        cmd
    }};
}

/// Progress message output symbols (flair).
pub const PROGRESS_SYMBOLS: &[&str] = &["b", "B", "$", "#"];

/// Progress output macro that respects [`OutputMode`] settings.
///
/// Wraps eprintln! so that the underlying macro's vararg handling is exposed.
#[macro_export]
macro_rules! rbmt_eprintln {
    ($($arg:tt)*) => {{
        match $crate::environment::OutputMode::from_env() {
            $crate::environment::OutputMode::Verbose => {
                eprintln!($($arg)*);
            }
            $crate::environment::OutputMode::Progress => {
                let msg = format!($($arg)*);
                // Show a symbol based on message hash.
                let hash = msg
                    .as_bytes()
                    .iter()
                    .fold(0usize, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as usize));
                let symbol = $crate::environment::PROGRESS_SYMBOLS[hash % $crate::environment::PROGRESS_SYMBOLS.len()];
                // Use carriage return to overwrite the same line, and ANSI escape to clear to EOL.
                eprint!("\r[{}] {}\x1b[K", symbol, msg);
            }
            $crate::environment::OutputMode::Quiet => {}
        }
    }};
}

/// Get list of package names and their directories in the workspace using cargo metadata.
/// Returns tuples of (`package_name`, `directory_path`) to support various workspace layouts including nested crates.
///
/// # Arguments
///
/// * `package_filter` - Optional filter for specific package names. If empty, returns all packages.
///
/// # Errors
///
/// Returns an error if any requested package name doesn't exist in the workspace.
pub fn get_workspace_packages(
    sh: &Shell,
    package_filter: &[String],
) -> Result<Vec<Package>, Box<dyn std::error::Error>> {
    let metadata = rbmt_cmd!(sh, "cargo metadata --no-deps --format-version 1").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    let all_packages: Vec<Package> = json["packages"]
        .as_array()
        .ok_or("Missing 'packages' field in cargo metadata")?
        .iter()
        .filter_map(|package| {
            Some(Package {
                name: package["name"].as_str()?.to_string(),
                dir: PathBuf::from(
                    package["manifest_path"].as_str()?.trim_end_matches("/Cargo.toml"),
                ),
                id: package["id"].as_str()?.to_string(),
            })
        })
        .collect();

    // If no package filter specified, return all packages.
    if package_filter.is_empty() {
        return Ok(all_packages);
    }

    // Resolve each requested string to a canonical manifest name,
    // falling back to directory basename matching if no manifest name matches.
    let mut resolved_names: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for requested in package_filter {
        // Exact manifest name match.
        if all_packages.iter().any(|pkg| &pkg.name == requested) {
            resolved_names.push(requested.clone());
            continue;
        }

        // Fall back to directory basename match.
        let dir_matches: Vec<&Package> = all_packages
            .iter()
            .filter(|pkg| {
                pkg.dir.file_name().and_then(|n| n.to_str()).is_some_and(|n| n == requested)
            })
            .collect();

        match dir_matches.len() {
            0 => {
                errors.push(format!("Package not found in workspace: '{}'", requested));
            }
            1 => {
                resolved_names.push(dir_matches[0].name.clone());
            }
            _ => {
                errors.push(format!(
                    "Ambiguous package '{}': use the manifest name to disambiguate.",
                    requested
                ));
            }
        }
    }

    if !errors.is_empty() {
        let mut error_msg = errors.join("\n\n");

        error_msg.push_str("\n\nAvailable packages (manifest name / directory):");
        for pkg in &all_packages {
            error_msg.push_str(&format!("\n  - {} ({})", pkg.name, pkg.dir.display()));
        }

        return Err(error_msg.into());
    }

    // Filter to only resolved packages.
    let package_info: Vec<Package> = all_packages
        .into_iter()
        .filter(|pkg| resolved_names.iter().any(|r| r == &pkg.name))
        .collect();

    Ok(package_info)
}

/// Get the workspace root directory from metadata.
///
/// This is the directory containing the top-level `Cargo.toml`. It is the
/// authoritative location for workspace-level files, regardless of where
/// the shell's current directory happens to be.
///
/// For single-package repositories with no explicit `[workspace]` table, Cargo
/// creates an implicit workspace and `workspace_root` resolves to the package
/// directory itself.
pub fn get_workspace_root(sh: &Shell) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let metadata = rbmt_cmd!(sh, "cargo metadata --no-deps --format-version 1").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;
    let root = json["workspace_root"].as_str().ok_or("Missing workspace_root in cargo metadata")?;
    Ok(PathBuf::from(root))
}

/// Get the cargo target directory from metadata.
///
/// This respects `CARGO_TARGET_DIR`, .cargo/config.toml, and other cargo
/// target directory configuration.
pub fn get_target_dir(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let metadata = rbmt_cmd!(sh, "cargo metadata --no-deps --format-version 1").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;
    let target_dir =
        json["target_directory"].as_str().ok_or("Missing target_directory in cargo metadata")?;
    Ok(target_dir.to_string())
}

/// Discover the features defined for a package.
///
/// Returns all keys from the package's `[features]` table, excluding `"default"` since
/// it is not a feature that can be passed directly to `--features`. Optional dependencies
/// are included automatically.
pub fn discover_features(
    sh: &Shell,
    package: &Package,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let metadata = rbmt_cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    let packages =
        json["packages"].as_array().ok_or("Missing 'packages' field in cargo metadata")?;

    // Match by manifest path so this works regardless of the shell's cwd.
    let manifest_path = package.dir.join("Cargo.toml");
    let pkg = packages
        .iter()
        .find(|p| p["manifest_path"].as_str().is_some_and(|path| Path::new(path) == manifest_path))
        .ok_or_else(|| format!("Package not found in cargo metadata: {}", package.dir.display()))?;

    let mut features: Vec<String> = pkg["features"]
        .as_object()
        .map(|f| f.keys().filter(|k| *k != "default").cloned().collect())
        .unwrap_or_default();

    features.sort();
    Ok(features)
}

/// Get the current git commit ID.
///
/// Returns `None` if the working directory is not inside a git repository or
/// if git is not available.
pub fn git_commit_id(sh: &Shell) -> Option<String> {
    sh.cmd("git").args(["rev-parse", "HEAD"]).quiet().read().ok().map(|s| s.trim().to_owned())
}

/// A minimal representation of a package manifest (`Cargo.toml`).
///
/// Only fields not available via `cargo metadata` are included here. Prefer
/// `cargo metadata` for all other package information since it is the stable,
/// supported interface for querying package data.
pub struct Manifest {
    /// The `exclude` field from `[package]`, listing paths excluded from publishing.
    pub exclude: Vec<String>,
}

impl Manifest {
    /// Read and parse the `Cargo.toml` in the given package directory.
    pub fn read(package_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize)]
        struct CargoToml {
            package: CargoPackage,
        }

        #[derive(serde::Deserialize)]
        struct CargoPackage {
            #[serde(default)]
            exclude: Vec<String>,
        }

        let contents = fs::read_to_string(package_dir.join("Cargo.toml"))?;
        let cargo_toml: CargoToml = toml::from_str(&contents)?;

        Ok(Self { exclude: cargo_toml.package.exclude })
    }
}

/// A minimal representation of a `Cargo.toml` for deserializing `[package.metadata.rbmt]`.
///
/// `T` is the type of the `[package.metadata.rbmt]` table. Each subcommand module defines its
/// own `T` containing only the fields it needs.
///
/// ```ignore
/// #[derive(serde::Deserialize, Default)]
/// struct RbmtTable {
///     #[serde(default)]
///     lint: LintConfig,
/// }
///
/// let path = package_dir.join("Cargo.toml");
/// let contents = fs::read_to_string(&path)?;
/// let config = toml::from_str::<PackageManifest<RbmtTable>>(&contents)?
///     .package.metadata.rbmt.lint;
/// ```
#[derive(serde::Deserialize, Default)]
pub(crate) struct PackageManifest<T: Default> {
    #[serde(default)]
    pub(crate) package: PackageTable<T>,
}

/// A minimal representation of a `Cargo.toml` for deserializing both
/// `[workspace.metadata.rbmt]` and `[package.metadata.rbmt]` simultaneously.
///
/// Used when a module needs to prefer the workspace namespace and fall back to
/// the package namespace, as with `[workspace.metadata.rbmt.tools]`.
///
/// ```ignore
/// let contents = fs::read_to_string(&path)?;
/// let toml = toml::from_str::<WorkspaceManifest<RbmtTable>>(&contents)?;
/// let config = toml.workspace.metadata.rbmt.tools
///     .or(toml.package.metadata.rbmt.tools);
/// ```
#[derive(serde::Deserialize, Default)]
pub(crate) struct WorkspaceManifest<T: Default> {
    #[serde(default)]
    pub(crate) workspace: WorkspaceTable<T>,
    #[serde(default)]
    pub(crate) package: PackageTable<T>,
}

/// The `[workspace]` table of a `Cargo.toml`, generic over the `[workspace.metadata.rbmt]` type.
#[derive(serde::Deserialize, Default)]
pub(crate) struct WorkspaceTable<T: Default> {
    #[serde(default)]
    pub(crate) metadata: MetadataTable<T>,
}

/// The `[package]` table of a `Cargo.toml`, generic over the `[package.metadata.rbmt]` type.
#[derive(serde::Deserialize, Default)]
pub(crate) struct PackageTable<T: Default> {
    #[serde(default)]
    pub(crate) metadata: MetadataTable<T>,
}

/// The `[*.metadata]` table of a `Cargo.toml`, generic over the `[*.metadata.rbmt]` type.
#[derive(serde::Deserialize, Default)]
pub(crate) struct MetadataTable<T: Default> {
    #[serde(default)]
    pub(crate) rbmt: T,
}
