//! Crate re-export leak detection with package-name spelling for `cargo rbmt api`.
//!
//! Re-exports the private dependency as a whole crate via `pub extern crate`.
//! The `private` list uses the package name (`internal-helper`) rather than the
//! manifest key (`internal`). Both spellings must be caught (exit 3).

pub extern crate internal;