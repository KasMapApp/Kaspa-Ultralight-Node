//! Block header types.
//!
//! [`BlockHeader`] is the pre-KIP-6 (v1) header. [`BlockHeaderV2`] adds the
//! 32-byte PMR field from KIP-6. [`AnyHeader`] is the enum KUN uses
//! everywhere so a single proof can mix pre- and post-fork headers — useful
//! during the transition period and for archival proofs over old chain
//! segments.
//!
//! The borsh layout is stable within a major format version; our own
//! `hash()` method always re-derives from the serialized preimage so we
//! never trust an externally-claimed hash.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::hash::{Hash, HashScheme};

/// Domain tag for the header hash preimage. Placeholder — reconcile with
/// rusty-kaspa's actual header-hash convention in Phase 0.
pub const HEADER_HASH_TAG: &[u8] = b"kun:header:v1";

/// Pre-KIP-6 block header. 248 bytes on the wire when all `parents_by_level`
/// levels are empty after GHOSTDAG level 0; headers with many superblock
/// levels are correspondingly larger.
///
/// `cached_hash` holds the upstream rusty-kaspa header hash when the header
/// came in via RPC. We prefer that over re-deriving because KUN's internal
/// domain-separated hashing does not match rusty-kaspa's (reconciling them
/// is a Phase 0 task — until then the cached hash is the authority when
/// present). Tests that build synthetic headers leave it `None` and fall
/// back to KUN's own hashing.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct BlockHeader {
    pub cached_hash: Option<crate::hash::Hash>,
    pub version: u16,
    /// Parents plural, per superblock level. `parents_by_level[0]` is the
    /// GHOSTDAG parents; `[1..]` are parents inherited from higher
    /// superblock levels. The selected parent is always
    /// `parents_by_level[0][0]` post-disqualification resolution.
    pub parents_by_level: Vec<Vec<Hash>>,
    /// Merkle root of transactions included in THIS block.
    pub hash_merkle_root: Hash,
    /// Accepted-Transaction Merkle Root: root of the Merkle tree of all
    /// transactions accepted by this block's merge set, minus GHOSTDAG
    /// conflicts. KIP-6 §"ATMR".
    pub accepted_id_merkle_root: Hash,
    /// MuHash commitment to the UTXO set after applying this block.
    pub utxo_commitment: Hash,
    pub timestamp: u64,
    pub bits: u32,
    pub nonce: u64,
    pub daa_score: u64,
    /// Blue accumulated work (U192 BE). GHOSTDAG selection rule compares
    /// this value across siblings.
    pub blue_work: [u8; 24],
    pub blue_score: u64,
    /// Pointer to the depth-3 posterity header (currently ≡ pruning point).
    /// KIP-6 §"Posterity Headers".
    pub pruning_point: Hash,
}

/// Post-KIP-6 header, adds a 32-byte PMR (PoChM Merkle Root) field.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct BlockHeaderV2 {
    pub v1: BlockHeader,
    pub pmr: Hash,
}

/// Era-generic header. Proofs use this so they can span the fork boundary.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum AnyHeader {
    V1(BlockHeader),
    V2(BlockHeaderV2),
}

impl AnyHeader {
    pub fn core(&self) -> &BlockHeader {
        match self {
            Self::V1(h) => h,
            Self::V2(h) => &h.v1,
        }
    }

    /// PMR is only defined in V2. Pre-fork callers should expect `None`.
    pub fn pmr(&self) -> Option<Hash> {
        match self {
            Self::V1(_) => None,
            Self::V2(h) => Some(h.pmr),
        }
    }

    /// Selected parent = `parents_by_level[0][0]`, if present.
    pub fn selected_parent(&self) -> Option<Hash> {
        self.core().parents_by_level.first().and_then(|lvl| lvl.first().copied())
    }

    /// Return this header's hash. Prefers the upstream rusty-kaspa hash if
    /// one is cached (which is true for any header we received over RPC);
    /// otherwise falls back to KUN's own domain-tagged serialization hash.
    ///
    /// The fallback path is not rusty-kaspa-compatible and is only correct
    /// within KUN's own closed world (synthetic test DAGs, simulated
    /// fixtures). Phase 0 item D tracks reconciling the two schemes.
    pub fn hash(&self, hs: &dyn HashScheme) -> Hash {
        if let Some(h) = self.core().cached_hash {
            return h;
        }
        let preimage = borsh::to_vec(self).expect("borsh serialization of AnyHeader must be infallible");
        hs.hash_domain(HEADER_HASH_TAG, &preimage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Blake2bScheme;

    fn make_v1() -> BlockHeader {
        BlockHeader {
            cached_hash: None,
            version: 1,
            parents_by_level: vec![vec![Hash([1; 32]), Hash([2; 32])]],
            hash_merkle_root: Hash([3; 32]),
            accepted_id_merkle_root: Hash([4; 32]),
            utxo_commitment: Hash([5; 32]),
            timestamp: 1_234_567_890,
            bits: 0x1d00_ffff,
            nonce: 0xdead_beef_1234_5678,
            daa_score: 87_412_559,
            blue_work: [6; 24],
            blue_score: 1_000_000,
            pruning_point: Hash([7; 32]),
        }
    }

    #[test]
    fn v1_borsh_round_trip() {
        let h = AnyHeader::V1(make_v1());
        let bytes = borsh::to_vec(&h).unwrap();
        let h2: AnyHeader = borsh::from_slice(&bytes).unwrap();
        assert_eq!(h, h2);
    }

    #[test]
    fn v2_borsh_round_trip_preserves_pmr() {
        let h = AnyHeader::V2(BlockHeaderV2 { v1: make_v1(), pmr: Hash([9; 32]) });
        let bytes = borsh::to_vec(&h).unwrap();
        let h2: AnyHeader = borsh::from_slice(&bytes).unwrap();
        assert_eq!(h, h2);
        assert_eq!(h2.pmr(), Some(Hash([9; 32])));
    }

    #[test]
    fn v1_and_v2_with_identical_core_hash_differently() {
        let v1 = AnyHeader::V1(make_v1());
        let v2 = AnyHeader::V2(BlockHeaderV2 { v1: make_v1(), pmr: Hash::ZERO });
        let s = Blake2bScheme;
        assert_ne!(v1.hash(&s), v2.hash(&s));
    }

    #[test]
    fn selected_parent_is_first_parent_of_level_zero() {
        let h = AnyHeader::V1(make_v1());
        assert_eq!(h.selected_parent(), Some(Hash([1; 32])));
    }

    #[test]
    fn selected_parent_none_when_level_zero_empty() {
        let mut core = make_v1();
        core.parents_by_level = vec![];
        let h = AnyHeader::V1(core);
        assert_eq!(h.selected_parent(), None);
    }
}
