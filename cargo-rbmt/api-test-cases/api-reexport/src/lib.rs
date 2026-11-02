//! Re-export leak detection test for `cargo rbmt api`.
//!
//! The only public item is a re-export of a private dependency. The identifier
//! scan can't catch this because `pub use` renders with the local crate path.
//! Detection requires the crate-origin check via rustdoc JSON.

pub use internal::InternalHelper as ReexportedInternalHelper;
