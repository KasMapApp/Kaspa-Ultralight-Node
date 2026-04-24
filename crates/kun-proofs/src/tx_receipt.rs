//! Transaction-receipt proofs (txR). KIP-6 §"PoP and validation receipt".
//!
//! Lands in v0.2 alongside `accepted_txs_of` RPC support.

use kun_core::{Hash, PosterityChain, TxReceipt, VerifyError};

use kun_proof_source::ProofSource;

pub async fn generate_tx_receipt(_src: &dyn ProofSource, tx_id: Hash) -> Result<TxReceipt, GenerateError> {
    Err(GenerateError::NotYetImplemented { tx_id })
}

pub fn verify_tx_receipt(_r: &TxReceipt, _trusted: &PosterityChain) -> Result<(), VerifyError> {
    Err(VerifyError::PreForkHeader)
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("tx receipts land in v0.2 (after accepted_txs_of RPC support); tx={tx_id}")]
    NotYetImplemented { tx_id: Hash },

    #[error(transparent)]
    Source(#[from] kun_proof_source::ProofSourceError),
}
