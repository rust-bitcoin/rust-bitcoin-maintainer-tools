use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, process};

use anyhow::Context;
use clap::{arg, command, value_parser, Command};
use crates_io_api::AsyncClient;
use releases::{json, Config, CrateVersion};
use semver::Version;
use toml::Table;

/// A green tick in UTF-8.
const TICK: &str = "\x1b[92m\u{2713}\x1b[0m";

/// A red cross in UTF-8.
const CROSS: &str = "\x1b[91m\u{2717}\x1b[0m";

/// API rate limit.
const RATE_LIMIT_MILLIS: u64 = 100;

/// The json file with all the versions in it.
const DEFAULT_CONFIG_FILE: &str = "./releases.json";

// (Tobin) I'm not sure what this email address is used for, I guess its as a point of contact for
// API abuse - if that is the case then using an address that will get back to me is ok.
const DEFAULT_USER_AGENT: &str = "releases_bot (releases_bot@tobin.cc)";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // FIXME: I can't get clap to set these with `.default_value`?
    let mut config_file = PathBuf::from(DEFAULT_CONFIG_FILE);
    let mut user_agent = DEFAULT_USER_AGENT;

    let matches = command!() // requires `cargo` feature
        .arg(
            arg!(
                --user_agent <AGENT> "Set the user agent"
            )
            .required(false)
            .value_parser(value_parser!(String)),
        )
        .arg(
            arg!(
                -c --config <FILE> "Sets a custom config file"
            )
            .required(false)
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
                -d --debug ... "Turn debugging information on"
            )
            .required(false)
            .value_parser(value_parser!(u8)),
        )
        .subcommand(
            Command::new("show-latest-releases")
                .about("show latest expected releases from config file"),
        )
        .subcommand(
            Command::new("check-latest-releases").about(
                "check latest releases from config file against latest releases on crates.io",
            ),
        )
        .subcommand(
            Command::new("check-latest-dependencies")
                .about(
                    "check repo uses the latest releases (from crates.io) for all its dependencies",
                )
                .arg(
                    arg!([repository] "Path to the repository to check")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!([crate_name] "Crate name (only required for workspace)")
                        .required(false)
                        .value_parser(value_parser!(String)),
                ),
        )
        .get_matches();

    // Flags can have multiple occurrences, but we don't currently support verbose debugging output.
    let debug = *matches.get_one::<u8>("debug").expect("Count's are defaulted");
    if debug > 0 {
        println!("Debugging is on");
    }

    if let Some(file) = matches.get_one::<PathBuf>("config") {
        config_file = file.to_path_buf();
    }
    if debug > 0 {
        println!("Using config file: {}", config_file.display());
    }

    if let Some(agent) = matches.get_one::<String>("user_agent") {
        user_agent = agent;
    }

    let config = read_config_file(&config_file)?;

    if matches.subcommand_matches("show-latest-releases").is_some() {
        show_releases(&config.latests)?;
        process::exit(0);
    }

    // Everything else needs the API client.
    let cli = AsyncClient::new(user_agent, Duration::from_millis(RATE_LIMIT_MILLIS))?;

    if matches.subcommand_matches("check-latest-releases").is_some() {
        check_latest_releases(&cli, &config.latests, debug).await?;
        process::exit(0);
    }

    if let Some(sub) = matches.subcommand_matches("check-latest-dependencies") {
        let repo = sub.get_one::<PathBuf>("repository").expect("missing directory argument");
        let crate_name = sub.get_one::<String>("crate_name");
        check_latest_dependencies(&cli, repo, crate_name, debug).await?;
    }

    Ok(())
}

fn read_config_file(file: &Path) -> anyhow::Result<Config> {
    let data = fs::read_to_string(file)?;
    let json: json::Config = serde_json::from_str(&data)?;
    let config = Config::try_from(json)?;

    Ok(config)
}

/// Prints a list of `releases`.
fn show_releases(releases: &[CrateVersion]) -> anyhow::Result<()> {
    println!();
    for release in releases {
        println!("    - {:20} {}", release.package, release.version);
    }

    Ok(())
}

/// Checks the releases in `latests` against the latest releases on crates.io
async fn check_latest_releases(
    cli: &AsyncClient,
    latests: &[CrateVersion],
    _debug: u8,
) -> anyhow::Result<()> {
    let mut found_stale = false;

    println!(
        "\nChecking release versions in config file against the latest release on crates.io\n"
    );

    for latest in latests {
        let released = api_latest(cli, &latest.package).await?;
        if latest.version != released {
            found_stale = true;
            println!(
                "Latest crates.io release {} {} does not match the config file latest version {}",
                latest.package, released, latest.version
            );
        } else {
            println!("{} {} \x1b[92m\u{2713}\x1b[0m", latest.package, latest.version);
        }
    }

    if !found_stale {
        println!("\nAll releases on crates.io match those in the config file")
    }
    Ok(())
}

/// Checks if a crate in `crate_dir` is using the latest dependencies released on `crates.io`.
async fn check_latest_dependencies(
    cli: &AsyncClient,
    repo_dir: &Path,
    crate_name: Option<&String>,
    _debug: u8,
) -> anyhow::Result<()> {
    let mut path = repo_dir.to_path_buf();
    if let Some(name) = crate_name {
        path.push(name);
    };
    path.push("Cargo.toml");

    let crate_name = match crate_name {
        Some(name) => name,
        // next_back is equivalent to last() but more efficient
        None => repo_dir.iter().next_back().expect("invalid repository").to_str().unwrap(),
    };

    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read manifest from {}", path.display()))?;

    let manifest = data.parse::<Table>()?;

    let package_section = manifest["package"].as_table().expect("manifest has package section");
    let crate_version = package_section.get("version").expect("all crates have a version");

    println!("\nChecking latest dependencies for:");
    println!("    crate: {}", crate_name);
    println!("    version: {}", crate_version);
    println!("    manifest: {}", path.display());
    println!();

    let dependencies_section =
        manifest["dependencies"].as_table().expect("manifest has dependencies section");
    let mut deps: Vec<String> = dependencies_section.keys().map(Clone::clone).collect();
    deps.sort();
    for key in deps {
        let value = dependencies_section.get(&key).expect("we know this key exists");
        let package = match value.as_table() {
            Some(t) => match t.get("package") {
                Some(v) => v.as_str().unwrap_or(&key),
                None => &key,
            },
            None => &key,
        };
        let version = match value.as_table() {
            Some(t) => match t.get("version") {
                Some(v) => v.as_str(),
                None => None,
            },
            None => value.as_str(),
        };

        let latest = api_latest(cli, package).await?;

        // If version is not specified in the manifest cargo uses latest.
        match version {
            Some(version) => {
                let version = Version::parse(version)?;
                if latest.major != version.major || latest.minor != version.minor {
                    println!("    - {:20} {}      {} latest: {}", package, CROSS, version, latest);
                } else if latest.patch != version.patch {
                    println!(
                        "    - {:20} {}      {} latest: {}",
                        package, TICK, version, latest
                    );
                } else {
                    println!("    - {:20} {}      {}", package, TICK, latest);
                }
            }
            None => println!("    - {:20} {}", package, latest),
        };
    }

    Ok(())
}

/// Gets the latest released version of `package` from `crates.io`.
async fn api_latest(cli: &AsyncClient, package: &str) -> anyhow::Result<Version> {
    let response = cli.get_crate(package).await?;
    Ok(Version::parse(&response.crate_data.max_version)?)
}
