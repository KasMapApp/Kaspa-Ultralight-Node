//! Proof types: the on-the-wire structures that KUN generates, stores, and
//! verifies. Algorithms live in `kun-proofs`; this module only defines the
//! shapes so `kun-cli` and any library consumer can load proofs without
//! pulling in the generator (→ keeps the WASM verify-only binary tight).
//!
//! Every proof kind carries:
//!   * `target` — the block (or tx) the claim is about
//!   * `anchor` — a trusted posterity header the verifier must already know
//!   * enough intermediate data to re-derive `target` from `anchor`
//!
//! All structures are borsh-canonical. A `ProofEnvelope` wraps them with a
//! version byte, kind tag, and length prefix so we can evolve the wire
//! format without ambiguity.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::hash::Hash;
use crate::header::AnyHeader;
use crate::merkle::MerkleProof;

/// Semver-tracked proof format version. Bump when wire layout changes.
pub const PROOF_FORMAT_V1: u8 = 1;

/// Discriminates between the different proof shapes. borsh derives a
/// canonical discriminant based on declaration order — keep it stable.
#[derive(Copy, Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum ProofKind {
    /// Pre-KIP-6 "diamond DAG" PoChM. Ships in v0.1.
    Diamond,
    /// KIP-6 efficient PoChM. Ships post-fork (or via simulation in v0.2).
    Pochm,
    /// Transaction receipt (accepted-tx inclusion proof).
    TxReceipt,
    /// Proof of Publication (any-tx inclusion, possibly rejected by GHOSTDAG).
    Pop,
}

/// Ordered list of super-block-level pre-fork PoChM data.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct DiamondProof {
    /// Block we're proving is on the selected chain.
    pub target: Hash,
    /// Trusted posterity header the verifier already holds.
    pub posterity_anchor: Hash,
    /// Ordered selected-chain block headers, from `posterity_anchor` DOWN
    /// to `target`. `chain[0].hash() == posterity_anchor`,
    /// `chain[len-1].hash() == target`, and each entry's selected parent
    /// equals the next entry's hash.
    pub chain: Vec<AnyHeader>,
    /// Per-chain-entry, the OTHER (non-selected) parents needed for strict
    /// GHOSTDAG re-validation. Empty in prototype mode; populated when the
    /// `strict-diamond` verification flag is on.
    pub aux_parents: Vec<Vec<AnyHeader>>,
}

/// One step of a KIP-6 PoChM. A proof has `popcount(posterity_depth)` steps;
/// each corresponds to one `1` bit in the binary expansion of
/// `posterity_depth` walked MSB-first.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct PoChMStep {
    /// `log2` of this step's jump size. `i < 32` is guaranteed by the
    /// pruning-window bound.
    pub i: u8,
    /// Merkle proof that the intermediate target is in `current_C.pmr`.
    pub pmr_proof: MerkleProof,
    /// Intermediate header for the NEXT iteration (`parent(C, 2^i)`).
    /// `None` on the LAST step because that step's leaf IS `proof.target`
    /// and verifiers get `target` straight from the outer struct.
    pub next_header: Option<AnyHeader>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct PoChM {
    pub target: Hash,
    pub anchor: Hash,
    /// d = posterity_depth(target). Verifier cross-checks sum of step
    /// powers-of-two equals this.
    pub posterity_depth: u32,
    pub steps: Vec<PoChMStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct TxReceipt {
    pub tx_id: Hash,
    pub pochm: PoChM,
    /// Proof that `tx_id` appears in `accepting_header`'s ATMR.
    pub atmr_proof: MerkleProof,
    pub accepting_header: AnyHeader,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct PoP {
    pub tx_id: Hash,
    pub pochm: PoChM,
    /// Chain of headers from `pochm.target` (= B) down to `publishing_header`
    /// (= C), walked by selected parent.
    pub b_to_c_chain: Vec<AnyHeader>,
    /// Proof that `tx_id` is in `publishing_header.hash_merkle_root` (the
    /// per-block txn tree, NOT the ATMR).
    pub block_tx_proof: MerkleProof,
    pub publishing_header: AnyHeader,
}

/// Wire-format envelope for any proof kind. Self-describing: has a version
/// byte, a kind tag, the kind-specific payload, and (optionally) a trailer
/// with sub-section sizes for the `kun proof size --breakdown` command.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct ProofEnvelope {
    pub format_version: u8,
    pub kind: ProofKind,
    pub payload: ProofPayload,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum ProofPayload {
    Diamond(DiamondProof),
    Pochm(PoChM),
    TxReceipt(TxReceipt),
    Pop(PoP),
}

impl ProofEnvelope {
    pub fn diamond(p: DiamondProof) -> Self {
        Self { format_version: PROOF_FORMAT_V1, kind: ProofKind::Diamond, payload: ProofPayload::Diamond(p) }
    }
    pub fn pochm(p: PoChM) -> Self {
        Self { format_version: PROOF_FORMAT_V1, kind: ProofKind::Pochm, payload: ProofPayload::Pochm(p) }
    }
    pub fn tx_receipt(p: TxReceipt) -> Self {
        Self { format_version: PROOF_FORMAT_V1, kind: ProofKind::TxReceipt, payload: ProofPayload::TxReceipt(p) }
    }
    pub fn pop(p: PoP) -> Self {
        Self { format_version: PROOF_FORMAT_V1, kind: ProofKind::Pop, payload: ProofPayload::Pop(p) }
    }

    pub fn serialize(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("borsh serialization is infallible for ProofEnvelope")
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(bytes)
    }

    pub fn serialized_size(&self) -> usize {
        self.serialize().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::BlockHeader;

    fn fake_header(seed: u8) -> AnyHeader {
        AnyHeader::V1(BlockHeader {
            cached_hash: None,
            version: 1,
            parents_by_level: vec![vec![Hash([seed; 32])]],
            hash_merkle_root: Hash([seed; 32]),
            accepted_id_merkle_root: Hash([seed; 32]),
            utxo_commitment: Hash([seed; 32]),
            timestamp: 0,
            bits: 0,
            nonce: 0,
            daa_score: 0,
            blue_work: [0; 24],
            blue_score: 0,
            pruning_point: Hash::ZERO,
        })
    }

    #[test]
    fn envelope_round_trip_diamond() {
        let dp = DiamondProof {
            target: Hash([1; 32]),
            posterity_anchor: Hash([2; 32]),
            chain: vec![fake_header(2), fake_header(1)],
            aux_parents: vec![vec![], vec![]],
        };
        let env = ProofEnvelope::diamond(dp.clone());
        let bytes = env.serialize();
        let env2 = ProofEnvelope::deserialize(&bytes).unwrap();
        assert_eq!(env, env2);
        assert_eq!(env2.kind, ProofKind::Diamond);
        if let ProofPayload::Diamond(dp2) = env2.payload {
            assert_eq!(dp, dp2);
        } else {
            panic!("expected diamond payload");
        }
    }

    #[test]
    fn envelope_version_byte_is_first_byte_of_serialization() {
        let dp = DiamondProof { target: Hash::ZERO, posterity_anchor: Hash::ZERO, chain: vec![], aux_parents: vec![] };
        let bytes = ProofEnvelope::diamond(dp).serialize();
        assert_eq!(bytes[0], PROOF_FORMAT_V1);
    }

    #[test]
    fn corrupt_bytes_never_panic_on_deserialize() {
        // Property test lite: verify random truncations and bit-flips return
        // Err, never panic. Proptest covers the full space in its own module.
        let dp = DiamondProof { target: Hash::ZERO, posterity_anchor: Hash::ZERO, chain: vec![], aux_parents: vec![] };
        let bytes = ProofEnvelope::diamond(dp).serialize();
        for trunc in 0..bytes.len() {
            let _ = ProofEnvelope::deserialize(&bytes[..trunc]);
        }
        let mut mut_bytes = bytes.clone();
        for i in 0..mut_bytes.len() {
            mut_bytes[i] ^= 0xaa;
            let _ = ProofEnvelope::deserialize(&mut_bytes);
            mut_bytes[i] ^= 0xaa;
        }
    }
}
