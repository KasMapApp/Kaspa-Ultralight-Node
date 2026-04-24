//! `RpcProofSource` — production [`ProofSource`] backed by `kun-rpc`.
//!
//! Wraps a [`KunRpc`] handle with a bounded LRU cache keyed by block hash.
//! The selected-parent walks inside PoChM construction can revisit the same
//! block dozens of times; caching cuts that to a single RPC round-trip.

use std::num::NonZeroUsize;
use std::sync::Arc;

use async_trait::async_trait;
use lru::LruCache;
use parking_lot::Mutex;
use tracing::debug;

use kun_core::{AnyHeader, Hash, HashScheme, PosterityHeader};
use kun_rpc::KunRpc;

use crate::error::{ProofSourceError, Result};
use crate::traits::ProofSource;

const DEFAULT_HEADER_CACHE: usize = 4096;

pub struct RpcProofSource {
    rpc: KunRpc,
    hs: Arc<dyn HashScheme>,
    cache: Mutex<LruCache<Hash, AnyHeader>>,
}

impl RpcProofSource {
    pub fn new(rpc: KunRpc, hs: Arc<dyn HashScheme>) -> Self {
        Self {
            rpc,
            hs,
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(DEFAULT_HEADER_CACHE).expect("nonzero"))),
        }
    }

    pub fn with_cache_size(rpc: KunRpc, hs: Arc<dyn HashScheme>, size: usize) -> Self {
        let nz = NonZeroUsize::new(size.max(1)).expect("nonzero");
        Self { rpc, hs, cache: Mutex::new(LruCache::new(nz)) }
    }

    fn cache_get(&self, h: Hash) -> Option<AnyHeader> {
        self.cache.lock().get(&h).cloned()
    }

    fn cache_put(&self, h: Hash, v: AnyHeader) {
        self.cache.lock().put(h, v);
    }

    async fn header_cached(&self, h: Hash) -> Result<AnyHeader> {
        if let Some(v) = self.cache_get(h) {
            return Ok(v);
        }
        let v = self.rpc.get_block_header(h).await.map_err(ProofSourceError::from)?;
        self.cache_put(h, v.clone());
        Ok(v)
    }

    async fn selected_parent(&self, h: Hash) -> Result<Hash> {
        let hdr = self.header_cached(h).await?;
        hdr.selected_parent().ok_or(ProofSourceError::NoSelectedParent(h))
    }
}

#[async_trait]
impl ProofSource for RpcProofSource {
    async fn get_header(&self, h: Hash) -> Result<AnyHeader> {
        self.header_cached(h).await
    }

    async fn posterity_of(&self, b: Hash) -> Result<PosterityHeader> {
        // Current era: posterity ≡ pruning point pointer on the header.
        let hdr = self.header_cached(b).await?;
        let pp_hash = hdr.core().pruning_point;
        if pp_hash == Hash::ZERO {
            return Err(ProofSourceError::PosterityNotYet(b));
        }
        let pp_header = self.header_cached(pp_hash).await?;
        Ok(PosterityHeader {
            header: pp_header,
            posterity_depth3: pp_hash, // conservative: re-use the same hash until decoupled
            chain_index: 0,            // unknown from RPC alone; real index is filled in by sync
        })
    }

    async fn posterity_depth(&self, b: Hash) -> Result<u32> {
        let pp = self.posterity_of(b).await?.header.hash(&*self.hs);
        let mut cur = b;
        let mut d: u32 = 0;
        while cur != pp {
            cur = self.selected_parent(cur).await?;
            d = d
                .checked_add(1)
                .ok_or_else(|| ProofSourceError::Rpc(kun_rpc::RpcError::UnexpectedResponse("depth overflow".into())))?;
            // Defensive bound: posterity_depth at 24h density and 10 BPS ≤ 86400.
            if d > 100_000 {
                return Err(ProofSourceError::Rpc(kun_rpc::RpcError::UnexpectedResponse(
                    "exceeded sane posterity_depth bound; chain likely malformed".into(),
                )));
            }
        }
        Ok(d)
    }

    async fn chain_ancestor(&self, b: Hash, n: u64) -> Result<AnyHeader> {
        let mut cur = b;
        for _ in 0..n {
            cur = self.selected_parent(cur).await?;
        }
        self.header_cached(cur).await
    }

    async fn chain_ancestors_pow2(&self, _b: Hash) -> Result<(u8, Vec<Hash>)> {
        // Requires PMR on headers (post-KIP-6) or a simulated-PMR cache
        // (v0.2 feature). Until then, diamond is the only supported proof.
        Err(ProofSourceError::PreFork("chain_ancestors_pow2 requires KIP-6 PMR or simulation"))
    }

    async fn accepting_block_of(&self, tx_id: Hash) -> Result<AnyHeader> {
        // Phase 0 RPC discovery: confirm whether there's a direct RPC for
        // tx→accepting-block lookup. Until confirmed, we fail loudly rather
        // than iterate `get_virtual_chain_from_block` unbounded.
        Err(ProofSourceError::PreFork(
            "accepting_block_of needs Phase-0-verified RPC (get_utxo_return_address or direct scan)",
        ))
        .map(|_: ProofSourceError| -> AnyHeader { unreachable!() })
        .map_err(|_: ProofSourceError| ProofSourceError::TxNotFound(tx_id))
    }

    async fn accepted_txs_of(&self, b: Hash) -> Result<Vec<Hash>> {
        // Request the full block with transactions, read the verbose data's
        // accepted-tx list. RPC method: `get_block(hash, include_transactions=true)`.
        // For v0.1 we don't need this (diamond proofs don't); placeholder.
        debug!(%b, "accepted_txs_of not implemented in v0.1 — tx-receipts land in v0.2");
        Err(ProofSourceError::PreFork("accepted_txs_of lands with tx-receipt support in v0.2"))
    }

    async fn diamond_chain(&self, b: Hash) -> Result<Vec<AnyHeader>> {
        use std::time::Instant;
        use tracing::info;

        // 1. Anchor = posterity(b). In the current era this is the
        //    `pruning_point` field on b's header.
        let anchor = self.posterity_of(b).await?;
        let anchor_hash = anchor.header.hash(&*self.hs);

        let t0 = Instant::now();
        info!(target = %b, anchor = %anchor_hash, "fetching selected chain anchor → target via get_virtual_chain_from_block");

        // 2. Paginated call to `get_virtual_chain_from_block` to gather the
        //    TRUE selected chain from the anchor forward. The node caps each
        //    response at `mergeset_size_limit * 10` (≈ 2,480 on 10 BPS),
        //    and `added` is returned in forward chronological order starting
        //    AFTER start_hash.
        let mut selected_chain_hashes: Vec<Hash> = Vec::new();
        let mut cursor = anchor_hash;
        let mut batches = 0u32;
        let mut last_log = t0;
        loop {
            let vc = self.rpc.get_virtual_chain_from_block(cursor, false).await.map_err(ProofSourceError::from)?;
            batches += 1;
            if vc.added.is_empty() {
                // No more forward progress. Either we're at virtual, or the
                // node reorg'd the anchor out; bail with what we have.
                break;
            }
            // Check if target is in this batch.
            let mut hit_target = false;
            for h in &vc.added {
                selected_chain_hashes.push(*h);
                if *h == b {
                    hit_target = true;
                    break;
                }
            }

            if last_log.elapsed().as_secs() >= 5 {
                info!(
                    batches,
                    hashes = selected_chain_hashes.len(),
                    elapsed_ms = t0.elapsed().as_millis() as u64,
                    "paginated selected-chain fetch progress"
                );
                last_log = Instant::now();
            }
            if hit_target {
                break;
            }
            cursor = *vc.added.last().expect("non-empty checked above");

            if selected_chain_hashes.len() > 1_000_000 {
                return Err(ProofSourceError::Rpc(kun_rpc::RpcError::UnexpectedResponse(
                    "selected-chain fetch exceeded sane length".into(),
                )));
            }
        }
        info!(
            batches,
            hashes = selected_chain_hashes.len(),
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "selected-chain hashes assembled"
        );

        // 3. The RPC returns blocks starting AFTER start_hash, so the anchor
        //    itself is not in `selected_chain_hashes`. If the target wasn't
        //    seen during the full walk up to virtual, it's not a selected-
        //    chain block (parallel tip / merged-only block). Fail loudly.
        let target_idx = selected_chain_hashes
            .iter()
            .position(|h| *h == b)
            .ok_or(ProofSourceError::NotOnSelectedChain(b))?;
        selected_chain_hashes.truncate(target_idx + 1);

        // 4. Fetch each header. LRU cache absorbs repeat calls when a later
        //    diamond includes the same blocks. First-run cost is
        //    N `get_block` round-trips — a batched `get_blocks` is the v0.2
        //    optimization.
        let t_headers = Instant::now();
        let mut headers: Vec<AnyHeader> = Vec::with_capacity(selected_chain_hashes.len() + 1);
        headers.push(anchor.header);
        let mut fetched = 0usize;
        let mut last_log = t_headers;
        for h in &selected_chain_hashes {
            headers.push(self.header_cached(*h).await?);
            fetched += 1;
            if fetched % 5000 == 0 || last_log.elapsed().as_secs() >= 5 {
                let rate = fetched as f64 / t_headers.elapsed().as_secs_f64().max(0.001);
                info!(
                    fetched,
                    elapsed_ms = t_headers.elapsed().as_millis() as u64,
                    blocks_per_sec = rate as u64,
                    "header fetch progress"
                );
                last_log = Instant::now();
            }
        }

        info!(
            total = headers.len(),
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "diamond chain complete"
        );
        // headers[0] = anchor, headers[last] = target → matches DiamondProof convention.
        Ok(headers)
    }

    async fn current_selected_chain(&self, safety_depth: u32) -> Result<(Hash, Vec<AnyHeader>)> {
        use std::time::Instant;
        use tracing::info;

        // 1. Anchor = node's current pruning point.
        let dag = self.rpc.get_block_dag_info().await.map_err(ProofSourceError::from)?;
        let anchor_hash = dag.pruning_point;
        let anchor_header = self.header_cached(anchor_hash).await?;

        let t0 = Instant::now();
        info!(anchor = %anchor_hash, "fetching current selected chain anchor → virtual");

        // 2. Walk forward to virtual (empty response signals we're at the tip).
        let mut selected_chain_hashes: Vec<Hash> = Vec::new();
        let mut cursor = anchor_hash;
        let mut batches = 0u32;
        let mut last_log = t0;
        loop {
            let vc = self.rpc.get_virtual_chain_from_block(cursor, false).await.map_err(ProofSourceError::from)?;
            batches += 1;
            if vc.added.is_empty() {
                break;
            }
            selected_chain_hashes.extend_from_slice(&vc.added);
            if last_log.elapsed().as_secs() >= 5 {
                info!(
                    batches,
                    hashes = selected_chain_hashes.len(),
                    elapsed_ms = t0.elapsed().as_millis() as u64,
                    "paginated selected-chain fetch progress"
                );
                last_log = Instant::now();
            }
            cursor = *vc.added.last().expect("non-empty checked above");
            if selected_chain_hashes.len() > 1_000_000 {
                return Err(ProofSourceError::Rpc(kun_rpc::RpcError::UnexpectedResponse(
                    "selected-chain fetch exceeded sane length".into(),
                )));
            }
        }

        info!(
            batches,
            hashes = selected_chain_hashes.len(),
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "selected-chain hashes assembled"
        );

        // 3. Pick the target `safety_depth` back from the latest chain entry.
        if selected_chain_hashes.is_empty() {
            return Err(ProofSourceError::PosterityNotYet(anchor_hash));
        }
        let target_idx = selected_chain_hashes
            .len()
            .saturating_sub(1 + safety_depth as usize)
            .min(selected_chain_hashes.len().saturating_sub(1));
        let target_hash = selected_chain_hashes[target_idx];
        selected_chain_hashes.truncate(target_idx + 1);
        info!(
            target = %target_hash,
            target_depth_from_tip = safety_depth,
            chain_len = selected_chain_hashes.len() + 1,
            "chose auto target (safety-depth from selected tip)"
        );

        // 4. Fetch each header.
        let t_headers = Instant::now();
        let mut headers: Vec<AnyHeader> = Vec::with_capacity(selected_chain_hashes.len() + 1);
        headers.push(anchor_header);
        let mut fetched = 0usize;
        let mut last_log = t_headers;
        for h in &selected_chain_hashes {
            headers.push(self.header_cached(*h).await?);
            fetched += 1;
            if fetched % 5000 == 0 || last_log.elapsed().as_secs() >= 5 {
                let rate = fetched as f64 / t_headers.elapsed().as_secs_f64().max(0.001);
                info!(
                    fetched,
                    elapsed_ms = t_headers.elapsed().as_millis() as u64,
                    blocks_per_sec = rate as u64,
                    "header fetch progress"
                );
                last_log = Instant::now();
            }
        }
        info!(total = headers.len(), elapsed_ms = t0.elapsed().as_millis() as u64, "auto-target chain complete");
        Ok((target_hash, headers))
    }
}
