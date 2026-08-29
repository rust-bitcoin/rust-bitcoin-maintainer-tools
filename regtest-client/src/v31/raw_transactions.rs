// SPDX-License-Identifier: CC0-1.0

//! Macros for implementing JSON-RPC methods on a client.
//!
//! Specifically this is methods found under the `== Rawtransactions ==` section of the
//! API docs of Bitcoin Core `v31`.
//!
//! All macros require `Client` to be in scope.
//!
//! See or use the `define_jsonrpc_bitreq_client!` macro to define a `Client`.

/// Implements Bitcoin Core JSON-RPC API method `abortprivatebroadcast`.
#[macro_export]
macro_rules! impl_client_v31__abort_private_broadcast {
    () => {
        impl Client {
            pub fn abort_private_broadcast(&self, id: &str) -> Result<AbortPrivateBroadcast> {
                self.call("abortprivatebroadcast", &[id.into()])
            }
        }
    };
}

/// Implements Bitcoin Core JSON-RPC API method `getprivatebroadcastinfo`.
#[macro_export]
macro_rules! impl_client_v31__get_private_broadcast_info {
    () => {
        impl Client {
            pub fn get_private_broadcast_info(&self) -> Result<GetPrivateBroadcastInfo> {
                self.call("getprivatebroadcastinfo", &[])
            }
        }
    };
}
