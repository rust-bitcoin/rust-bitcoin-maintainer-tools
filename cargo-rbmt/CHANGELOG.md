# Changelog

## [Unreleased]

## [0.3.0] - 2026-05-15

* New `generate` command ensures files are generated.
* Capture failure output in `docs`, `docsrs`, and `fmt` commands.
* Enforce toolchain in `integration` package.
* Dedupe lines in API file output.
* [BREAKING] Refactor the `test` command interface to allow any cargo commands to be passed to all the underlying `cargo test` commands.

## [0.2.1] - 2026-05-04

* Fix color output in `progress` mode.
* Add a `version` command which shows git revision and manifest version of rbmt.

## [0.2.0] - 2026-04-29

* Switch license from CC0 to Apache-2.0/MIT.
* Add `run` command as a cargo passthrough with toolchain and lockfile management.
* Add `progress` mode to `RBMT_LOG_LEVEL` for interactive use.
* Add trailing newline clean up to `fmt`.
* Add building example docs.
* More robust lockfile management when using the baseline feature of `test`.
* Support older MSRVs (e.g. 1.56.0) for lockfile management.

## [0.1.0] - 2026-03-20

Initial release of `cargo-rbmt`, a cargo subcommand for rust-bitcoin maintainer workflows. This matches the functionality of the legacy ci shell scripts and codifies a few common job patterns.

* **Lock File Management** // `lock` manages cargo lock files for minimal, existing, and recent dependency versions.
* **Toolchain Management** // `toolchains` can install and manage Rust toolchains (stable, nightly, MSRV) with automatic pinning. The correct toolchain is automatically selected per-command when necessary.
* **General Linting** // `fmt`, `lint`, `docs` and friends handle general linting.
* **Find Duplicate Dependencies** // `lint` detects duplicate dependencies in a package and across a workspace.
* **Matrix Tests** // Test across toolchains (stable, nightly, MSRV), dependencies (minimal, recent), and feature sets of a package. Can also ensure bisectability hammering all commits in a branch.
* **API Checks** // `api` can help expose accidental changes to the exposed API of a package.
* **Prerelease Help** // Run readiness checks before a crate is released with `prerelease`.
* **Tool Version Pinning** // Install and update tools pinned in workspace metadata with the `tools` command.
* **Importable CI Actions** // GitHub/Forgejo actions for common tasks like toolchain and tool version updates.

[Unreleased]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.3.0...HEAD
[0.3.0]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.2.1...cargo-rbmt-0.3.0
[0.2.1]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.2.0...cargo-rbmt-0.2.1
[0.2.0]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.1.0...cargo-rbmt-0.2.0
[0.1.0]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/releases/tag/cargo-rbmt-0.1.0
