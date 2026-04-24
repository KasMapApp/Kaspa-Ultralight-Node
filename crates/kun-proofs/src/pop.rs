//! Proof of Publication (PoP). KIP-6 §"PoP and validation receipt".
//!
//! Lands in v0.2.

use kun_core::{Hash, PoP, PosterityChain, VerifyError};

use kun_proof_source::ProofSource;

pub async fn generate_pop(_src: &dyn ProofSource, tx_id: Hash) -> Result<PoP, GenerateError> {
    Err(GenerateError::NotYetImplemented { tx_id })
}

pub fn verify_pop(_proof: &PoP, _trusted: &PosterityChain) -> Result<(), VerifyError> {
    Err(VerifyError::PreForkHeader)
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("PoPs land in v0.2 (after block_tx_proof RPC support); tx={tx_id}")]
    NotYetImplemented { tx_id: Hash },

    #[error(transparent)]
    Source(#[from] kun_proof_source::ProofSourceError),
}
