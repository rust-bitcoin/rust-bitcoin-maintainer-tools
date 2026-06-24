# Gating MSRVs

This doc contains notes on a strategy used to gate an MSRV upgrade behind a feature flag instead of a major semver bump.

The changes from the `0.32.x` branch containing rust-bitcoin's `bitcoin` crate and the `master` branch are substantial. `master` holds the new, smashed out, stabilized crates like `consensus-encoding`, `units`, and `primitives`. Ideally, users of the `0.32.x` series can start using these new stabilized types without having to first switch to some `v0.33.0` series which would change the types of *everything* (a major version bump). But the `0.32.x` series has an MSRV of `1.56.1` while the new crates are using `1.74.0` (for pretty essential stuff like GATs). This is about a 2 year jump, not insignificant. We first attempted to release a `0.32.100` version which just bumped the MSRV to `1.74.0`, but realized after some complaints that there was a friendlier way to do this while still avoiding the `0.33.0` release. Add the new crates behind a feature flag. Cargo only attempts to build the `1.74.0` requiring code if a consumer opts-in and enables the flag (this requires disciplined dependency management, but it is possible).

Great! However we ran into one complications. The new crates are also making use of cargo's weak-dependency syntax. This has an MSRV of `1.60.0` and is *not* able to be hidden behind a feature flag. What isn't well documented though is that pre-`1.60.0` toolchains appear to simply ignore crates published to a registry using weak-dependency syntax. This is referred to as "v2 metadata". So how does `0.32.x`'s old `1.56.1` MSRV deal with the feature flag as well as the v2 metadata?

## V3 Resolver

One side note, this might sound like the perfect task for cargo's v3 resolver, but that didn't stabilize until `1.84.0`. Asking consumers to generate lockfiles with relatively fresh versions of cargo is not practical.

## Tests

All of these tests are making use of the fictional [`bitcoin-msrv-compat`](https://crates.io/crates/bitcoin-msrv-compat) crate which only has 2 versions published. `v1.0.0` is empty, but has a stated MSRV of `1.56.0`. `v1.1.0` re-exports the stabilized crates which means it has an MSRV of `1.74.0`, and it truthfully states that in its manifest's `rust-version`. Also, `v1.1.0`'s dependency tree makes use of cargo's weak-dependency syntax which means its registry index uses the "v2" metadata.

### V2 Metadata

The first test is a package on the `1.56.0` toolchain.

```
[package]
name = "consumer"
version = "0.0.0"
rust-version = "1.56.0"

[dependencies]
bitcoin-msrv-compat = "1"
```

* The `1.56.0` toolchain selects `v1.0.0` for `bitcoin-msrv-compat`. I believe this is due to the toolchain not understanding the "v2" metadata format of `v1.1.0`.
* The `1.60.0` toolchain selects `v1.1.0`, I believe this is because it can understand the v2 metadata, but then fails to build it due to its 1.74.0 MSRV:

  ```
  error: package `bitcoin-consensus-encoding v1.0.0` cannot be built because it requires rustc 1.74.0 or newer, while the currently active rustc version is 1.60.0
  ```

* An older toolchain like `1.50.0` chokes on `bitcoin-msrv-compat`'s 2021 edition, but this isn't related to the issue.

### With a Feature Flag

The second test is for toolchains which understand the "v2" metadata, but don't meet the required `1.74.0` MSRV. We saw in teh first test that if we do nothing, it grabs the `v1.1.0` version, but then fails to build. So we put the dependency behind a feature flag. It still gets `v1.1.0` added to the lockfile, but is only built if a consumer opts in by enabling the feature. Consumers using pre `1.74.0` should not do this.

```
[package]
name = "consumer"
version = "0.0.0"
rust-version = "1.60.0"

[dependencies]
bitcoin-msrv-compat = { version = "1", optional = true }
```

* The `1.60.0` puts `v1.1.0` in the lockfile, but only fails if `--features bitcoin-msrv-compat` is included in the build.
* The `1.74.0` toolchain works fine since it meets the MSRV requirements.

## Conclusion

* Users on toolchains before `1.60.0` will simply not see or be effected by the `0.32.101` release.
* Users on toolchains between `1.60.0` and `1.74.0` will see the minor version change, but not be effected unless they (or one of their dependencies) enables the `encoding` feature flag hiding the `1.74.0` code in `0.32.101`.
* Users on toolchains after `1.74.0` will see the minor version change and could opt-in to the new crates by enabling this `encoding` flag.

Old toolchains are not effected and newer toolchains can opt-in.
