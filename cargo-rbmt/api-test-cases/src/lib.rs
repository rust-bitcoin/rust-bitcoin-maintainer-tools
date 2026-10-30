//! # API Test Cases
//!
//! This package contains fun scenarios for the cargo-rbmt `api` subcommand.

pub mod associated_types;
pub mod blanket_trait_bounds;
pub mod generic_specialized_impls;
pub mod multiple_from_impls;
pub mod multiple_trait_impls;
pub mod trait_default_methods;

use internal::{InternalHelper, InternalTrait};

/// This function leaks an internal type in its return type.
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
