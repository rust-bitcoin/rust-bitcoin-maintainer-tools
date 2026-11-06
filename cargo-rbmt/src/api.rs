// SPDX-License-Identifier: MIT AND Apache-2.0

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use public_api::rustdoc_types::{Crate, Id, ItemEnum};
use xshell::Shell;

use crate::environment::{
    get_target_dir, get_workspace_packages, get_workspace_root, CmdExt, Manifest, Package,
    PackageManifest, ProgressGuard,
};
use crate::lock::LockFile;
use crate::{git, toolchain};

/// Directory where API files are stored, relative to each package directory.
const API_DIR: &str = "api";

/// RUSTDOCFLAGS to allow broken intra-doc links during API checking.
///
/// When generating API documentation with limited features (e.g., --no-default-features),
/// some doc links may reference items that don't exist without those features.
/// This flag suppresses those warnings so we can focus on actual API changes.
const RUSTDOCFLAGS_ALLOW_BROKEN_LINKS: &str = "-A rustdoc::broken_intra_doc_links";

/// A public API for a single feature configuration, plus crate attribution data
/// for detecting re-exports of private dependencies.
struct PackageApiEntry {
    api: public_api::PublicApi,
    origins: ItemOrigins,
}

/// Public APIs for a package across feature configurations.
type PackageApis = HashMap<FeatureConfig, PackageApiEntry>;

/// How a public item came from an external crate.
enum Origin {
    /// Item re-export, `pub use`.
    Use(String),
    /// Crate re-export, `pub extern crate`.
    ExternCrate(String),
}

/// Maps a public item's rustdoc `Id` to the external crate that defines it.
struct ItemOrigins(HashMap<u32, Origin>);

impl ItemOrigins {
    /// Traverses json to attribute re-exported items to their origin crate.
    fn from_rustdoc_json(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let raw = fs::read_to_string(path)?;
        let crate_: Crate = serde_json::from_str(&raw)?;

        let mut origins = HashMap::new();

        for (id, item) in &crate_.index {
            match &item.inner {
                ItemEnum::Use(use_) => {
                    let Some(target) = use_.id else { continue };
                    let Some(summary) = crate_.paths.get(&target) else { continue };
                    let Some(ext) = crate_.external_crates.get(&summary.crate_id) else { continue };
                    origins.insert(id.0, Origin::Use(ext.name.clone()));
                }
                ItemEnum::ExternCrate { name, .. } => {
                    origins.insert(id.0, Origin::ExternCrate(name.clone()));
                }
                _ => {}
            }
        }

        Ok(Self(origins))
    }

    /// The external crate that defines this item via `pub use`, if not the local crate.
    fn use_origin(&self, item: &public_api::PublicItem) -> Option<&str> {
        match self.0.get(&item.id().0)? {
            Origin::Use(name) => Some(name.as_str()),
            Origin::ExternCrate(_) => None,
        }
    }

    /// The external crate that defines this item via `pub extern crate`, if not the local crate.
    fn extern_crate_origin(&self, item: &public_api::PublicItem) -> Option<&str> {
        match self.0.get(&item.id().0)? {
            Origin::ExternCrate(name) => Some(name.as_str()),
            Origin::Use(_) => None,
        }
    }
}

/// API-specific configuration, read from `[package.metadata.rbmt.api]` in `Cargo.toml`.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ApiConfig {
    /// Whether to run API checks for this package. Defaults to `false`.
    enabled: bool,
    /// Whether to generate API snapshot files. Defaults to `false`.
    snapshot: bool,
    /// Feature combinations to test (in addition to no-features and all-features).
    features: Vec<Vec<String>>,
    /// List of private/internal dependencies that should not appear in public API.
    private: Vec<String>,
}

impl ApiConfig {
    /// Load API configuration from `[package.metadata.rbmt.api]` in the package's `Cargo.toml`.
    fn load(package_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize, Default)]
        struct RbmtTable {
            #[serde(default)]
            api: ApiConfig,
        }

        let path = package_dir.join("Cargo.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        Ok(toml::from_str::<PackageManifest<RbmtTable>>(&contents)?.package.metadata.rbmt.api)
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

/// Build a map of item ID to extra context if it has any.
struct ItemContext {
    // Parent ID to the context it provides.
    map: HashMap<Id, String>,
}

impl ItemContext {
    /// Prime `ItemContext` based on a full API.
    fn new(api: &public_api::PublicApi) -> Self {
        let parent_items: Vec<_> = api.items().collect();
        let id_to_item: HashMap<_, _> = parent_items.iter().map(|i| (i.id(), i)).collect();

        let map = api.items()
            .filter_map(|item| {
                if let Some(parent_id) = item.parent_id() {
                    if let Some(parent_item) = id_to_item.get(&parent_id) {
                        // If parent is a trait impl (contains "for" keyword), capture the context.
                        if parent_item.tokens().any(
                            |token| matches!(token, public_api::tokens::Token::Keyword(kw) if kw == "for"),
                        ) {
                            let context = format!("[impl: {}]", parent_item);
                            return Some((parent_id, context));
                        }
                    }
                }
                // Item has no useful context.
                None
            })
            .collect();

        Self { map }
    }

    /// Format a `PublicItem` for display, appending context if it has any.
    fn format(&self, item: &public_api::PublicItem) -> String {
        match item.parent_id().and_then(|pid| self.map.get(&pid)) {
            Some(ctx) => format!("{item} {ctx}"),
            None => item.to_string(),
        }
    }
}

/// A feature set configuration's diff with formatting contexts.
struct FeatureDiff {
    feature_config: FeatureConfig,
    diff: public_api::diff::PublicApiDiff,
    baseline_context: ItemContext,
    current_context: ItemContext,
}

/// Represents all diffs for a single package across different feature configurations.
struct PackageDiff {
    package_name: String,
    feature_diffs: Vec<FeatureDiff>,
}

/// Error type for when API diffs are detected.
pub(crate) struct ApiDiffError {
    package_diffs: Vec<PackageDiff>,
}

impl std::fmt::Display for ApiDiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "API diffs found in {} package(s)", self.package_diffs.len())?;
        for package in &self.package_diffs {
            for feature in &package.feature_diffs {
                writeln!(
                    f,
                    "--- {} API Diff ({})",
                    package.package_name,
                    feature.feature_config.name()
                )?;
                for item in &feature.diff.removed {
                    writeln!(f, "- {}", feature.baseline_context.format(item))?;
                }
                for item in &feature.diff.changed {
                    writeln!(
                        f,
                        "~ {} > {}",
                        feature.baseline_context.format(&item.old),
                        feature.current_context.format(&item.new)
                    )?;
                }
                for item in &feature.diff.added {
                    writeln!(f, "+ {}", feature.current_context.format(item))?;
                }
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for ApiDiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "ApiDiffError") }
}

impl std::error::Error for ApiDiffError {}

/// Outcome of the API check.
///
/// Distinguished from hard errors (I/O, build failures) so callers can map
/// each outcome to a different exit code: 2 for diffs, 3 for leaks, 1 for
/// hard failures.
pub enum ApiOutcome {
    /// No API changes or leaks detected; all checks passed.
    Success,
    /// API diffs detected against the baseline.
    DiffDetected(ApiDiffError),
    /// Private dependency exposed in public API.
    PrivateDepLeak(String),
}

/// Run the API task to check or generate API snapshots for packages.
///
/// # Arguments
///
/// * `lockfile` - Optional lockfile variant to use.
/// * `packages` - Optional list of packages to check. If empty, checks all packages in the workspace.
/// * `baseline` - Git ref for baseline diff comparison. When not provided, outputs APIs to stdout.
/// * `snapshot` - Whether to generate API snapshot files to disk.
///
/// # Returns
///
/// * `Ok(ApiOutcome::Success)` - No API diffs or leaks detected.
/// * `Ok(ApiOutcome::DiffDetected(ApiDiffError))` - API diffs found vs baseline.
/// * `Ok(ApiOutcome::PrivateDepLeak(String))` - Private dependency exposed in public API.
/// * `Err(_)` - Hard failure (build error, I/O error, etc.).
pub fn run(
    sh: &Shell,
    lockfile: Option<LockFile>,
    packages: &[String],
    baseline: Option<&str>,
    snapshot: bool,
) -> Result<ApiOutcome, Box<dyn std::error::Error>> {
    let lockfile = if let Some(lockfile) = lockfile { lockfile } else { LockFile::max(sh)? };
    let packages = get_workspace_packages(sh, packages)?;
    let _lockfile_guard = lockfile.activate(sh)?;
    let mut progress = ProgressGuard::new();
    rbmt_eprintln!("Running API check...");
    toolchain::prepare_toolchain(sh, toolchain::Toolchain::Nightly)?;

    let baseline = match baseline {
        Some(ref_name) => {
            let resolved = git::resolve_baseline(sh, ref_name)?;
            rbmt_eprintln!("Comparing against baseline: {}", resolved);
            Some(resolved)
        }
        None => None,
    };

    let mut package_diffs = Vec::new();
    let mut package_apis: Vec<(String, PackageApis)> = Vec::new();

    for package in packages {
        let api_config = ApiConfig::load(&package.dir)?;

        if !api_config.enabled {
            continue;
        }

        rbmt_eprintln!("API check enabled in {}", package.name);

        let current = get_package_apis(sh, &package.name, &package.dir)?;
        if let Some(msg) = check_private_deps(
            &package.name,
            &current,
            &api_config.private,
            &package.dep_aliases,
            &package.deps,
        )? {
            return Ok(ApiOutcome::PrivateDepLeak(msg));
        }

        if snapshot || api_config.snapshot {
            write_api_files(&package, &current)?;
        }
        if let Some(baseline) = &baseline {
            if let Some(package_diff) = check_baseline(sh, &package, baseline, current)? {
                package_diffs.push(package_diff);
            }
        } else {
            package_apis.push((package.name, current));
        }
    }

    if !package_diffs.is_empty() {
        return Ok(ApiOutcome::DiffDetected(ApiDiffError { package_diffs }));
    }

    // Output all APIs by default.
    if baseline.is_none() {
        progress.disable();
        for (package_name, feature_apis) in package_apis {
            for (feature_config, entry) in feature_apis {
                let api = &entry.api;
                println!("--- {} API ({})", package_name, feature_config.name());
                let context = ItemContext::new(api);
                for item in api.items() {
                    println!("{}", context.format(item));
                }
            }
        }
    }

    Ok(ApiOutcome::Success)
}

/// Get the public APIs for a single package across all feature configurations.
fn get_package_apis(
    sh: &Shell,
    package_name: &str,
    package_dir: &PathBuf,
) -> Result<PackageApis, Box<dyn std::error::Error>> {
    let workspace_root = get_workspace_root(sh)?;
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
        let mut cmd = rbmt_cmd!(sh, "cargo rustdoc --lib");
        for arg in config.cargo_args() {
            cmd = cmd.arg(arg);
        }
        cmd = cmd.args(["--", "-Z", "unstable-options", "--output-format", "json"]);
        cmd.env("RUSTDOCFLAGS", RUSTDOCFLAGS_ALLOW_BROKEN_LINKS).run_with_capture()?;

        // Change back to workspace root and parse JSON.
        sh.change_dir(&workspace_root);
        let json_path = get_target_dir(sh)?
            .join("doc")
            // Rustdoc replaces hyphens with underscores in the filename.
            .join(CrateName::from_package_name(package_name).to_string())
            .with_extension("json");

        let public_api = public_api::Builder::from_rustdoc_json(&json_path).build()?;
        let origins = ItemOrigins::from_rustdoc_json(&json_path)?;
        apis.insert(config, PackageApiEntry { api: public_api, origins });
    }

    Ok(apis)
}

/// Write API files to disk.
fn write_api_files(
    package: &Package,
    apis: &PackageApis,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check that the package's manifest excludes the `api/` directory from publishing.
    let manifest = Manifest::read(&package.dir)?;
    if !manifest.exclude.iter().any(|e| e.starts_with("api")) {
        return Err(format!(
            "Package '{}' has an api/ directory but does not exclude it from publishing. \
             Add \"api\" to the `exclude` list in {}/Cargo.toml.",
            package.name,
            package.dir.display(),
        )
        .into());
    }

    let package_api_dir = package.dir.join(API_DIR);
    fs::create_dir_all(&package_api_dir)?;
    for (config, entry) in apis {
        let public_api = &entry.api;
        let output_file = package_api_dir.join(config.filename());
        let context = ItemContext::new(public_api);
        let api_display =
            public_api.items().map(|item| context.format(item)).collect::<Vec<_>>().join("\n");
        fs::write(&output_file, api_display)?;
    }
    Ok(())
}

/// Compare current APIs against a baseline ref, return diffs for any feature sets with API changes.
fn check_baseline(
    sh: &Shell,
    package: &Package,
    baseline: &str,
    current_apis: PackageApis,
) -> Result<Option<PackageDiff>, Box<dyn std::error::Error>> {
    rbmt_eprintln!("Comparing against baseline: {}", baseline);

    let mut baseline_apis = {
        let _guard = git::GitSwitchGuard::new(sh, baseline)?;
        get_package_apis(sh, &package.name, &package.dir)?
    };

    let mut feature_diffs = Vec::new();
    for (feature_config, entry) in current_apis {
        let current_api = entry.api;
        let baseline_entry = baseline_apis.remove(&feature_config).ok_or(format!(
            "Feature {:?} not found in baseline for {}",
            feature_config, package.name
        ))?;
        let baseline_api = baseline_entry.api;

        let baseline_context = ItemContext::new(&baseline_api);
        let current_context = ItemContext::new(&current_api);
        let diff = public_api::diff::PublicApiDiff::between(baseline_api, current_api);
        if !diff.is_empty() {
            feature_diffs.push(FeatureDiff {
                feature_config,
                diff,
                baseline_context,
                current_context,
            });
        }
    }

    if feature_diffs.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PackageDiff { package_name: package.name.clone(), feature_diffs }))
    }
}

/// A crate name in underscored form, as it appears in Rust import paths.
///
/// Rust identifiers use underscores, but crate and package names in TOML or
/// cargo metadata use hyphens. `CrateName` normalises to the underscored form
/// for comparison against public API tokens.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CrateName(String);

impl CrateName {
    /// Create from a package name (e.g. from rustdoc `external_crates` or
    /// `cargo metadata`). Hyphens are replaced with underscores; names already
    /// in underscored form pass through unchanged.
    fn from_package_name(name: &str) -> Self { Self(name.replace('-', "_")) }

    /// Create from a manifest key, the dependency key as written under
    /// `[dependencies]` in `Cargo.toml`. If the key is an alias for a different
    /// package (via `package = "..."`), resolves through `aliases` to the actual
    /// package name. Falls back to simple normalisation when the name is not a
    /// known alias.
    fn from_manifest_key(name: &str, aliases: &HashMap<String, String>) -> Self {
        let normalized = name.replace('-', "_");
        let resolved = aliases
            .iter()
            .find(|(alias, _)| alias.replace('-', "_") == normalized)
            .map(|(_, package)| package.replace('-', "_"))
            .unwrap_or(normalized);
        Self(resolved)
    }
}

impl std::fmt::Display for CrateName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

/// Check if any private dependencies are exposed in the public API.
///
/// Three detection strategies are used:
///
/// * **Identifier scan**: checks rendered tokens for the private crate's name (catches return
///   types, generics, type aliases).
/// * **Crate-origin scan**: traces `pub use` items to their defining crate via rustdoc JSON
///   metadata (catches item re-exports, which render with the local crate path).
/// * **Extern crate scan**: checks `pub extern crate` declarations against private deps
///   (catches whole-crate re-exports, which the crate-origin scan can't trace).
///
/// # Arguments
///
/// * `package_name` - package being analyzed.
/// * `current` - package public API and crate-origin data per feature config.
/// * `private_deps` - dependencies which should not leak through the exposed API.
/// * `dep_aliases` - maps manifest alias names to package names for ease of configuration.
/// * `deps` - package's full dependency list, used to validate `private_deps` entries.
fn check_private_deps(
    package_name: &str,
    current: &PackageApis,
    private_deps: &[String],
    dep_aliases: &HashMap<String, String>,
    deps: &[String],
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if private_deps.is_empty() {
        return Ok(None);
    }

    let resolved: Vec<CrateName> =
        private_deps.iter().map(|d| CrateName::from_manifest_key(d, dep_aliases)).collect();

    // Validate entries resolve to an actual dependency, else a typo in `private` silently
    // passes giving false confidence.
    let known: HashSet<CrateName> = deps.iter().map(|d| CrateName::from_package_name(d)).collect();
    for (entry, dep) in private_deps.iter().zip(&resolved) {
        if !known.contains(dep) {
            let resolved_suffix = if *dep == CrateName::from_package_name(entry) {
                String::new()
            } else {
                format!(" (resolved to '{dep}')")
            };
            return Err(format!(
                "'{entry}'{resolved_suffix} in [package.metadata.rbmt.api] private list is not a dependency of {package_name}",
            )
            .into());
        }
    }

    let mut leaked_items = Vec::new();

    for (feature_config, entry) in current {
        for item in entry.api.items() {
            // Identifier scan: intermediate path segments (e.g. the
            // `internal_helper` in `internal_helper::InternalHelper`) are
            // rendered as `Identifier` tokens. Catches return types, generics,
            // and type aliases that leak a private dep's name.
            let token_leak = || {
                item.tokens()
                    .filter(|t| matches!(t, public_api::tokens::Token::Identifier(_)))
                    .any(|t| resolved.contains(&CrateName::from_package_name(t.text())))
            };

            // Crate-origin scan: traces `pub use` re-exports to their defining
            // crate via rustdoc JSON metadata. These items render with the local
            // crate's path, so the identifier scan can't catch them.
            let use_origin_leak = || {
                entry
                    .origins
                    .use_origin(item)
                    .is_some_and(|d| resolved.contains(&CrateName::from_package_name(d)))
            };

            // Extern crate scan: checks `pub extern crate` declarations. These
            // have no `id` in rustdoc JSON so the crate-origin scan can't trace
            // them. The extern crate name is the manifest key, i.e. the alias
            // for renamed deps, so resolve it the same way as `private` entries
            // before comparing.
            let extern_crate_leak = || {
                entry.origins.extern_crate_origin(item).is_some_and(|name| {
                    resolved.contains(&CrateName::from_manifest_key(name, dep_aliases))
                })
            };

            if token_leak() || use_origin_leak() || extern_crate_leak() {
                leaked_items.push((feature_config.name(), item.to_string()));
            }
        }
    }

    if !leaked_items.is_empty() {
        let mut message =
            format!("Private dependency exposed in public API of {}:\n", package_name);
        for (feature, item) in leaked_items {
            message.push_str(&format!("  [{}] {}\n", feature, item));
        }
        return Ok(Some(message));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an alias map in the shape `cargo metadata` reports it (raw strings).
    fn aliases(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(alias, name)| (alias.to_string(), name.to_string())).collect()
    }

    #[test]
    fn from_manifest_key_accepts_alias() {
        let deps = aliases(&[("internal", "internal-helper")]);
        assert_eq!(
            CrateName::from_manifest_key("internal", &deps),
            CrateName::from_package_name("internal-helper"),
        );
    }

    #[test]
    fn from_manifest_key_accepts_underscored_alias() {
        let deps = aliases(&[("hex-lite", "hex-conservative")]);
        assert_eq!(
            CrateName::from_manifest_key("hex_lite", &deps),
            CrateName::from_package_name("hex-conservative"),
        );
    }

    #[test]
    fn from_manifest_key_accepts_package_name() {
        let deps = aliases(&[("internal", "internal-helper")]);
        assert_eq!(
            CrateName::from_manifest_key("internal-helper", &deps),
            CrateName::from_package_name("internal-helper"),
        );
    }

    #[test]
    fn from_manifest_key_passes_through_unknown_name() {
        let deps = aliases(&[("internal", "internal-helper")]);
        assert_eq!(
            CrateName::from_manifest_key("typo", &deps),
            CrateName::from_package_name("typo"),
        );
    }

    #[test]
    fn from_package_name_normalizes_hyphens() {
        assert_eq!(CrateName::from_package_name("internal-helper").to_string(), "internal_helper",);
    }

    #[test]
    fn from_package_name_passes_through_underscores() {
        assert_eq!(CrateName::from_package_name("internal_helper").to_string(), "internal_helper",);
    }
}
