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
- [Run](#run)
- [Lock Files](#lock-files)
- [API](#api)
- [Toolchains](#toolchains)
- [Tools](#tools)
- [Workspace Integration](#workspace-integration)
  - [1. Install on system](#1-install-on-system)
  - [2. Add as a dev-dependency](#2-add-as-a-dev-dependency)
- [CI Actions](#ci-actions)

## Environment Variables

* `RBMT_LOG_LEVEL`
  * `verbose`: [DEFAULT] Print out underlying cargo commands and all their output, good for CI.
  * `quiet`: Suppress both cargo and rbmt stderr to reduce all noise.
  * `progress`: Show rbmt stderr on a single line with a visual indicator, for interactive use.

## Configuration

Configuration for `rbmt` is stored in `[package.metadata.rbmt]` in a package's `Cargo.toml` manifest. Some configuration lives under `[workspace.metadata.rbmt]` in the root manifest of a workspace, but can fallback to `[package.metadata.rbmt]` if there is only one package in the repository.

> **NOTE:** Cargo reserves `[package.metadata]` and `[workspace.metadata]` as explicitly supported extension points for third-party tooling. Cargo itself ignores any keys nested under these tables, so they will never clash with a future built-in Cargo field. `[workspace.metadata]` was stabilized in Cargo 1.46 and `[package.metadata]` has been around much longer. The `rbmt` sub-key further namespaces the configuration to avoid collisions with other tools. If a repository only has one package and is not using any workspace features, use the `package` namespace because simply adding the `workspace.metadata` settings enables workspace features in cargo.

## Format

The `fmt` command formats all files in the workspace using `rustfmt` with the nightly toolchain, which is the convention in the rust-bitcoin ecosystem.

```bash
cargo rbmt fmt
cargo rbmt fmt --check
cargo rbmt fmt -p bitcoin
```

## Lint

The `lint` command detects duplicate dependencies, but some may be unavoidable (e.g., during dependency updates where transitive dependencies haven't caught up). Configure `[package.metadata.rbmt.lint]` to whitelist specific duplicates.

```toml
[package.metadata.rbmt.lint]
allowed_duplicates = [
    "syn",
    "bitcoin_hashes",
]
```

> **NOTE:** Linting is only enforced (through command failure) on the given *nightly* toolchain. It is possible for different versions of rust to have different lint rules and behaviour, so to keep things simple just the newest is considered fail worthy.

## Test

The `test` command runs feature matrix testing for your package. Every run unconditionally tests all features enabled, no features enabled, and each feature by itself. A package's features are auto-discovered. Randomly sampled feature subsets (number of sets grows with the number of package features) are tested per commit ID to try and catch interaction bugs without running massive matrices on every run.

The `--baseline <ref>` flag checks that every commit between `<ref>` and `HEAD` passes the test suite, ensuring the branch remains bisectable.

Arguments after `--` are passed to both build and test commands.

```bash
cargo rbmt test -- -Z build-std --target x86_64-unknown-linux-gnu
```

For test-specific options use environment variables. There are many different layers under the hood, so might take some searching. For example, setting thread counts in tests uses the `RUST_TEST_THREADS` environment variable.

```bash
RUST_TEST_THREADS=4 cargo rbmt test
```

> **NOTE:** The separate build step detects implicit test code dependencies. Example runs use default settings and ignore all arguments.

```toml
[package.metadata.rbmt.test]
# Examples to run with different feature configurations.
#
# Supported formats:
# * "name" - runs with no default features.
# * "name:feature1 feature2" - runs with specific features.
examples = [
    "bip32",              # No default features
    "bip32:serde rand",   # Specific features
]

# Features to exclude from auto-discovery.
# Use for internal or alias features that should not be tested in isolation.
exclude_features = ["_internal", "default-features"]

# Always test specific feature combinations.
exact_features = [
    ["serde", "rand"],        # Test serde and rand interaction.
    ["serde", "std"],         # Assuming serde has a weak dependency on std, test interaction when enabled.
    ["rand", "std"],          # Assuming rand has a weak dependency on std, test interaction when enabled.
    ["serde", "rand", "std"], # Test both with weak dependency interaction.
]

# Run tests on all possible feature subsets instead of the default handful per-commit.
sample_strategy = "all"

# Feature-specific MSRV overrides.
msrv_overrides = { "serde" = "1.75.0" }
```

### no_std

When a package declares `#![no_std]` in its library source, `cargo-rbmt test` automatically performs an additional verification step on the `thumbv7m-none-eabi` target to try and detect unintentional std library usage.

## Integration

The `integration` command runs tests in isolated integration sub-packages designed to work with the [`corepc`](https://github.com/rust-bitcoin/corepc) testing framework, which provides Bitcoin Core binaries and testing infrastructure. Integration sub-packages should define its own `[package.metadata.rbmt.toolchains.stable]` to lock a stable version.

Integration sub-packages should be *standalone*, not members of a workspace. This ensures test infrastructure dependencies don't influence workspace dependency resolution which could change the minimum versions for tests.

```toml
[package.metadata.rbmt.integration]
# Integration tests package name, defaults to "bitcoind-tests".
package = "bitcoind-tests"
# Versions to test. If omitted, tests all discovered versions from Cargo.toml.
versions = ["29_0", "28_2", "27_2"]
```

## Prerelease

The `prerelease` command performs readiness checks before releasing a package. Checks are opt-in and only run for packages with `enabled = true` that also have a version bump in `Cargo.toml` since the baseline ref.

```toml
[package.metadata.rbmt.prerelease]
enabled = true
# baseline = "master"  # default
```

Use `--force` to run checks regardless of whether a version bump is detected.

```bash
cargo rbmt prerelease --force
```

## Run

The `run` command executes arbitrary cargo commands with the specified toolchain and lockfile.

```bash
cargo rbmt run --lockfile minimal --toolchain nightly -- <CARGO_COMMAND> [ARGS...]
```

The `--` separator tells `cargo-rbmt` to stop parsing its own flags and pass everything after it to cargo. For example, here is how to run benchmarks with the nightly toolchain.

```bash
cargo rbmt run --toolchain nightly -- bench
```

## Lock Files

To ensure your package works with the full range of declared dependency versions, `cargo-rbmt` can generate lock files for different version scenarios.

* `Cargo-minimal.lock` - Minimum versions that satisfy your dependency constraints. Verifies that direct dependency versions aren't being bumped by transitive dependencies.
* `Cargo-maximum.lock` - Maximum versions that satisfy your dependency constraints. Verifies new updates do not break.
* `Cargo-recent.lock` - Recent versions start out the same as maximum, but are conservatively updated. Versions are only increased if needed due to new dependency constraints.

The `lock` command generates and maintains these files for you. You can then use `--lockfile` with any command to test against any version set.

```bash
# Generate minimal and recent (default for backward compatibility).
cargo rbmt lock
# Generate all three lock files.
cargo rbmt lock --lockfiles minimal,maximum,recent
```

When you specify `--lockfile`, the tool copies that lock file to `Cargo.lock` before running the command. This allows you to test your code against different dependency version constraints.

```bash
# Test with minimal versions.
cargo rbmt --lockfile minimal test
# Test with maximum versions.
cargo rbmt --lockfile maximum test
```

## API

The `api` command helps maintain API stability by generating public API snapshots and checking for breaking changes. It uses the [public-api](https://github.com/Enselic/cargo-public-api) crate to analyze a crate's public interface.

> **NOTE:** `api` has an implicit dependency on the version of the nightly toolchain since it relies on an unstable docsrs interface. Currently, it requires [*nightly-2025-08-02* or later](https://github.com/cargo-public-api/cargo-public-api/blob/main/README.md#compatibility-matrix).

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

### `#[doc(hidden)]` policy

Items marked with `#[doc(hidden)]` are *excluded from API snapshots and breaking change detection*. `#[doc(hidden)]` is an escape hatch to allow API changes without triggering breaking change warnings in CI. While hiding documentation doesn't change the actual types or signatures, it signals that the item is not part of the public API contract and may be modified or removed without warning.

## Generate

The `generate` command detects changes to generated files by running the file generation script, which is assumed to be at `<package-root>/generate-files.sh`, and running a diff. If the file generation script is not present or a diff is present after file generation, the caller will receive an error.

```bash
# For a single package
cargo rbmt generate -p fuzz

# For multiple packages
cargo rbmt generate -p fuzz -p crypto

```

## Toolchains

The `toolchains` command installs the three required toolchains for `cargo-rbmt` commands, `nightly`, `stable`, and `MSRV`. `nightly` and `stable` Toolchain versions are read from the root manifest `Cargo.toml` of a repository. The `MSRV` is read from all the package manifests in a workspace. Workspaces must declare a single consistent MSRV across all packages. Workspaces with conflicting `rust-version` fields are not supported.

> **NOTE:** This command requires `rustup` on the system, which is not the case for all other `cargo-rbmt` commands.

Workspace enabled repositories should set the versions under the `workspace.metadata.rbmt.toolchains` namespace in the root `Cargo.toml`. If a repository is a single package without a workspace, use the `package.metadata.rbmt.toolchains` namespace instead.

```toml
[workspace.metadata.rbmt.toolchains]
nightly = "nightly-2026-03-13"
stable = "1.93.1"
```

The current versions can be queried with the `--msrv`, `--stable`, or `--nightly` flags.

```bash
cargo +$(cargo rbmt toolchains --nightly) test --features one-off
```

The `--update-nightly` and `--update-stable` flags each install the corresponding floating toolchain, query its resolved version from `rustc`, and write the result to the appropriate version file before proceeding with the normal install and export.

## Tools

The `tools` command installs external cargo tools whose versions are pinned in the *root* `Cargo.toml` manifest. The preferred location is `[workspace.metadata.rbmt.tools]`.

```toml
[workspace.metadata.rbmt.tools]
cargo-semver-checks = "0.46.0"
cargo-public-api = "0.50.1"
```

For single-package repos with no explicit `[workspace]` table, `[package.metadata.rbmt.tools]` is supported as a fallback.

```bash
# Install all tools at their pinned versions.
cargo rbmt tools

# Install only a specific tool.
cargo rbmt tools cargo-semver-checks

# Install each tool at its latest version and update the pins in Cargo.toml.
cargo rbmt tools --update

# Update only a specific tool.
cargo rbmt tools --update cargo-public-api
```

The `--update` flag installs each tool without a version constraint, then reads the resolved version back from `cargo install --list` and writes it into `Cargo.toml`. The resulting diff can be reviewed and committed as a deliberate version bump.

> **Note:** Tools are installed via `cargo install`. Installing or updating a tool overwrites any previously installed version of that binary system-wide. If you rely on a specific version of a tool outside of this workflow, be aware that running `cargo rbmt tools` will replace it with the pinned version.

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

## CI Actions

A composite action is provided to make it easy to use `cargo-rbmt` in Github/Forgejo Actions CI. Although it might be easier to write a custom action per-repository.

For faster CI runs, consider adding cargo build caching to your workflow with something like `Swatinem/rust-cache`.

```yaml
steps:
  - uses: actions/checkout@v6
  - uses: Swatinem/rust-cache@v2
  - uses: rust-bitcoin/rust-bitcoin-maintainer-tools/.github/actions/setup-rbmt@master
  - run: cargo rbmt test
```

See the [action](../actions/setup-rbmt/action.yml) for more details.
