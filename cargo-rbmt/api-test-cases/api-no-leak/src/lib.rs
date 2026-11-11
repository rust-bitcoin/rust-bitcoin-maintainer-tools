//! Happy-path test for `cargo rbmt api` private-dep checking.
//!
//! Uses the internal dependency internally but never exposes it in the public
//! API. The private check should succeed (exit 0).

use internal::InternalHelper;

/// Public function that uses the private dep internally but doesn't leak it.
pub fn public_function() -> i32 {
    let _x = InternalHelper::new(42);
    0
}