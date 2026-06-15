# MSRV Test Cases

Contains test scenarios for verifying the `cargo-rbmt test` subcommand's MSRV override functionality.

The `consumer` package has an optional dependency on the `higher-msrv-dep`. run `cargo rbmt test --toolchain msrv --lockfile existing` in `consumer` to check that the correct toolchains are used to test MSRV.
