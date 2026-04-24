use thiserror::Error;

use kun_core::Hash;

#[derive(Debug, Error)]
pub enum ProofSourceError {
    #[error("block not found: {0}")]
    BlockNotFound(Hash),

    #[error("block {0} is not on the selected chain (Kaspa has parallel tips; only one is a chain block at any moment — use `kun dag info` and pick the `sink`)")]
    NotOnSelectedChain(Hash),

    #[error("block {0} is older than the pruning window; cannot construct proof from a non-archival node")]
    BlockPruned(Hash),

    #[error("block {0} has no selected parent (genesis or malformed header)")]
    NoSelectedParent(Hash),

    #[error("posterity for block {0} is not yet available (block is newer than current pruning point)")]
    PosterityNotYet(Hash),

    #[error("tx not found in any accepted-chain block: {0}")]
    TxNotFound(Hash),

    #[error("operation not supported pre-fork (KIP-6 required): {0}")]
    PreFork(&'static str),

    #[error(transparent)]
    Rpc(#[from] kun_rpc::RpcError),
}

pub type Result<T> = std::result::Result<T, ProofSourceError>;
