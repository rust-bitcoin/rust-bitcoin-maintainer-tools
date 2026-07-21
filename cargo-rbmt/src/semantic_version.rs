// SPDX-License-Identifier: MIT AND Apache-2.0

//! Semantic version parsing and comparison.

/// Represents a semantic version (major.minor.patch).
///
/// The derived ordering depends on the order of the fields to match semantic version rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Parse a version string like "1.29.0" into a Version struct.
    pub fn parse(version_str: &str) -> Option<Self> {
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() < 2 {
            return None;
        }

        let major = parts[0].parse::<u32>().ok()?;
        let minor = parts[1].parse::<u32>().ok()?;
        let patch = if parts.len() > 2 { parts[2].parse::<u32>().ok()? } else { 0 };

        Some(Self { major, minor, patch })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parse() {
        assert_eq!(Version::parse("1.29.0"), Some(Version { major: 1, minor: 29, patch: 0 }));
        assert_eq!(Version::parse("1.30.1"), Some(Version { major: 1, minor: 30, patch: 1 }));
        assert_eq!(Version::parse("1.29"), Some(Version { major: 1, minor: 29, patch: 0 }));
        assert_eq!(Version::parse("1"), None);
        assert_eq!(Version::parse("invalid"), None);
    }

    #[test]
    fn test_version_comparison() {
        let v1_28 = Version::parse("1.28.0").unwrap();
        let v1_29 = Version::parse("1.29.0").unwrap();
        let v1_30 = Version::parse("1.30.0").unwrap();

        assert!(v1_28 < v1_29);
        assert!(v1_29 < v1_30);
        assert!(v1_30 > v1_28);
    }
}
