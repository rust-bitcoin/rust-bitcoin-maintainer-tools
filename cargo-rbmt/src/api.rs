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

/// Feature configurations to test for API generation.
#[derive(Debug, Clone, Copy)]
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
    fn filename(&self) -> &'static str {
        match self {
            Self::None => "no-features.txt",
            Self::Alloc => "alloc-only.txt",
            Self::All => "all-features.txt",
        }
    }

    /// Get the cargo arguments for this configuration.
    fn cargo_args(&self) -> &'static [&'static str] {
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
/// # Arguments
///
/// * `packages` - Optional list of packages to check. If empty, checks all packages in the workspace.
pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    environment::quiet_println("Running API check...");
    toolchain::check_toolchain(sh, toolchain::Toolchain::Nightly)?;

    let package_info = environment::get_packages(sh, packages)?;

    for (package_name, package_dir) in &package_info {
        generate_api_files(sh, package_name, package_dir)?;
    }

    check_for_changes(sh)?;

    environment::quiet_println("API check completed successfully");
    Ok(())
}

/// Generate API files for the specified package.
///
/// Creates three files in the `api/<package>/` directory.
///
/// * `no-features.txt` - API with no features enabled.
/// * `alloc-only.txt` - API with only alloc feature enabled.
/// * `all-features.txt` - API with all features enabled.
fn generate_api_files(
    sh: &Shell,
    package_name: &str,
    package_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    environment::quiet_println(&format!("Generating API files for {}", package_name));

    let workspace_root = sh.current_dir();
    let package_api_dir = workspace_root.join(API_DIR).join(package_name);
    fs::create_dir_all(&package_api_dir)?;

    // Generate API for each feature configuration.
    for config in [
        FeatureConfig::None,
        FeatureConfig::Alloc,
        FeatureConfig::All,
    ] {
        let output_file = package_api_dir.join(config.filename());

        // Change to the package directory to run rustdoc.
        // This is necessary because cargo doesn't allow feature flags with -p option.
        sh.change_dir(package_dir);

        // Generate rustdoc JSON.
        let mut cmd = quiet_cmd!(sh, "cargo rustdoc");
        for arg in config.cargo_args() {
            cmd = cmd.arg(arg);
        }
        cmd = cmd.args(&["--", "-Z", "unstable-options", "--output-format", "json"]);
        cmd.env("RUSTDOCFLAGS", RUSTDOCFLAGS_ALLOW_BROKEN_LINKS)
            .run()?;

        // Construct the path to the generated JSON file.
        sh.change_dir(&workspace_root);
        let target_dir = environment::get_target_dir(sh)?;
        let json_path = Path::new(&target_dir)
            .join("doc")
            // Rustdoc replaces hyphens with underscores in the filename.
            .join(package_name.replace('-', "_"))
            .with_extension("json");

        // Parse the rustdoc JSON and extract public API.
        let public_api = public_api::Builder::from_rustdoc_json(&json_path).build()?;
        fs::write(&output_file, public_api.to_string())?;
    }

    Ok(())
}

/// Check for changes to the API files using git.
fn check_for_changes(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    environment::quiet_println("Checking for API changes...");

    // Check if there are any changes to the API directory.
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
