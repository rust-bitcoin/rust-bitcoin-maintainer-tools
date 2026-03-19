# Caching

cargo-rbmt is a cargo orchestrator, it mostly just invokes a series of cargo commands. So
cargo-rbmt depends on cargo's performance when doing heavy duty test matrices (e.g. feature
sets across different toolchains).

Cargo caches artifacts in a workspace's `target/` directory. [Fingerprinting] techniques
are used for cache freshness. The consequence is that artifacts are reused across jobs wherever
the compiler version, feature flags, and compilation profile are the same, with no
configuration required.

However, the fingerprinting is complex and sometimes things appear to be needlessly built.
For example, all nightly toolchains are key'd under the same hash. So if you change the
nightly toolchain you are on, and then flip back, all of the artifacts will be replaced.

Another issue is that cargo's `target/` cache is per-workspace, so artifacts are not shared
between workspaces on the same machine. The theoretical fix is a shared, per-user artifact
cache. This has been a long-standing cargo feature request, see [rust-lang/cargo#5931].
"per-user" means an OS user, so a shared cache would benefit all workspaces on a machine
without any coordination.

The complex fingerprinting and the per-workspace cache can cause issues for test performance.

An alternative, seen in Andrew Poelstra's [Nix-based CI setup], is to "nixify what cargo does to
ensure an effective cache". Nix never forgets what it already built, but it works by building test
binaries directly, skipping cargo entirely. Since cargo-rbmt is a cargo orchestrator there is no
good middle ground between these two approaches. You either use cargo's cache or you replace it.

[fingerprinting]: https://github.com/rust-lang/cargo/blob/master/src/cargo/core/compiler/fingerprint/mod.rs
[rust-lang/cargo#5931]: https://github.com/rust-lang/cargo/issues/5931
[Nix-based CI setup]: https://github.com/apoelstra/local-nix-ci
