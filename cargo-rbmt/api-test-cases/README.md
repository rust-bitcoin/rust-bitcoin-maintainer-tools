# API Test Cases

A workspace of packages for testing the `cargo-rbmt api` subcommand.

* `api-diff`: lean package with a known API change vs a synthetic baseline; exercises the diff-detection path (exit 2).
* `api-leaky`: Package whose public API leaks a private dependency; exercises the private-dep leak check (exit 1).
* `internal-helper`: An internal crate that `api-leaky` depends on.
