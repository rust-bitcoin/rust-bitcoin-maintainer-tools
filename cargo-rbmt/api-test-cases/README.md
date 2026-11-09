# API Test Cases

A workspace of packages for testing the `cargo-rbmt api` subcommand. The root
package hosts integration tests (in `tests/`) that run the subcommand against
these fixtures and assert on the exit code. Run them with `cargo test`.

* `api-diff`: lean package with a known API change vs a synthetic baseline; exercises the
  diff-detection path (exit 2).
* `api-leaky`: Package whose public API leaks a private dependency through
  return types, generics, and type aliases; exercises the identifier-based leak
  check (exit 3).
* `api-item-reexport`: Package whose only public item is a `pub use` of a private
  dependency; exercises the crate-origin re-export detection (exit 3).
* `api-crate-reexport`: Package whose only public item is a `pub extern crate` of a private
  dependency with the manifest-key spelling (`private = ["internal"]`); exercises the
  extern-crate re-export detection (exit 3).
* `api-crate-reexport-package-name`: Same as `api-crate-reexport` but with the package-name
  spelling (`private = ["internal-helper"]`); verifies both spellings are caught (exit 3).
* `api-no-leak`: Package that uses a private dependency internally without exposing it
  in the public API; exercises the happy path (exit 0).
* `internal-helper`: An internal crate that the leak test packages depend on.
