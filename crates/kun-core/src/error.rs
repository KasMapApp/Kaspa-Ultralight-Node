//! Shared error types.

use thiserror::Error;

use crate::hash::Hash;
use crate::merkle::MerkleError;
use crate::posterity::PosterityError;

/// Verification errors. Exhaustive enumeration per KIP-6 and the three-case
/// PoChM verifier; allows the CLI and any library consumer to report a
/// precise reason for invalidation.
#[derive(Debug, Error)]
pub enum VerifyError {
    #[error("anchor {0} is not in the trusted posterity chain")]
    UntrustedAnchor(Hash),

    #[error("empty proof is malformed: target must equal anchor and there must be zero steps")]
    MalformedEmptyProof,

    #[error("step count {got} does not match popcount(posterity_depth)={expected}")]
    StepCountMismatch { expected: u32, got: usize },

    #[error("step `i` values must be strictly decreasing (MSB first)")]
    NonMonotonicSteps,

    #[error("posterity_depth overflow while summing 2^i terms")]
    DepthOverflow,

    #[error("sum of 2^i over steps does not equal posterity_depth")]
    DepthMismatch,

    #[error("merkle-proof leaf does not match the intermediate block we expected")]
    LeafMismatch,

    #[error("last step should not carry a next_header")]
    TrailingHeader,

    #[error("intermediate step is missing its next_header")]
    MissingIntermediateHeader,

    #[error("PMR absent on V1 header where V2 was required (post-fork anchor expected)")]
    PreForkHeader,

    #[error("diamond proof: chain entry's selected parent does not match the next header's hash")]
    SelectedParentMismatch,

    #[error("accepted-tx merkle proof root does not match header's accepted_id_merkle_root")]
    AtmrMismatch,

    #[error("merge set size {0} exceeds the consensus cap of 180")]
    MergeSetSizeExceeded(usize),

    #[error("gap-spanning proof across a 24h+ pruning anomaly at index {start}..{end}")]
    GapSpanningProof { start: u64, end: u64 },

    #[error("unsupported proof format version {0}")]
    UnsupportedVersion(u8),

    #[error("proof bytes truncated or malformed")]
    Truncated,

    #[error(transparent)]
    Merkle(#[from] MerkleError),

    #[error(transparent)]
    Posterity(#[from] PosterityError),

    #[error(transparent)]
    Io(#[from] borsh::io::Error),
}
