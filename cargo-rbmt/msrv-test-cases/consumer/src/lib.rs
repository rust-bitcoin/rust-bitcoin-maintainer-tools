//! A simple test case demonstrating MSRV overrides.
//!
//! `cargo rbmt test --toolchain msrv` should use the package's `rust-version` when no features
//! are enabled, and `1.70.0` when `higher-msrv-dep` is enabled.

/// A simple struct for testing.
pub struct Example {
    pub value: String,
}

impl Example {
    /// Create a new example.
    pub fn new(value: String) -> Self { Self { value } }
}

/// Re-export from the higher MSRV dependency when the feature is enabled.
#[cfg(feature = "higher-msrv-dep")]
pub use higher_msrv_dep::check_positive;

#[cfg(test)]
mod tests {
    use super::Example;

    #[test]
    fn test_example() {
        let ex = Example::new("hello".to_string());
        assert_eq!(ex.value, "hello");
    }

    #[test]
    #[cfg(feature = "higher-msrv-dep")]
    fn test_higher_msrv_dep() {
        use super::check_positive;
        assert!(check_positive(Some(5)));
        assert!(!check_positive(Some(-1)));
        assert!(!check_positive(None));
    }
}
