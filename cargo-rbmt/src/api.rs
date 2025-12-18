use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use xshell::Shell;

use crate::{environment, quiet_cmd, toolchain};

/// Directory where API files are stored, relative to workspace root.
const API_DIR: &str = "api";

/// RUSTDOCFLAGS to allow broken intra-doc links during API checking.
///
/// When generating API documentation with limited features (e.g., --no-default-features),
/// some doc links may reference items that don't exist without those features.
/// This flag suppresses those warnings so we can focus on actual API changes.
const RUSTDOCFLAGS_ALLOW_BROKEN_LINKS: &str = "-A rustdoc::broken_intra_doc_links";

/// A collection of public APIs for a single package across different feature configurations.
type PackageApis = HashMap<FeatureConfig, public_api::PublicApi>;

/// Feature configurations to test for API generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FeatureConfig {
    /// No features enabled.
    None,
    /// Only alloc feature enabled.
    Alloc,
    /// All features enabled.
    All,
}

impl FeatureConfig {
    /// Get the filename for this configuration.
    fn filename(self) -> &'static str {
        match self {
            Self::None => "no-features.txt",
            Self::Alloc => "alloc-only.txt",
            Self::All => "all-features.txt",
        }
    }

    /// Get a display name for this configuration.
    fn display_name(self) -> &'static str {
        match self {
            Self::None => "no-features",
            Self::Alloc => "alloc-only",
            Self::All => "all-features",
        }
    }

    /// Get the cargo arguments for this configuration.
    fn cargo_args(self) -> &'static [&'static str] {
        match self {
            Self::None => &["--no-default-features"],
            Self::Alloc => &["--no-default-features", "--features=alloc"],
            Self::All => &["--all-features"],
        }
    }
}

/// Run the API check task.
///
/// This command checks for changes to the public API of workspace packages by generating
/// API files using the `public-api` library and comparing them with committed versions in the
/// `api/` directory.
///
/// When generating new API files (no baseline), also checks that features are additive.
/// When a baseline ref is provided, performs semver compatibility checking by comparing the
/// current API against the baseline.
///
/// # Arguments
///
/// * `packages` - Optional list of packages to check. If empty, checks all packages in the workspace.
/// * `baseline` - Optional git ref to use as baseline for semver comparison.
pub fn run(
    sh: &Shell,
    packages: &[String],
    baseline: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    environment::quiet_println("Running API check...");
    toolchain::check_toolchain(sh, toolchain::Toolchain::Nightly)?;

    let package_info = environment::get_packages(sh, packages)?;

    if let Some(baseline_ref) = baseline {
        check_semver(sh, &package_info, baseline_ref)?;
    } else {
        check_apis(sh, &package_info)?;
    }

    environment::quiet_println("API check completed successfully");
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

    for config in [FeatureConfig::None, FeatureConfig::Alloc, FeatureConfig::All] {
        // Change to package directory to run rustdoc.
        // This is necessary because cargo doesn't allow feature flags with -p option.
        sh.change_dir(package_dir);

        // Generate rustdoc JSON.
        let mut cmd = quiet_cmd!(sh, "cargo rustdoc");
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
    package_info: &[(String, PathBuf)],
) -> Result<(), Box<dyn std::error::Error>> {
    for (package_name, package_dir) in package_info {
        let mut apis = get_package_apis(sh, package_name, package_dir)?;

        // Write API files.
        let workspace_root = sh.current_dir();
        let package_api_dir = workspace_root.join(API_DIR).join(package_name);
        fs::create_dir_all(&package_api_dir)?;

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
    }

    // Check for changes to the API files using git.
    environment::quiet_println("Checking for API changes...");

    let status_output = quiet_cmd!(sh, "git status --porcelain {API_DIR}").read()?;
    if !status_output.trim().is_empty() {
        // Show the diff for context.
        quiet_cmd!(sh, "git diff --color=always {API_DIR}").run()?;

        eprintln!();
        return Err(
            "You have introduced changes to the public API, commit the changes to api/ currently in your working directory"
                .into(),
        );
    }

    environment::quiet_println("No changes to the current public API");
    Ok(())
}

/// Run semver compatibility check against a baseline ref.
///
/// Compares current API vs. baseline API for breaking changes (e.g. removed/changed items).
fn check_semver(
    sh: &Shell,
    package_info: &[(String, PathBuf)],
    baseline_ref: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    environment::quiet_println(&format!("Running semver check against baseline: {}", baseline_ref));

    // Store current branch/commit to restore later.
    let current_ref = quiet_cmd!(sh, "git rev-parse --abbrev-ref HEAD").read()?;
    let current_ref = current_ref.trim();

    // Generate APIs for current commit.
    environment::quiet_println("Generating APIs for current commit...");
    let mut current_apis = HashMap::new();
    for (package_name, package_dir) in package_info {
        let package_apis = get_package_apis(sh, package_name, package_dir)?;
        current_apis.insert(package_name.clone(), package_apis);
    }

    // Switch to baseline.
    environment::quiet_println(&format!("Switching to baseline: {}", baseline_ref));
    quiet_cmd!(sh, "git switch --detach {baseline_ref}").run()?;

    // Generate APIs for baseline.
    environment::quiet_println("Generating APIs for baseline...");
    let mut baseline_apis = HashMap::new();
    for (package_name, package_dir) in package_info {
        let package_apis = get_package_apis(sh, package_name, package_dir)?;
        baseline_apis.insert(package_name.clone(), package_apis);
    }

    // Switch back to original ref.
    environment::quiet_println(&format!("Returning to: {}", current_ref));
    quiet_cmd!(sh, "git switch {current_ref}").run()?;

    // Check for breaking changes in each package.
    for package_name in package_info.iter().map(|(name, _)| name) {
        let Some(mut baseline) = baseline_apis.remove(package_name) else {
            environment::quiet_println(&format!(
                "Warning: Package '{}' not found in baseline - skipping comparison",
                package_name
            ));
            continue;
        };

        let Some(mut current) = current_apis.remove(package_name) else {
            environment::quiet_println(&format!(
                "Warning: Package '{}' exists in baseline but not in current - possible removal",
                package_name
            ));
            continue;
        };

        for config in [FeatureConfig::None, FeatureConfig::Alloc, FeatureConfig::All] {
            let baseline_api = baseline.remove(&config).ok_or("Config not found in baseline")?;
            let current_api = current.remove(&config).ok_or("Config not found in current")?;

            let diff = public_api::diff::PublicApiDiff::between(baseline_api, current_api);

            if !diff.removed.is_empty() || !diff.changed.is_empty() {
                eprintln!("API changes detected in {} ({})", package_name, config.display_name());
                return Err("Semver compatibility check failed: breaking changes detected".into());
            }
        }
    }

    Ok(())
}
