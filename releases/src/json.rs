//! Types that represent the JSON schema and provide deserialization.
//!
//! We deserialize to Rust std types then convert to more strictly typed types manually.

#![allow(dead_code)] // This is just the JSON schemar, all fields exist.

use serde::Deserialize;

/// The `releases.json` config file.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// The github organisation this config file relates to.
    pub org: String,
    /// List of the latest releases.
    pub latests: Vec<CrateVersion>,
    /// List of all releases we run checks against.
    pub releases: Vec<CrateNode>,
}

/// As specific version of a crate.
#[derive(Debug, Deserialize)]
pub struct CrateVersion {
    /// The crate's package name on crates.io
    pub package: String,
    /// The dependencies semantic version number.
    pub version: String,
}

/// A version of one of the crates along with a list of its dependencies (and their versions) - used
/// to make a dependency graph.
#[derive(Debug, Deserialize)]
pub struct CrateNode {
    /// The crate's package name on crates.io
    pub package: String,
    /// The release's semantic version number.
    pub version: String,
    /// List of this releases dependencies.
    pub dependencies: Vec<CrateVersion>,
}
