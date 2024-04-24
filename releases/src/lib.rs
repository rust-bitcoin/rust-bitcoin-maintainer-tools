// SPDX-License-Identifier: CC0-1.0

//! Tool to check various release related things.

// Coding conventions.
#![warn(missing_docs)]

use std::fmt;

use anyhow::bail;
use semver::Version;

pub mod json;

/// The state of the rust-bitcoin org that this tool aims to run checks on.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Config {
    /// List of the latest releases.
    pub latests: Vec<CrateVersion>,
    /// The releases we want to check.
    pub releases: Vec<CrateNode>,
}

impl Config {
    /// Returns the latest release of the given crate.
    pub fn latest(&self, package: &str) -> anyhow::Result<Version> {
        let mut want = None;
        for latest in self.latests.iter() {
            if latest.package == package {
                want = Some(latest.version.clone());
            }
        }
        if want.is_none() {
            bail!("package {} is not listed in latest section of config file", package);
        }

        let mut found = Version::parse("0.0.0").expect("valid zero version");
        let mut release = None;
        for r in self.releases.iter() {
            if r.package == package && r.version > found {
                found = r.version.clone();
                release = Some(r);
            }
        }
        match release {
            Some(r) => {
                if r.version != want.expect("checked above") {
                    bail!("the latest version in the releases section for {} does not match the verison in the latest section", package);
                }
                Ok(r.version.clone())
            }
            None => bail!("we don't have a release in the config file for {}", package),
        }
    }
}

impl TryFrom<json::Config> for Config {
    type Error = semver::Error;

    fn try_from(json: json::Config) -> Result<Self, Self::Error> {
        let latests: Result<Vec<_>, _> = json.latests.into_iter().map(TryFrom::try_from).collect();
        let releases: Result<Vec<_>, _> =
            json.releases.into_iter().map(TryFrom::try_from).collect();
        Ok(Self { latests: latests?, releases: releases? })
    }
}

/// As specific version of a crate.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CrateVersion {
    /// The crate's package name on crates.io
    pub package: String,
    /// The dependencies semantic version number.
    pub version: Version,
}

impl TryFrom<json::CrateVersion> for CrateVersion {
    type Error = semver::Error;

    fn try_from(json: json::CrateVersion) -> Result<Self, Self::Error> {
        Ok(Self { package: json.package, version: Version::parse(&json.version)? })
    }
}

/// A version of one of the crates that lives in the github.com/rust-bitcoin org.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CrateNode {
    /// The crate this release is for.
    pub package: String,
    /// The release's semantic version number.
    pub version: Version,
    /// List of this releases dependencies.
    pub dependencies: Vec<CrateVersion>,
}

impl fmt::Display for CrateNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.package, self.version)
    }
}

impl TryFrom<json::CrateNode> for CrateNode {
    type Error = semver::Error;

    fn try_from(json: json::CrateNode) -> Result<Self, Self::Error> {
        let mut dependencies = vec![];
        for d in json.dependencies {
            let converted = CrateVersion::try_from(d)?;
            dependencies.push(converted);
        }

        Ok(Self { package: json.package, version: Version::parse(&json.version)?, dependencies })
    }
}
