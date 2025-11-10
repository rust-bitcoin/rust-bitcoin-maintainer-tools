# Maintainer Tools

Maintainer tools for Rust-based projects in the Bitcoin domain. Built with [xshell](https://github.com/matklad/xshell).

## Configuration

Configuration for `rbmt` is stored in `contrib/rbmt.toml`. The file can live at both the workspace root (e.g. `$ROOT/contrib/rbmt.toml`) as well as per-crate (e.g. `$ROOT/$CRATE/contrib/rbmt.toml`) within a repository.

### Lint

The `lint` command detects duplicate dependencies, but some may be unavoidable (e.g., during dependency updates where transitive dependencies haven't caught up). Configure the `[lint]` section to whitelist specific duplicates for a workspace (or a crate if only one crate in a repository).

```toml
[lint]
allowed_duplicates = [
    "syn",
    "bitcoin_hashes",
]
```

### Test

The `test` command can be configured to run feature matrix testing for your crate. Configure with the `contrib/rbmt.toml` file at the crate level.

```toml
[test]
# Examples to run with specific features enabled.
# Format: "example_name:feature1 feature2"
examples = [
    "example1:serde",
    "example2:serde rand",
]

# Features to test with the conventional `std` feature enabled.
# Tests each feature alone with std, all pairs, and all together.
# Example: ["serde", "rand"] tests: std+serde, std+rand, std+serde+rand
features_with_std = ["serde", "rand"]

# Features to test without the `std` feature.
# Tests each feature alone, all pairs, and all together.
# Example: ["serde", "rand"] tests: serde, rand, serde+rand
features_without_std = ["serde", "rand"]

# Exact feature combinations to test.
# Use for crates that don't follow conventional `std` patterns.
# Each inner array is tested as-is with no automatic combinations.
# Example: [["serde", "rand"], ["rand"]] tests exactly those two combinations
exact_features = [
    ["serde", "rand"],
    ["rand"],
]

# Features to test with an explicit `no-std` feature enabled.
# Only use if your crate has a `no-std` feature (rust-miniscript pattern).
# Tests each feature with no-std, all pairs, and all together.
# Example: ["serde", "rand"] tests: no-std+serde, no-std+rand, no-std+serde+rand
features_with_no_std = ["serde", "rand"]
```

### Environment Variables

* `RBMT_LOG_LEVEL=quiet` - Suppress verbose output and reduce cargo noise.

## Lock Files

To ensure your crate works with the full range of declared dependency versions, `rbmt` requires two lock files in your repository.

* `Cargo-minimal.lock` - Minimum versions that satisfy your dependency constraints.
* `Cargo-recent.lock` - Recent/updated versions of dependencies.

The `rbmt lock` command generates and maintains these files for you. You can then use `--lock-file` with any command to test against either version set.

### Usage

**Generate/update lock files**

```bash
rbmt lock
```

1. Verify that direct dependency versions aren't being bumped by transitive dependencies.
2. Generate `Cargo-minimal.lock` with minimal versions across the entire dependency tree.
3. Update `Cargo-recent.lock` with conservatively updated dependencies.

**Use a specific lock file**

```bash
# Test with minimal versions.
rbmt --lock-file minimal test stable

# Test with recent versions.
rbmt --lock-file recent test stable

# Works with any command.
rbmt --lock-file minimal lint
rbmt --lock-file minimal docs
```

When you specify `--lock-file`, the tool copies that lock file to `Cargo.lock` before running the command. This allows you to test your code against different dependency version constraints.

## Workspace Integration

`rbmt` can simply be installed globally, or as a dev-dependency for more granular control of dependency versions.

### 1. Install globally

Install the tool globally on your system with `cargo install`.

```bash
cargo install rust-bitcoin-maintainer-tools@0.1.0
```

Then run from anywhere in your repository.

```bash
rbmt lint
```

### 2. Add as a dev-dependency

Add as a dev-dependency to a workspace member. This pins the tool version in your lockfile for reproducible builds.

```toml
[dev-dependencies]
rust-bitcoin-maintainer-tools = "0.1.0"
```

Then run via cargo.

```bash
cargo run --bin rbmt -- lint
```

It might be worth wrapping in an [xtask](https://github.com/matklad/cargo-xtask) package for a clean interface.
