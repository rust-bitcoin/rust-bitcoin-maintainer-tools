//! A simple crate with MSRV 1.70.0.
//!
//! This crate requires Rust 1.70.0 or later because it uses
//! [is_some_and](https://doc.rust-lang.org/std/option/enum.Option.html#method.is_some_and),
//! which was [stabilized in Rust 1.70.0](https://releases.rs/docs/1.70.0/#stabilized-apis).

/// A helper function that uses `Option::is_some_and` (stabilized in Rust 1.70.0).
pub fn check_positive(value: Option<i32>) -> bool { value.is_some_and(|v| v > 0) }

#[cfg(test)]
mod tests {
    use super::check_positive;

    #[test]
    fn test_check_positive() {
        assert!(check_positive(Some(5)));
        assert!(!check_positive(Some(-1)));
        assert!(!check_positive(None));
    }
}
