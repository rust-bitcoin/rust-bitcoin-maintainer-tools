use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use xshell::Shell;

use crate::{environment, quiet_cmd, toolchain};

/// RAII guard for temporarily switching git refs.
struct GitSwitchGuard<'a> {
    sh: &'a Shell,
}

impl<'a> GitSwitchGuard<'a> {
    /// Create a new guard and switch to the specified ref.
    fn new(sh: &'a Shell, git_ref: &str) -> Result<Self, Box<dyn std::error::Error>> {
        environment::quiet_println(&format!("Switching to ref: {}", git_ref));
        quiet_cmd!(sh, "git switch --detach {git_ref}").run()?;
        Ok(Self { sh })
    }
}

impl Drop for GitSwitchGuard<'_> {
    fn drop(&mut self) {
        environment::quiet_println("Returning to previous ref...");
        // Use expect here because if this fails, we're already in a bad state
        // and there's not much we can do about it in Drop.
        quiet_cmd!(self.sh, "git switch --detach -")
            .run()
            .expect("Failed to switch back to previous git ref");
    }
}

/// Directory where API files are stored, relative to each package directory.
const API_DIR: &str = "api";

/// RUSTDOCFLAGS to allow broken intra-doc links during API checking.
///
/// When generating API documentation with limited features (e.g., --no-default-features),
/// some doc links may reference items that don't exist without those features.
/// This flag suppresses those warnings so we can focus on actual API changes.
const RUSTDOCFLAGS_ALLOW_BROKEN_LINKS: &str = "-A rustdoc::broken_intra_doc_links";

/// A collection of public APIs for a single package across different feature configurations.
type PackageApis = HashMap<FeatureConfig, public_api::PublicApi>;

/// API configuration loaded from rbmt.toml.
#[derive(Debug, serde::Deserialize, Default)]
#[serde(default)]
struct Config {
    api: ApiConfig,
}

/// API-specific configuration.
#[derive(Debug, serde::Deserialize)]
#[serde(default)]
struct ApiConfig {
    /// Whether to run API checks for this package. Defaults to `true`.
    enabled: bool,
    /// Feature combinations to test (in addition to no-features and all-features).
    features: Vec<Vec<String>>,
    /// Default git ref to use as baseline for semver comparison.
    /// If not set, only feature additivity and git status checks are performed.
    baseline: Option<String>,
}

impl Default for ApiConfig {
    fn default() -> Self { Self { enabled: true, features: Vec::new(), baseline: None } }
}

impl ApiConfig {
    /// Load API configuration from a package directory.
    fn load(package_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = package_dir.join(environment::CONFIG_FILE_PATH);

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.api)
    }
}

/// Feature configurations to test for API generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FeatureConfig {
    /// No features enabled (--no-default-features).
    None,
    /// Specific features enabled (--no-default-features --features=X,Y).
    Some(Vec<String>),
    /// All features enabled (--all-features).
    All,
}

impl FeatureConfig {
    /// Get the filename for this configuration.
    fn filename(&self) -> String { format!("{}.txt", self.name()) }

    /// Get the display name for this configuration.
    fn name(&self) -> String {
        match self {
            Self::None => "no-features".to_string(),
            Self::Some(features) => format!("{}-only", features.join("-")),
            Self::All => "all-features".to_string(),
        }
    }

    /// Get the cargo arguments for this configuration.
    fn cargo_args(&self) -> Vec<String> {
        match self {
            Self::None => vec!["--no-default-features".to_string()],
            Self::Some(features) => {
                let mut args = vec!["--no-default-features".to_string()];
                args.push(format!("--features={}", features.join(",")));
                args
            }
            Self::All => vec!["--all-features".to_string()],
        }
    }
}

/// Run the API check task.
///
/// This command checks for changes to the public API of workspace packages by generating
/// API files using the `public-api` library and comparing them with committed versions in each
/// package's own `api/` directory.
///
/// Always checks that features are additive and API files match git state.
/// When a baseline ref is given or configured, also performs semver
/// compatibility checking by comparing the current API against the baseline.
///
/// # Arguments
///
/// * `packages` - Optional list of packages to check. If empty, checks all packages in the workspace.
/// * `baseline` - Git ref for optional semver comparison.
pub fn run(
    sh: &Shell,
    packages: &[environment::Package],
    baseline: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    environment::quiet_println("Running API check...");
    toolchain::prepare_toolchain(sh, toolchain::Toolchain::Nightly)?;

    check_apis(sh, packages, baseline)?;

    Ok(())
}

/// Get the public APIs for a single package across all feature configurations.
fn get_package_apis(
    sh: &Shell,
    package_name: &str,
    package_dir: &PathBuf,
) -> Result<PackageApis, Box<dyn std::error::Error>> {
    let workspace_root = sh.current_dir();
    let mut apis = HashMap::new();

    let mut feature_configs = vec![FeatureConfig::None, FeatureConfig::All];
    let api_config = ApiConfig::load(Path::new(package_dir))?;
    for features in &api_config.features {
        if !features.is_empty() {
            feature_configs.push(FeatureConfig::Some(features.clone()));
        }
    }

    for config in feature_configs {
        // Change to package directory to run rustdoc.
        // This is necessary because cargo doesn't allow feature flags with -p option.
        sh.change_dir(package_dir);

        // Generate rustdoc JSON.
        // Use --lib to avoid ambiguity errors in packages with multiple targets (e.g. lib + bin).
        let mut cmd = quiet_cmd!(sh, "cargo rustdoc --lib");
        for arg in config.cargo_args() {
            cmd = cmd.arg(arg);
        }
        cmd = cmd.args(&["--", "-Z", "unstable-options", "--output-format", "json"]);
        cmd.env("RUSTDOCFLAGS", RUSTDOCFLAGS_ALLOW_BROKEN_LINKS).run()?;

        // Change back to workspace root and parse JSON.
        sh.change_dir(&workspace_root);
        let target_dir = environment::get_target_dir(sh)?;
        let json_path = Path::new(&target_dir)
            .join("doc")
            // Rustdoc replaces hyphens with underscores in the filename.
            .join(package_name.replace('-', "_"))
            .with_extension("json");

        let public_api = public_api::Builder::from_rustdoc_json(&json_path).build()?;
        apis.insert(config, public_api);
    }

    Ok(apis)
}

/// Check API files for all packages.
///
/// For each package, generates public API files for different feature configurations,
/// validates that features are additive, and checks for git changes.
fn check_apis(
    sh: &Shell,
    package_info: &[crate::environment::Package],
    baseline: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut api_dirs: Vec<PathBuf> = Vec::new();

    for (package_name, package_dir) in package_info {
        let api_config = ApiConfig::load(package_dir)?;

        if !api_config.enabled {
            continue;
        }

        check_api_excluded(package_dir, package_name)?;
        let mut apis = get_package_apis(sh, package_name, package_dir)?;

        // Write API files into the package's own api/ directory.
        let package_api_dir = package_dir.join(API_DIR);
        fs::create_dir_all(&package_api_dir)?;
        api_dirs.push(package_api_dir.clone());

        for (config, public_api) in &apis {
            let output_file = package_api_dir.join(config.filename());
            fs::write(&output_file, public_api.to_string())?;
        }

        // Check that features are additive (all-features contains everything from no-features).
        let no_features =
            apis.remove(&FeatureConfig::None).ok_or("No-features config not found")?;
        let all_features =
            apis.remove(&FeatureConfig::All).ok_or("All-features config not found")?;

        let diff = public_api::diff::PublicApiDiff::between(no_features, all_features);

        if !diff.removed.is_empty() || !diff.changed.is_empty() {
            eprintln!("Non-additive features detected in {}:", package_name);

            if !diff.removed.is_empty() {
                eprintln!("  Items removed when enabling features:");
                for item in &diff.removed {
                    eprintln!("    - {}", item);
                }
            }

            if !diff.changed.is_empty() {
                eprintln!("  Items changed when enabling features:");
                for item in &diff.changed {
                    eprintln!("    - old: {}", item.old);
                    eprintln!("      new: {}", item.new);
                }
            }

            return Err("Non-additive features detected".into());
        }

        // CLI flag takes priority over config.
        if let Some(baseline) = baseline.or(api_config.baseline.as_deref()) {
            check_semver(sh, package_name, package_dir, baseline)?;
        }
    }

    for api_dir in &api_dirs {
        let status_output = quiet_cmd!(sh, "git status --porcelain {api_dir}").read()?;
        if !status_output.trim().is_empty() {
            // Show the diff for context.
            quiet_cmd!(sh, "git diff --color=always {api_dir}").run()?;

            eprintln!();
            return Err(format!(
                "You have introduced changes to the public API, commit the changes to {} currently in your working directory",
                api_dir.display()
            ).into());
        }
    }

    Ok(())
}

/// Check that the package's manifest excludes the `api/` directory from publishing.
fn check_api_excluded(
    package_dir: &Path,
    package_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = environment::Manifest::read(package_dir)?;

    if !manifest.exclude.iter().any(|e| e.starts_with("api")) {
        return Err(format!(
            "Package '{}' has an api/ directory but does not exclude it from publishing. \
             Add \"api\" to the `exclude` list in {}/Cargo.toml.",
            package_name,
            package_dir.display(),
        )
        .into());
    }

    Ok(())
}

/// Run semver compatibility check against a baseline ref.
///
/// Compares the current all-features API against the baseline to report removed, changed, and
/// added items. This check is informational and never fails, it just prints a summary of API
/// differences to help maintainers assess semver impact.
///
/// Only checks the all-features configuration. This means items that were moved behind a
/// feature gate (from unconditional to `#[cfg(feature = "...")]`) will not be detected as
/// removed, since they still appear in the all-features API. Detecting such changes would
/// require checking every feature combination from the baseline.
fn check_semver(
    sh: &Shell,
    package_name: &str,
    package_dir: &PathBuf,
    baseline: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    environment::quiet_println(&format!("Running semver check against baseline: {}", baseline));

    let mut current_apis = get_package_apis(sh, package_name, package_dir)?;
    let mut baseline_apis = {
        let _guard = GitSwitchGuard::new(sh, baseline)?;
        get_package_apis(sh, package_name, package_dir)?
    };

    let baseline_api = baseline_apis
        .remove(&FeatureConfig::All)
        .ok_or("All-features config not found in baseline")?;
    let current_api = current_apis
        .remove(&FeatureConfig::All)
        .ok_or("All-features config not found in current")?;

    let diff = public_api::diff::PublicApiDiff::between(baseline_api, current_api);

    eprintln!("Semver check vs {}:", baseline);

    if !diff.removed.is_empty() {
        eprintln!("  Removed (possibly breaking):");
        for item in &diff.removed {
            eprintln!("    - {}", item);
        }
    }

    if !diff.changed.is_empty() {
        eprintln!("  Changed (possibly breaking):");
        for item in &diff.changed {
            eprintln!("    old: {}", item.old);
            eprintln!("    new: {}", item.new);
        }
    }

    if !diff.added.is_empty() {
        eprintln!("  Added:");
        for item in &diff.added {
            eprintln!("    + {}", item);
        }
    }

    eprintln!(
        "  Summary: {} removed, {} changed, {} added",
        diff.removed.len(),
        diff.changed.len(),
        diff.added.len()
    );

    Ok(())
}
