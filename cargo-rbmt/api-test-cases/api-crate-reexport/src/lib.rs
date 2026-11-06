//! Crate re-export leak detection test for `cargo rbmt api`.
//!
//! Re-exports the private dependency as a whole crate via `pub extern crate`.
//! The identifier and crate-origin scans can't catch this because
//! `ItemEnum::ExternCrate` has no `id` to trace through rustdoc JSON paths.

pub extern crate internal;