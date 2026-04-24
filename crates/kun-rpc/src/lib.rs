//! `kun-rpc` — wRPC client wrapper for KUN.
//!
//! Exposes [`KunRpc`], a thin handle over [`kaspa_wrpc_client::KaspaRpcClient`]
//! that returns KUN's portable types (never rusty-kaspa ones). Conversion
//! lives in [`converters`].
//!
//! The rest of the KUN workspace depends ONLY on this crate (and `kun-core`)
//! to talk to a node; the concrete rusty-kaspa dependency is confined here.

pub mod client;
pub mod converters;
pub mod error;

pub use client::{DagInfo, KunRpc, KunRpcConfig, ServerInfo, VirtualChain};
pub use error::{Result, RpcError};

// Re-export rusty-kaspa types that appear on our public API so callers don't
// need a direct dep on `kaspa-consensus-core`.
pub use kaspa_consensus_core::network::{NetworkId, NetworkType};
pub use kaspa_wrpc_client::WrpcEncoding;
