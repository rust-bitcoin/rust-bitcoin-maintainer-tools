//! Private dependency leak detection test cases for the `cargo rbmt api` command.
//!
//! Every item in this package leaks `internal_helper` into the public API.
//! The private check should catch all of them (exit 1).

use internal::{InternalHelper, InternalTrait};

/// Leaks an internal type in the return type.
pub fn use_internal_helper() -> InternalHelper {
    InternalHelper::new(42)
}

/// Type alias leaking internal type.
pub type InternalAlias = internal::InternalHelper;

/// Generic parameter with internal bound.
pub fn generic_with_internal_bound<T: internal::InternalTrait>() {}

/// Nested generic with internal type.
pub fn nested_generic() -> Vec<internal::InternalHelper> {
    vec![]
}

/// Using a star import and then using the imported type.
pub fn star_import_case() -> InternalHelper {
    InternalHelper::new(99)
}