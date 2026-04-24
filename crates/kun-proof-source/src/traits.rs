use async_trait::async_trait;

use kun_core::{AnyHeader, Hash, PosterityHeader};

use crate::error::Result;

/// Data-plane trait the proof algorithms depend on.
///
/// Every method is async — production impls hit the network — but each is
/// independently cacheable. Implementors SHOULD cache by block hash to keep
/// repeated selected-parent walks in a PoChM construction cheap.
#[async_trait]
pub trait ProofSource: Send + Sync {
    async fn get_header(&self, h: Hash) -> Result<AnyHeader>;

    /// Posterity header for `b`: the earliest posterity block whose past
    /// contains `b`. Current era: coincides with the pruning point.
    async fn posterity_of(&self, b: Hash) -> Result<PosterityHeader>;

    /// `posterity_depth(b)` — number of selected-chain hops from
    /// `posterity(b)` down to `b`. Zero iff `b` is itself a posterity.
    async fn posterity_depth(&self, b: Hash) -> Result<u32>;

    /// `parent(b, n)` — nth selected-parent of `b`.
    async fn chain_ancestor(&self, b: Hash, n: u64) -> Result<AnyHeader>;

    /// Return `k` and the ordered ancestors `[parent(b, 2^0), parent(b, 2^1),
    /// ..., parent(b, 2^(k-1))]`, with `k` determined per KIP-6 (least int
    /// s.t. `parent(b, 2^k) ∈ Past(next_posterity(b))`).
    ///
    /// Pre-fork implementations may return
    /// [`ProofSourceError::PreFork`](crate::ProofSourceError::PreFork).
    async fn chain_ancestors_pow2(&self, b: Hash) -> Result<(u8, Vec<Hash>)>;

    /// Block that accepted `tx_id` (if found in the pruning window).
    async fn accepting_block_of(&self, tx_id: Hash) -> Result<AnyHeader>;

    /// Accepted-transaction list for `b`, ordered as stored in the ATMR.
    async fn accepted_txs_of(&self, b: Hash) -> Result<Vec<Hash>>;

    /// Full diamond-DAG chain for `b`: ordered selected-chain headers from
    /// `posterity(b)` DOWN to `b` (inclusive of both endpoints).
    async fn diamond_chain(&self, b: Hash) -> Result<Vec<AnyHeader>>;

    /// Fetch the current selected chain from the node's pruning point
    /// forward to the latest selected-chain tip. Returns (chosen_target,
    /// chain_headers). The chosen target is a block `safety_depth` steps
    /// back from the very latest chain entry — deep enough to be immune to
    /// short-range reorgs between sampling and verification. Default
    /// safety_depth is 100 blocks (~100 s at 10 BPS, comfortably past the
    /// reorg horizon).
    async fn current_selected_chain(&self, safety_depth: u32) -> Result<(Hash, Vec<AnyHeader>)>;
}
