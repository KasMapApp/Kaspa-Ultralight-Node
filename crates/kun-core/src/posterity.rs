//! Posterity chain: KUN's trusted anchor set.
//!
//! A [`PosterityHeader`] is a selected-chain block header sampled at the
//! 24-hour posterity cadence. The chain of them is verifiable all the way
//! to genesis via the depth-3 posterity pointer each block carries (which
//! currently coincides with the pruning-point pointer, per KIP-6).
//!
//! A [`PosterityChain`] is the verifier's local view — typically loaded
//! from `~/.kun/posterity.borsh` at startup or bootstrapped out-of-band.
//! Every proof verification takes `&PosterityChain` and refuses to continue
//! if the anchor block isn't already trusted.

use std::collections::{BTreeMap, HashMap};

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::hash::{Hash, HashScheme};
use crate::header::AnyHeader;

/// A trusted posterity-chain entry.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct PosterityHeader {
    pub header: AnyHeader,
    /// The depth-3 posterity pointer this block carries (= `parent(B)`'s
    /// posterity today). When the chain is walked forward, each entry's
    /// `posterity_depth3` must point three tiers ahead.
    pub posterity_depth3: Hash,
    /// Monotonically increasing, one per posterity sample. Genesis = 0.
    pub chain_index: u64,
}

impl PosterityHeader {
    pub fn hash(&self, hs: &dyn HashScheme) -> Hash {
        self.header.hash(hs)
    }
}

/// In-memory lookup structure the verifier consults on every proof.
#[derive(Default, Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct PosterityChain {
    by_hash: HashMap<Hash, PosterityHeader>,
    by_index: BTreeMap<u64, Hash>,
}

impl PosterityChain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a posterity header, asserting it chains forward by exactly one
    /// index. Does NOT verify the depth-3 pointer cryptographically — that's
    /// the bootstrap's job; `PosterityChain` only enforces monotonicity.
    pub fn extend(&mut self, hs: &dyn HashScheme, entry: PosterityHeader) -> Result<(), PosterityError> {
        let next_index = self.by_index.keys().last().map_or(0, |last| last + 1);
        if entry.chain_index != next_index {
            return Err(PosterityError::NonMonotonicIndex { expected: next_index, got: entry.chain_index });
        }
        let hash = entry.hash(hs);
        if self.by_hash.contains_key(&hash) {
            return Err(PosterityError::DuplicateHash(hash));
        }
        self.by_index.insert(entry.chain_index, hash);
        self.by_hash.insert(hash, entry);
        Ok(())
    }

    pub fn contains(&self, h: Hash) -> bool {
        self.by_hash.contains_key(&h)
    }

    pub fn get(&self, h: Hash) -> Option<&PosterityHeader> {
        self.by_hash.get(&h)
    }

    pub fn latest(&self) -> Option<&PosterityHeader> {
        self.by_index.values().next_back().and_then(|h| self.by_hash.get(h))
    }

    pub fn len(&self) -> usize {
        self.by_index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }

    /// Serialize to borsh for on-disk storage (`~/.kun/posterity.borsh` in v0.1).
    pub fn serialize(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("borsh serialization is infallible for PosterityChain")
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(bytes)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PosterityError {
    #[error("posterity chain index non-monotonic: expected {expected}, got {got}")]
    NonMonotonicIndex { expected: u64, got: u64 },
    #[error("duplicate posterity hash: {0}")]
    DuplicateHash(Hash),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Blake2bScheme;
    use crate::header::BlockHeader;

    fn entry(idx: u64, seed: u8) -> PosterityHeader {
        PosterityHeader {
            header: AnyHeader::V1(BlockHeader {
                cached_hash: None,
                version: 1,
                parents_by_level: vec![vec![Hash([seed.wrapping_sub(1); 32])]],
                hash_merkle_root: Hash([seed; 32]),
                accepted_id_merkle_root: Hash([seed; 32]),
                utxo_commitment: Hash([seed; 32]),
                timestamp: idx * 86_400,
                bits: 0,
                nonce: u64::from(seed),
                daa_score: idx * 86_400,
                blue_work: [0; 24],
                blue_score: idx * 10_000,
                pruning_point: Hash([seed.wrapping_sub(3); 32]),
            }),
            posterity_depth3: Hash([seed.wrapping_sub(3); 32]),
            chain_index: idx,
        }
    }

    #[test]
    fn extend_appends_monotonically() {
        let s = Blake2bScheme;
        let mut chain = PosterityChain::new();
        chain.extend(&s, entry(0, 1)).unwrap();
        chain.extend(&s, entry(1, 2)).unwrap();
        chain.extend(&s, entry(2, 3)).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.latest().unwrap().chain_index, 2);
    }

    #[test]
    fn extend_rejects_out_of_order_index() {
        let s = Blake2bScheme;
        let mut chain = PosterityChain::new();
        chain.extend(&s, entry(0, 1)).unwrap();
        assert!(matches!(
            chain.extend(&s, entry(2, 3)),
            Err(PosterityError::NonMonotonicIndex { expected: 1, got: 2 })
        ));
    }

    #[test]
    fn borsh_round_trip() {
        let s = Blake2bScheme;
        let mut chain = PosterityChain::new();
        chain.extend(&s, entry(0, 1)).unwrap();
        chain.extend(&s, entry(1, 2)).unwrap();
        let bytes = chain.serialize();
        let chain2 = PosterityChain::deserialize(&bytes).unwrap();
        assert_eq!(chain.len(), chain2.len());
        assert_eq!(chain.latest().map(|h| h.chain_index), chain2.latest().map(|h| h.chain_index));
    }
}
