//! KIP-6 Proof of Chain Membership (PoChM).
//!
//! Implements the binary-decomposition algorithm from KIP-6, lines 102–117.
//! In v0.1 this module is a skeleton: the [`generate_pochm`] function
//! requires PMR-bearing headers (V2) and therefore fails for all current
//! mainnet blocks. It lands in full in v0.2 behind a simulation shim.

use kun_core::{Hash, PoChM, PosterityChain, VerifyError};

use kun_proof_source::ProofSource;

/// Generate a PoChM for `target`. Pre-fork this always returns `PreFork`.
pub async fn generate_pochm(_src: &dyn ProofSource, target: Hash) -> Result<PoChM, GenerateError> {
    Err(GenerateError::PreFork { target })
}

/// Verify a PoChM against a trusted posterity chain. The three-case
/// verifier from KIP-6 (§"Our Proposal"):
///
/// - Case A: `posterity_depth = 2^i` → a single Merkle proof into the
///   anchor's PMR
/// - Case B: `posterity_depth = 2^i + 2^j` (i > j) → two steps, one Merkle
///   proof each, plus one intermediate header
/// - Case C (general): `popcount(posterity_depth)` steps, each step a
///   Merkle proof, every non-last step also carrying an intermediate
///   V2 header that the next step's PMR comes from
///
/// The loop below handles all three cases uniformly.
pub fn verify_pochm(_proof: &PoChM, _trusted: &PosterityChain) -> Result<(), VerifyError> {
    // Full implementation lands in v0.2; the shape of the algorithm is
    // documented in the plan file and mirrored in the `kun-core::PoChMStep`
    // type comments.
    Err(VerifyError::PreForkHeader)
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("PoChM generation requires KIP-6 PMR on headers; target {target} has pre-fork header")]
    PreFork { target: Hash },

    #[error(transparent)]
    Source(#[from] kun_proof_source::ProofSourceError),
}
