# `rust-bitcoin` Maintainer Tools

[![CI](https://github.com/nyonson/rust-bitcoin-maintainer-tools/actions/workflows/ci.yml/badge.svg)](https://github.com/nyonson/rust-bitcoin-maintainer-tools/actions/workflows/ci.yml)

This repository contains utilities for maintaining projects in the rust-bitcoin ecosystem.

* [`actions`](./actions) composable CI actions for Github/Forgejo actions.
* [`ci`](./ci) holds the legacy shell scripts for continuous integration tests in the rust-bitcoin ecosystem.
* [`cargo-rbmt`](./cargo-rbmt) is a rust re-write of the legacy `ci` scripts, providing advanced test and lint features.
* [`docs`](./docs) contains notes on overall design.
* [`forge`](./forge) is a tool for working with the rust-bitcoin's central repository server.
* [`runner`](./runner`) infrastructure for CI runners.

## Contributing

The canonical repository is at [git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools](https://git.rust-bitcoin.org/rust-bitcoin/rust-bitcoin-maintainer-tools). A read-only remote exists on [GitHub](https://github.com/rust-bitcoin/rust-bitcoin-maintainer-tools).

We have a dedicated developer channel on IRC, #bitcoin-rust@libera.chat where you may get helpful advice if you have questions.
