# corepc-regtest-client

Blocking JSON-RPC client for testing against specific versions of Bitcoin Core.

This crate provides version-specific blocking clients (`v17` through `v31`) intended for
integration and regtest testing. It is used by [`bitcoind`](https://crates.io/crates/bitcoind)
and the `corepc` integration test suite.

**Please do not use this crate in production software.**

## History

This crate was split from `corepc-client` at **v0.16.0** to separate the blocking,
version-specific integration/regtest client from the production async client. See issue #674.

## Minimum Supported Rust Version (MSRV)

This library should always compile on **Rust 1.75.0**.

## Licensing

The code in this project is licensed under the [Creative Commons CC0 1.0 Universal license](LICENSE).
We use the [SPDX license list](https://spdx.org/licenses/) and [SPDX IDs](https://spdx.dev/ids/).
