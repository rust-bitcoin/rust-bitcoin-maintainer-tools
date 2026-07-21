# Changelog

## [Unreleased]

## [0.5.1] - 2026-07-15

* Fix bug in `run` where package name instead of package ID are passed to cargo.

## [0.5.0] - 2026-07-08

* [BREAKING] Combine `docs` and `docsrs` into one `docs` command which runs docsrs mode by default. Use `--no-docsrs` for the stable toolchain docs build.
* Add private dependency leaks in public api detection to `api`.
* [BREAKING] Refactor the `api` command around diff detection.
* Fix discrepancy between `nightly` toolchain commit date vs. publish date.
* Add `--baseline` functionality to the `run` command to run per-commit.
* Push `--lockfile` arg to be per-subcommand since about half of the subcommands don't use it.
* Add version check at runtime if `workspace.metadata.rbmt.version` is set to a semver or hash value to ensure the right version of `cargo-rbmt` is running.

## [0.4.1] - 2026-06-24

* Fix windows support with correct internal `Path` usage.
* Fix `docs`/`docsrs` bug where a package with no examples is a no-op.

## [0.4.0] - 2026-06-16

* Add dynamic MSRVs based on features to the `test` command.
* [BREAKING] Drop the `-` syntax for running an example with no default features in `test`. Instead, an empty list now means no default features.
* `api` output files no longer de-dupe equivalent lines, instead they add relevant context to each line (e.g. add the trait impl).
* Add a `sample_strategy` configuration to the `test` command in order to allow testing all possible feature sets.
* Add the `Cargo-maximum.lock` lockfile version for maximum dependency version testing.
* Tweak how build and test args are passed down in `test`.
* Rename the `--lock-file` flag to `--lockfile` to match cargo conventions (e.g. the unstable `--lockfile-path flag`). Still has an alias for backwards compatibility.
* [BREAKING] Drop the `bench` command in favor of just running `run -- bench`.
* [BREAKING] Remove the debug assertions flag from `test`, users should set `RUSTFLAGS` directly.

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

## 0.1.0 - 2026-03-20

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

[Unreleased]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.5.1...HEAD
[0.5.1]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.5.0...cargo-rbmt-0.5.1
[0.5.0]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.4.1...cargo-rbmt-0.5.0
[0.4.1]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.4.0...cargo-rbmt-0.4.1
[0.4.0]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.3.0...cargo-rbmt-0.4.0
[0.3.0]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.2.1...cargo-rbmt-0.3.0
[0.2.1]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.2.0...cargo-rbmt-0.2.1
[0.2.0]: https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools/compare/cargo-rbmt-0.1.0...cargo-rbmt-0.2.0
