# corepc-client

Rust client for the Bitcoin Core daemon's JSON-RPC API.

This crate provides:

- (deprecated) A  blocking client intended for integration testing (`client-sync`).
- An async client intended for production (`client-async`).

## Features

- `client-sync`: Deprecated blocking JSON-RPC client under the `client_sync` module.

```rust,no_run
use corepc_client::{Auth, Client};

let client = Client::new_with_auth("http://127.0.0.1:8332", Auth::None)?;
# Ok::<(), corepc_client::Error>(())
```

## Minimum Supported Rust Version (MSRV)

This library should always compile with any combination of features on **Rust 1.75.0**.

## Licensing

The code in this project is licensed under the [Creative Commons CC0 1.0 Universal license](LICENSE).
We use the [SPDX license list](https://spdx.org/licenses/) and [SPDX IDs](https://spdx.dev/ids/).
