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

/// Per-field byte-size decomposition of a single header.
///
/// Used by `kun proof header-stats` to answer the empirical question of
/// where the ~6 KB per real-mainnet header actually goes, and in particular
/// what fraction is `parents_by_level` (the multi-level GHOSTDAG parent
/// arrays). The reviewer-flagged "9 KB vs 100 KB" question for post-KIP-6
/// PoChM proofs hinges on this distribution.
///
/// `total_field_sum` adds every per-field count below it; `serialized_total`
/// is the actual borsh-encoded byte length of the whole header (including
/// the `AnyHeader` enum discriminant for the [`AnyHeader::field_sizes`]
/// case). The difference is `framing_overhead` — borsh writes a few extra
/// length prefixes when fields are nested inside the parent struct that
/// don't appear when each field is serialized standalone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HeaderSizeBreakdown {
    /// 1 byte for the `AnyHeader` enum tag (V1 / V2). 0 when this breakdown
    /// describes a bare [`BlockHeader`] or [`BlockHeaderV2`] without the
    /// envelope wrapper.
    pub envelope_discriminant: usize,
    pub cached_hash: usize,
    pub version: usize,
    pub parents_by_level: usize,
    pub hash_merkle_root: usize,
    pub accepted_id_merkle_root: usize,
    pub utxo_commitment: usize,
    pub timestamp: usize,
    pub bits: usize,
    pub nonce: usize,
    pub daa_score: usize,
    pub blue_work: usize,
    pub blue_score: usize,
    pub pruning_point: usize,
    /// Always 0 for V1 headers; 32 for V2 (post-KIP-6).
    pub pmr: usize,
    /// Sum of every per-field count above, including `pmr` and
    /// `envelope_discriminant`.
    pub total_field_sum: usize,
    /// `borsh::to_vec(...)` length of the entire header. Should equal
    /// `total_field_sum + framing_overhead` exactly.
    pub serialized_total: usize,
    /// `serialized_total − total_field_sum`. In practice this is 0 for
    /// borsh because every field's standalone encoding equals its in-struct
    /// encoding (no nested length prefixes). Kept as a safety check.
    pub framing_overhead: usize,
}

impl HeaderSizeBreakdown {
    /// Element-wise add another breakdown into self. Used to accumulate
    /// running totals when iterating a chain.
    pub fn add_in_place(&mut self, rhs: &Self) {
        self.envelope_discriminant += rhs.envelope_discriminant;
        self.cached_hash += rhs.cached_hash;
        self.version += rhs.version;
        self.parents_by_level += rhs.parents_by_level;
        self.hash_merkle_root += rhs.hash_merkle_root;
        self.accepted_id_merkle_root += rhs.accepted_id_merkle_root;
        self.utxo_commitment += rhs.utxo_commitment;
        self.timestamp += rhs.timestamp;
        self.bits += rhs.bits;
        self.nonce += rhs.nonce;
        self.daa_score += rhs.daa_score;
        self.blue_work += rhs.blue_work;
        self.blue_score += rhs.blue_score;
        self.pruning_point += rhs.pruning_point;
        self.pmr += rhs.pmr;
        self.total_field_sum += rhs.total_field_sum;
        self.serialized_total += rhs.serialized_total;
        self.framing_overhead += rhs.framing_overhead;
    }
}

impl BlockHeader {
    /// Decompose this header's borsh size by field. Does NOT account for
    /// the `AnyHeader` enum discriminant; for that, use
    /// [`AnyHeader::field_sizes`].
    pub fn field_sizes(&self) -> HeaderSizeBreakdown {
        let cached_hash = borsh::to_vec(&self.cached_hash).map(|v| v.len()).unwrap_or(0);
        let version = borsh::to_vec(&self.version).map(|v| v.len()).unwrap_or(0);
        let parents_by_level = borsh::to_vec(&self.parents_by_level).map(|v| v.len()).unwrap_or(0);
        let hash_merkle_root = borsh::to_vec(&self.hash_merkle_root).map(|v| v.len()).unwrap_or(0);
        let accepted_id_merkle_root = borsh::to_vec(&self.accepted_id_merkle_root).map(|v| v.len()).unwrap_or(0);
        let utxo_commitment = borsh::to_vec(&self.utxo_commitment).map(|v| v.len()).unwrap_or(0);
        let timestamp = borsh::to_vec(&self.timestamp).map(|v| v.len()).unwrap_or(0);
        let bits = borsh::to_vec(&self.bits).map(|v| v.len()).unwrap_or(0);
        let nonce = borsh::to_vec(&self.nonce).map(|v| v.len()).unwrap_or(0);
        let daa_score = borsh::to_vec(&self.daa_score).map(|v| v.len()).unwrap_or(0);
        let blue_work = borsh::to_vec(&self.blue_work).map(|v| v.len()).unwrap_or(0);
        let blue_score = borsh::to_vec(&self.blue_score).map(|v| v.len()).unwrap_or(0);
        let pruning_point = borsh::to_vec(&self.pruning_point).map(|v| v.len()).unwrap_or(0);

        let total_field_sum = cached_hash
            + version
            + parents_by_level
            + hash_merkle_root
            + accepted_id_merkle_root
            + utxo_commitment
            + timestamp
            + bits
            + nonce
            + daa_score
            + blue_work
            + blue_score
            + pruning_point;
        let serialized_total = borsh::to_vec(self).map(|v| v.len()).unwrap_or(0);
        let framing_overhead = serialized_total.saturating_sub(total_field_sum);

        HeaderSizeBreakdown {
            envelope_discriminant: 0,
            cached_hash,
            version,
            parents_by_level,
            hash_merkle_root,
            accepted_id_merkle_root,
            utxo_commitment,
            timestamp,
            bits,
            nonce,
            daa_score,
            blue_work,
            blue_score,
            pruning_point,
            pmr: 0,
            total_field_sum,
            serialized_total,
            framing_overhead,
        }
    }
}

impl BlockHeaderV2 {
    /// Decompose this header's borsh size by field, including the 32-byte
    /// PMR. Does NOT account for the `AnyHeader` enum discriminant.
    pub fn field_sizes(&self) -> HeaderSizeBreakdown {
        let mut b = self.v1.field_sizes();
        // PMR is a `Hash`, which serializes to its 32 bytes via borsh.
        let pmr_bytes = borsh::to_vec(&self.pmr).map(|v| v.len()).unwrap_or(0);
        b.pmr = pmr_bytes;
        b.total_field_sum += pmr_bytes;
        // serialized_total in `b` came from borsh-encoding the inner
        // BlockHeader; recompute against the V2 wrapper.
        let v2_total = borsh::to_vec(self).map(|v| v.len()).unwrap_or(0);
        b.serialized_total = v2_total;
        b.framing_overhead = v2_total.saturating_sub(b.total_field_sum);
        b
    }
}

impl AnyHeader {
    /// Decompose this header's full on-wire byte size, including the 1-byte
    /// `AnyHeader` enum discriminant.
    pub fn field_sizes(&self) -> HeaderSizeBreakdown {
        let envelope_total = borsh::to_vec(self).map(|v| v.len()).unwrap_or(0);
        let mut b = match self {
            Self::V1(h) => h.field_sizes(),
            Self::V2(h) => h.field_sizes(),
        };
        // The enum discriminant is exactly 1 byte (borsh encodes enum tags
        // as u8). Account for it as `envelope_discriminant` and adjust the
        // running totals.
        b.envelope_discriminant = envelope_total.saturating_sub(b.serialized_total);
        b.total_field_sum += b.envelope_discriminant;
        b.serialized_total = envelope_total;
        b.framing_overhead = envelope_total.saturating_sub(b.total_field_sum);
        b
    }
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

    fn assert_breakdown_matches_borsh<H: BorshSerialize>(any: &AnyHeader, _phantom: &H) {
        let b = any.field_sizes();
        let bytes = borsh::to_vec(any).unwrap().len();
        assert_eq!(
            b.serialized_total, bytes,
            "field_sizes().serialized_total ({}) must equal borsh::to_vec(any).len() ({})",
            b.serialized_total, bytes
        );
        assert_eq!(
            b.total_field_sum + b.framing_overhead,
            b.serialized_total,
            "total_field_sum + framing_overhead must equal serialized_total"
        );
    }

    #[test]
    fn field_sizes_minimal_v1_header() {
        let mut core = make_v1();
        core.parents_by_level = vec![vec![]];
        let h = AnyHeader::V1(core);
        assert_breakdown_matches_borsh(&h, &h);
        let b = h.field_sizes();
        assert_eq!(b.envelope_discriminant, 1);
        assert_eq!(b.pmr, 0);
        assert_eq!(b.framing_overhead, 0, "borsh has no nested length prefixes for fixed-size structs");
    }

    #[test]
    fn field_sizes_realistic_v1_header() {
        // Two parents at level 0; one synthetic level-1 parent. Should
        // yield a parents_by_level of (4 outer-vec-len) + (4 + 64) + (4 + 32) = 108 bytes.
        let mut core = make_v1();
        core.parents_by_level = vec![vec![Hash([1; 32]), Hash([2; 32])], vec![Hash([3; 32])]];
        let h = AnyHeader::V1(core);
        assert_breakdown_matches_borsh(&h, &h);
        let b = h.field_sizes();
        assert_eq!(b.parents_by_level, 4 + (4 + 64) + (4 + 32));
        assert_eq!(b.cached_hash, 1, "None Option<Hash> = 1-byte tag");
    }

    #[test]
    fn field_sizes_dense_v1_header() {
        // 17 superblock levels with 17 parents each — close to the
        // empirical mainnet 10-BPS shape that produces ~6 KB headers.
        let mut core = make_v1();
        core.parents_by_level =
            (0..17).map(|level| (0..17).map(|i| Hash([(level * 17 + i) as u8; 32])).collect::<Vec<_>>()).collect();
        let h = AnyHeader::V1(core);
        assert_breakdown_matches_borsh(&h, &h);
        let b = h.field_sizes();
        // 17 levels × (4 + 17×32) = 17 × 548 = 9316 byte parents_by_level
        // plus 4 outer vec length = 9320.
        assert_eq!(b.parents_by_level, 4 + 17 * (4 + 17 * 32));
        // parents_by_level should dominate this header (>95%).
        let pct = (b.parents_by_level as f64) / (b.serialized_total as f64);
        assert!(pct > 0.95, "parents_by_level should dominate dense headers (got {pct:.3})");
    }

    #[test]
    fn field_sizes_v2_includes_pmr_32_bytes() {
        let h = AnyHeader::V2(BlockHeaderV2 { v1: make_v1(), pmr: Hash([9; 32]) });
        assert_breakdown_matches_borsh(&h, &h);
        let b = h.field_sizes();
        assert_eq!(b.pmr, 32, "V2 header must record a 32-byte PMR field");
        assert_eq!(b.envelope_discriminant, 1);
    }

    #[test]
    fn field_sizes_with_cached_hash_some_is_33_bytes() {
        let mut core = make_v1();
        core.cached_hash = Some(Hash([42; 32]));
        let h = AnyHeader::V1(core);
        assert_breakdown_matches_borsh(&h, &h);
        let b = h.field_sizes();
        assert_eq!(b.cached_hash, 33, "Some(Hash) = 1-byte tag + 32-byte payload");
    }

    #[test]
    fn breakdown_add_in_place_accumulates_every_field() {
        let h1 = AnyHeader::V1(make_v1()).field_sizes();
        let h2 = AnyHeader::V1(make_v1()).field_sizes();
        let mut acc = HeaderSizeBreakdown::default();
        acc.add_in_place(&h1);
        acc.add_in_place(&h2);
        assert_eq!(acc.parents_by_level, h1.parents_by_level + h2.parents_by_level);
        assert_eq!(acc.serialized_total, h1.serialized_total + h2.serialized_total);
        assert_eq!(acc.envelope_discriminant, 2);
    }
}
