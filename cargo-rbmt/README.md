# cargo-rbmt

Maintainer tools for Rust-based projects in the Bitcoin domain. Built with [xshell](https://github.com/matklad/xshell).

## Table of Contents

- [Environment Variables](#environment-variables)
- [Configuration](#configuration)
- [Format](#format)
- [Lint](#lint)
- [Test](#test)
  - [no_std](#no_std)
- [Integration](#integration)
- [Prerelease](#prerelease)
- [Lock Files](#lock-files)
- [API](#api)
- [Toolchains](#toolchains)
- [Workspace Integration](#workspace-integration)
  - [1. Install on system](#1-install-on-system)
  - [2. Add as a dev-dependency](#2-add-as-a-dev-dependency)
- [GitHub Action](#github-action)

## Environment Variables

* `RBMT_LOG_LEVEL=quiet` - Suppress verbose output and reduce cargo noise.

## Configuration

Configuration for `rbmt` is stored in a per-package `rbmt.toml` file, a sibling to the package's manifest.

## Format

The `fmt` command formats all files in the workspace using `rustfmt` with the nightly toolchain, which is the convention in the rust-bitcoin ecosystem.

```bash
cargo rbmt fmt
cargo rbmt fmt --check
cargo rbmt fmt -p bitcoin
```

## Lint

The `lint` command detects duplicate dependencies, but some may be unavoidable (e.g., during dependency updates where transitive dependencies haven't caught up). Configure the `[lint]` section to whitelist specific duplicates.

```toml
[lint]
allowed_duplicates = [
    "syn",
    "bitcoin_hashes",
]
```

## Test

The `test` command runs feature matrix testing for your package. Every run unconditionally tests all features enabled, no features enabled, and each feature by itself. A package's features are auto-discovered. Each feature is tested individually, and two randomly-sampled feature subsets are tested per commit ID to try and catch interaction bugs without running massive matrices on every run.

```toml
[test]
# Examples to run with different feature configurations.
#
# Supported formats:
# * "name" - runs with default features.
# * "name:-" - runs with no-default-features.
# * "name:feature1 feature2" - runs with specific features.
examples = [
    "bip32",              # Default features
    "bip32:-",            # No default features
    "bip32:serde rand",   # Specific features
]

# Features to exclude from auto-discovery.
# Use for internal or alias features that should not be tested in isolation.
exclude_features = ["_internal", "default-features"]

# Exact feature combinations to always test.
exact_features = [
    ["serde", "rand"],
    ["rand"],
]
```

The following options are syntax sugar over `exact_features`. They generate all individual and pair combinations from a feature list, optionally prepending a base feature. Use these for packages with a conventional `std` feature or the legacy `no-std` pattern, rather than enumerating every combination manually in `exact_features`.

```toml
# Features to test with the conventional `std` feature enabled.
# Example: ["serde", "rand"] tests: std+serde, std+rand, std+serde+rand
features_with_std = ["serde", "rand"]

# Features to test without any base feature.
# Example: ["serde", "rand", "arbitrary"] tests: serde+rand, serde+arbitrary, rand+arbitrary
# (singles are covered by auto-discovery)
features_without_std = ["serde", "rand", "arbitrary"]

# Features to test with an explicit `no-std` feature (rust-miniscript pattern).
# Example: ["serde", "rand"] tests: no-std+serde, no-std+rand, no-std+serde+rand
features_with_no_std = ["serde", "rand"]
```

### no_std

When a package declares `#![no_std]` in its library source, `cargo-rbmt test` automatically performs an additional verification step on the `thumbv7m-none-eabi` target to try and detect unintentional std library usage.

## Integration

The `integration` command is designed to work with the [`corepc`](https://github.com/rust-bitcoin/corepc) integration testing framework, which provides Bitcoin Core binaries and testing infrastructure.

```toml
[integration]
# Integration tests package name, defaults to "bitcoind-tests".
package = "bitcoind-tests"
# Versions to test. If omitted, tests all discovered versions from Cargo.toml.
versions = ["29_0", "28_2", "27_2"]
```

## Prerelease

The `prerelease` command performs readiness checks before releasing a package. Checks are opt-in and only run for packages with `enabled = true` that also have a version bump in `Cargo.toml` since the baseline ref.

```toml
[prerelease]
enabled = true
# baseline = "master"  # default
```

Use `--force` to run checks regardless of whether a version bump is detected.

```bash
cargo rbmt prerelease --force
```

## Lock Files

To ensure your package works with the full range of declared dependency versions, `cargo-rbmt` requires two lock files in your repository.

* `Cargo-minimal.lock` - Minimum versions that satisfy your dependency constraints.
* `Cargo-recent.lock` - Recent/updated versions of dependencies.

The `lock` command generates and maintains these files for you. You can then use `--lock-file` with any command to test against either version set.

```bash
cargo rbmt lock
```

1. Verify that direct dependency versions aren't being bumped by transitive dependencies.
2. Generate `Cargo-minimal.lock` with minimal versions across the entire dependency tree.
3. Update `Cargo-recent.lock` with conservatively updated dependencies.

```bash
# Test with minimal versions.
cargo rbmt --lock-file minimal test stable

# Test with recent versions.
cargo rbmt --lock-file recent test stable

# Works with any command.
cargo rbmt --lock-file minimal lint
cargo rbmt --lock-file minimal docs
```

When you specify `--lock-file`, the tool copies that lock file to `Cargo.lock` before running the command. This allows you to test your code against different dependency version constraints.

## API

The `api` command helps maintain API stability by generating public API snapshots and checking for breaking changes. It uses the [public-api](https://github.com/Enselic/cargo-public-api) crate to analyze a crate's public interface. **Requires running with a nightly toolchain after nightly-2025-08-02** due to docsrs dependencies.

```bash
cargo rbmt api
```

1. Generates API snapshots for feature configurations.
2. Validates that features are additive (enabling features only adds to the API, never removes).
3. Checks for uncommitted changes to API files.

The generated API files are stored in `api/<package-name>/`.

```bash
cargo rbmt api --baseline v0.1.0
```

Compares the current API against a baseline git reference (tag, branch, or commit) to detect breaking changes.

## Toolchains

The `toolchains` command installs the three required toolchains for `cargo-rbmt` commands, `nightly`, `stable`, and `MSRV`. Toolchain versions are read from the repository. `nightly` and `stable` are set in `nightly-version` and `stable-version` files respectively. `MSRV` is read from package manifests. Workspaces must declare a single consistent MSRV across all packages. Workspaces with conflicting `rust-version` fields are not supported.

This command requires `rustup` on the system, which is not the case for all other `cargo-rbmt` commands.

The command prints `export` statements to stdout and all other output to stderr, so it can be used with `eval` to set toolchain version environment variables in the calling shell.

```bash
eval "$(cargo rbmt toolchains)"

cargo +$RBMT_NIGHTLY rbmt lint
cargo +$RBMT_STABLE rbmt test
cargo +$RBMT_MSRV rbmt test --toolchain msrv
```

The `--update-nightly` and `--update-stable` flags each install the corresponding floating toolchain, query its resolved version from `rustc`, and write the result to the appropriate version file before proceeding with the normal install and export.

## Workspace Integration

`cargo-rbmt` can simply be installed globally on a system or added as a dev-dependency to a package.

### 1. Install on system

Install the tool globally on your system with `cargo install`.

```bash
cargo install cargo-rbmt@0.1.0
```

Then run from anywhere in your repository as a cargo subcommand. It can also be called directly as `cargo-rbmt`.

```bash
cargo rbmt lint
```

### 2. Add as a dev-dependency

Add as a dev-dependency to a workspace member. This pins the tool version in your lockfile for reproducible builds. But this also means that `cargo-rbmt` dependencies could influence version resolution for the workspace.

```toml
[dev-dependencies]
cargo-rbmt = "0.1.0"
```

Then run via cargo.

```bash
cargo run --bin cargo-rbmt -- lint
```

It might be worth wrapping in an [xtask](https://github.com/matklad/cargo-xtask) package for a clean interface.

## GitHub Action

A composite action is provided to make it easy to use `cargo-rbmt` in Github Actions CI.

```yaml
steps:
  - uses: actions/checkout@v6
  - uses: rust-bitcoin/rust-bitcoin-maintainer-tools/.github/actions/setup-rbmt@master
    id: setup
  - run: cargo +${{ steps.setup.outputs.stable }} rbmt test
```

See the [action](../.github/actions/setup-rbmt/action.yml) for more details.
