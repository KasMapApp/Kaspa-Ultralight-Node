//! [`MockProofSource`] — in-memory scripted DAG for unit tests.
//!
//! Lets `kun-proofs` tests build a tiny selected-chain + posterity set
//! without touching the network.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use kun_core::{AnyHeader, Hash, HashScheme, PosterityHeader};

use crate::error::{ProofSourceError, Result};
use crate::traits::ProofSource;

pub struct MockProofSource {
    /// Map block-hash → header.
    headers: RwLock<HashMap<Hash, AnyHeader>>,
    /// Chain-ordered list of selected-chain block hashes (genesis first).
    chain: RwLock<Vec<Hash>>,
    /// Set of posterity hashes (subset of `chain`).
    posterities: RwLock<Vec<PosterityHeader>>,
    /// Per-block accepted transaction ids.
    accepted: RwLock<HashMap<Hash, Vec<Hash>>>,
    /// Hash function used for the `.hash()` computed inside insertion.
    hs: Arc<dyn HashScheme>,
}

impl MockProofSource {
    pub fn new(hs: Arc<dyn HashScheme>) -> Self {
        Self {
            headers: RwLock::new(HashMap::new()),
            chain: RwLock::new(Vec::new()),
            posterities: RwLock::new(Vec::new()),
            accepted: RwLock::new(HashMap::new()),
            hs,
        }
    }

    pub fn hs(&self) -> &dyn HashScheme {
        &*self.hs
    }

    /// Push a header onto the selected chain, returning its computed hash.
    pub fn push_chain(&self, header: AnyHeader) -> Hash {
        let hash = header.hash(&*self.hs);
        self.headers.write().insert(hash, header);
        self.chain.write().push(hash);
        hash
    }

    /// Mark an already-pushed chain block as a posterity.
    pub fn mark_posterity(&self, hash: Hash, chain_index: u64) {
        let hdr = self.headers.read().get(&hash).cloned().expect("header must be pushed first");
        self.posterities.write().push(PosterityHeader { header: hdr, posterity_depth3: hash, chain_index });
    }

    pub fn set_accepted(&self, block: Hash, txs: Vec<Hash>) {
        self.accepted.write().insert(block, txs);
    }

    fn chain_index_of(&self, h: Hash) -> Option<usize> {
        self.chain.read().iter().position(|x| *x == h)
    }
}

#[async_trait]
impl ProofSource for MockProofSource {
    async fn get_header(&self, h: Hash) -> Result<AnyHeader> {
        self.headers.read().get(&h).cloned().ok_or(ProofSourceError::BlockNotFound(h))
    }

    async fn posterity_of(&self, b: Hash) -> Result<PosterityHeader> {
        // Production semantics: anchor is OLDER than b (it's the past pruning
        // point embedded in b's header). Find the latest posterity whose
        // chain index <= b's index.
        let idx = self.chain_index_of(b).ok_or(ProofSourceError::BlockNotFound(b))?;
        let posterities = self.posterities.read();
        let mut best: Option<PosterityHeader> = None;
        let mut best_pi: Option<usize> = None;
        for p in posterities.iter() {
            let p_hash = p.header.hash(&*self.hs);
            if let Some(pi) = self.chain_index_of(p_hash) {
                if pi <= idx && (best_pi.is_none() || pi > best_pi.unwrap()) {
                    best = Some(p.clone());
                    best_pi = Some(pi);
                }
            }
        }
        best.ok_or(ProofSourceError::PosterityNotYet(b))
    }

    async fn posterity_depth(&self, b: Hash) -> Result<u32> {
        let p = self.posterity_of(b).await?;
        let p_hash = p.header.hash(&*self.hs);
        let bi = self.chain_index_of(b).ok_or(ProofSourceError::BlockNotFound(b))?;
        let pi = self.chain_index_of(p_hash).ok_or(ProofSourceError::BlockNotFound(p_hash))?;
        Ok((bi - pi) as u32)
    }

    async fn chain_ancestor(&self, b: Hash, n: u64) -> Result<AnyHeader> {
        let idx = self.chain_index_of(b).ok_or(ProofSourceError::BlockNotFound(b))?;
        if (n as usize) > idx {
            return Err(ProofSourceError::NoSelectedParent(b));
        }
        let target = self.chain.read()[idx - n as usize];
        self.get_header(target).await
    }

    async fn chain_ancestors_pow2(&self, _b: Hash) -> Result<(u8, Vec<Hash>)> {
        Err(ProofSourceError::PreFork("mock does not simulate PMR in v0.1"))
    }

    async fn accepting_block_of(&self, tx_id: Hash) -> Result<AnyHeader> {
        // Drop the read guard before awaiting — parking_lot's guards aren't
        // `Send`, and async `Send` futures can't hold them across await points.
        let owning_block: Option<Hash> = {
            let guard = self.accepted.read();
            guard.iter().find_map(|(b, txs)| if txs.contains(&tx_id) { Some(*b) } else { None })
        };
        match owning_block {
            Some(b) => self.get_header(b).await,
            None => Err(ProofSourceError::TxNotFound(tx_id)),
        }
    }

    async fn accepted_txs_of(&self, b: Hash) -> Result<Vec<Hash>> {
        self.accepted.read().get(&b).cloned().ok_or(ProofSourceError::BlockNotFound(b))
    }

    async fn diamond_chain(&self, b: Hash) -> Result<Vec<AnyHeader>> {
        let bi = self.chain_index_of(b).ok_or(ProofSourceError::BlockNotFound(b))?;
        let p = self.posterity_of(b).await?;
        let p_hash = p.header.hash(&*self.hs);
        let pi = self.chain_index_of(p_hash).ok_or(ProofSourceError::BlockNotFound(p_hash))?;
        if pi > bi {
            return Err(ProofSourceError::PosterityNotYet(b));
        }
        // Anchor (older, index pi) → target (newer, index bi) — no reverse.
        let chain = self.chain.read().clone();
        let segment: Vec<Hash> = chain[pi..=bi].to_vec();
        let mut out = Vec::with_capacity(segment.len());
        for h in segment {
            out.push(self.get_header(h).await?);
        }
        Ok(out)
    }

    async fn current_selected_chain(&self, _safety_depth: u32) -> Result<(Hash, Vec<AnyHeader>)> {
        Err(ProofSourceError::PreFork("MockProofSource does not simulate a live tip"))
    }
}
