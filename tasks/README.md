# Maintainer Tools

Maintainer tools for Rust-based projects in the Bitcoin domain. Built with [xshell](https://github.com/matklad/xshell).

## Configuration

Configuration for `rbmt` is stored in `contrib/rbmt.toml`.

### Lint

The `lint` command detects duplicate dependencies, but some may be unavoidable (e.g., during dependency updates where transitive dependencies haven't caught up). Configure the `[lint]` section to whitelist specific duplicates.

```toml
[lint]
allowed_duplicates = [
    "syn",
    "bitcoin_hashes",
]
```

### Environment Variables

* `RBMT_LOG_LEVEL=quiet` - Suppress verbose output and reduce cargo noise.

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
