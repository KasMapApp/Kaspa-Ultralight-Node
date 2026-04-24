//! `kun-proof-source` — the data-plane abstraction for proof construction.
//!
//! Proof algorithms (in `kun-proofs`) are backend-agnostic: they talk to a
//! [`ProofSource`] and never touch RPC directly. This crate provides:
//!
//! - [`ProofSource`]: the async trait every backend implements
//! - [`RpcProofSource`]: the production backend; wraps `kun-rpc::KunRpc`
//!   with an LRU header cache so repeated selected-parent walks don't hammer
//!   the network
//! - [`MockProofSource`]: in-memory scripted DAG for unit tests

pub mod error;
pub mod mock;
pub mod rpc;
pub mod traits;

pub use error::{ProofSourceError, Result};
pub use mock::MockProofSource;
pub use rpc::RpcProofSource;
pub use traits::ProofSource;
