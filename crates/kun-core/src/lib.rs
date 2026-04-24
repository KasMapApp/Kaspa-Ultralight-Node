//! `kun-core` — shared types, primitives, and the hash-scheme abstraction
//! used across all KUN crates.
//!
//! This crate has zero rusty-kaspa dependencies so it compiles everywhere,
//! including `wasm32-unknown-unknown`. Conversion to/from rusty-kaspa
//! consensus types lives in `kun-rpc::converters` to keep the upstream
//! dependency confined to the networking crate.

pub mod error;
pub mod hash;
pub mod header;
pub mod merkle;
pub mod posterity;
pub mod proof;

pub use error::VerifyError;
pub use hash::{scheme_for, Blake2bScheme, Blake3Scheme, Hash, HashEra, HashScheme, HASH_SIZE};
pub use header::{AnyHeader, BlockHeader, BlockHeaderV2, HEADER_HASH_TAG};
pub use merkle::{
    merkle_proof, merkle_root, verify_merkle_proof, MerkleError, MerkleProof, ATMR_LEAF_TAG, ATMR_NODE_TAG,
    PMR_LEAF_TAG, PMR_NODE_TAG,
};
pub use posterity::{PosterityChain, PosterityError, PosterityHeader};
pub use proof::{
    DiamondProof, PoChM, PoChMStep, PoP, ProofEnvelope, ProofKind, ProofPayload, TxReceipt, PROOF_FORMAT_V1,
};

/// Merge-set size limit (consensus parameter).
pub const MERGE_SET_SIZE_LIMIT: usize = 180;

/// Default posterity density (seconds between posterity headers). 24 hours
/// in the current era; KIP-6's "Posterity Header Density" optional follow-up
/// reduces this to 3600.
pub const POSTERITY_DENSITY_SECS: u64 = 86_400;
