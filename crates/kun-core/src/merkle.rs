//! Merkle tree primitives used by both PMR (KIP-6) and ATMR trees.
//!
//! The tree is binary with domain-separated leaf and internal-node hashes;
//! odd layers duplicate the last element (Bitcoin-style) so the tree is
//! always a balanced power-of-two. This matches the construction pattern
//! rusty-kaspa uses for existing Merkle roots in block headers.
//!
//! The domain tags applied here are KUN-internal placeholders. Once the
//! rusty-kaspa KIP-6 implementation lands, the real tags should be imported
//! from there (see `docs/rpc-contract.md` assumption D).

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::hash::{Hash, HashScheme};

/// Domain tag for leaf hashes inside the PMR tree. Placeholder — must be
/// reconciled with rusty-kaspa once KIP-6 merges.
pub const PMR_LEAF_TAG: &[u8] = b"kun:pmr:leaf:v1";
/// Domain tag for internal nodes of the PMR tree.
pub const PMR_NODE_TAG: &[u8] = b"kun:pmr:node:v1";
/// Domain tag for leaf hashes inside the ATMR tree (accepted-txn Merkle).
pub const ATMR_LEAF_TAG: &[u8] = b"kun:atmr:leaf:v1";
/// Domain tag for internal nodes of the ATMR tree.
pub const ATMR_NODE_TAG: &[u8] = b"kun:atmr:node:v1";

/// A Merkle inclusion proof. `siblings[0]` is the leaf's sibling,
/// `siblings[1]` is the sibling of the parent, and so on up to the root.
///
/// `directions` is a bitvector where bit `i` tells the verifier whether the
/// computed node at level `i` is on the LEFT (bit = 0) or RIGHT (bit = 1) of
/// its sibling. LSB of `directions[0]` corresponds to the leaf layer.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct MerkleProof {
    pub leaf: Hash,
    pub siblings: Vec<Hash>,
    /// Packed direction bits, one per `siblings` entry. `directions_len` is
    /// the number of meaningful LSB bits across `directions` when joined as
    /// a little-endian stream.
    pub directions: Vec<u64>,
    pub directions_len: u32,
}

impl MerkleProof {
    /// Read bit `i` (0 = leaf layer) out of the packed direction bitstream.
    pub fn dir_bit(&self, i: u32) -> bool {
        debug_assert!(i < self.directions_len);
        let word = self.directions[(i / 64) as usize];
        (word >> (i % 64)) & 1 == 1
    }
}

/// Build a dense Merkle tree from a list of leaf hashes and return the root.
/// Leaves are hashed through the leaf-tag domain before being inserted; an
/// internal parent is `hash_domain(node_tag, left || right)`.
///
/// Odd layers duplicate the last element. Empty input returns a zero hash
/// under the leaf tag (arbitrary but deterministic).
pub fn merkle_root(hs: &dyn HashScheme, leaf_tag: &[u8], node_tag: &[u8], leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return hs.hash_domain(leaf_tag, &[]);
    }
    let mut layer: Vec<Hash> = leaves.iter().map(|h| hs.hash_domain(leaf_tag, &h.0)).collect();
    while layer.len() > 1 {
        if layer.len() % 2 == 1 {
            let last = *layer.last().unwrap();
            layer.push(last);
        }
        layer = layer
            .chunks(2)
            .map(|pair| hs.hash_pair(node_tag, &pair[0], &pair[1]))
            .collect();
    }
    layer[0]
}

/// Construct a Merkle inclusion proof for `leaves[index]`. Returns the
/// [`MerkleProof`] whose `leaf` field is the PRE-HASH value `leaves[index]`
/// (so verifiers can re-derive the leaf under the domain tag themselves).
pub fn merkle_proof(
    hs: &dyn HashScheme,
    leaf_tag: &[u8],
    node_tag: &[u8],
    leaves: &[Hash],
    index: usize,
) -> MerkleProof {
    assert!(index < leaves.len(), "index {} out of range (len {})", index, leaves.len());
    let mut layer: Vec<Hash> = leaves.iter().map(|h| hs.hash_domain(leaf_tag, &h.0)).collect();
    let mut idx = index;
    let mut siblings = Vec::new();
    let mut dirs: Vec<u64> = Vec::new();
    let mut dir_len: u32 = 0;
    while layer.len() > 1 {
        if layer.len() % 2 == 1 {
            let last = *layer.last().unwrap();
            layer.push(last);
        }
        let sibling_idx = idx ^ 1;
        siblings.push(layer[sibling_idx]);

        // direction bit: if we are on the LEFT (idx even), sibling is on RIGHT → bit = 1
        let bit_right = idx % 2 == 0;
        let word = (dir_len / 64) as usize;
        while word >= dirs.len() {
            dirs.push(0);
        }
        if bit_right {
            dirs[word] |= 1u64 << (dir_len % 64);
        }
        dir_len += 1;

        layer = layer
            .chunks(2)
            .map(|pair| hs.hash_pair(node_tag, &pair[0], &pair[1]))
            .collect();
        idx /= 2;
    }
    MerkleProof { leaf: leaves[index], siblings, directions: dirs, directions_len: dir_len }
}

/// Verify a Merkle inclusion proof against a claimed root. Returns `Ok(())`
/// iff the proof is well-formed and the reconstructed root matches.
pub fn verify_merkle_proof(
    hs: &dyn HashScheme,
    leaf_tag: &[u8],
    node_tag: &[u8],
    proof: &MerkleProof,
    claimed_root: Hash,
) -> Result<(), MerkleError> {
    if proof.siblings.len() != proof.directions_len as usize {
        return Err(MerkleError::MalformedProof);
    }
    let mut cur = hs.hash_domain(leaf_tag, &proof.leaf.0);
    for (i, sibling) in proof.siblings.iter().enumerate() {
        // directions bit i: 1 = sibling is on the RIGHT (cur is on the LEFT).
        let sibling_on_right = proof.dir_bit(i as u32);
        cur = if sibling_on_right {
            hs.hash_pair(node_tag, &cur, sibling)
        } else {
            hs.hash_pair(node_tag, sibling, &cur)
        };
    }
    if cur == claimed_root {
        Ok(())
    } else {
        Err(MerkleError::RootMismatch)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MerkleError {
    #[error("malformed merkle proof: siblings and directions disagree in length")]
    MalformedProof,
    #[error("reconstructed root did not match claimed root")]
    RootMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Blake2bScheme;

    fn leaves(n: usize) -> Vec<Hash> {
        (0..n)
            .map(|i| {
                let mut b = [0u8; 32];
                b[0] = i as u8;
                Hash(b)
            })
            .collect()
    }

    #[test]
    fn single_leaf_root_is_domain_hash_of_leaf() {
        let s = Blake2bScheme;
        let ls = leaves(1);
        let root = merkle_root(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls);
        assert_eq!(root, s.hash_domain(PMR_LEAF_TAG, &ls[0].0));
    }

    #[test]
    fn proof_verifies_for_every_leaf_in_a_4_leaf_tree() {
        let s = Blake2bScheme;
        let ls = leaves(4);
        let root = merkle_root(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls);
        for i in 0..4 {
            let p = merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls, i);
            verify_merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &p, root).unwrap();
        }
    }

    #[test]
    fn proof_verifies_for_every_leaf_in_an_8_leaf_tree() {
        let s = Blake2bScheme;
        let ls = leaves(8);
        let root = merkle_root(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls);
        for i in 0..8 {
            let p = merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls, i);
            verify_merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &p, root).unwrap();
        }
    }

    #[test]
    fn odd_sized_trees_duplicate_last_leaf() {
        let s = Blake2bScheme;
        let ls = leaves(5);
        let root = merkle_root(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls);
        for i in 0..5 {
            let p = merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls, i);
            verify_merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &p, root).unwrap();
        }
    }

    #[test]
    fn corrupt_sibling_fails_verification() {
        let s = Blake2bScheme;
        let ls = leaves(4);
        let root = merkle_root(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls);
        let mut p = merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls, 2);
        p.siblings[0].0[0] ^= 0xff;
        assert!(matches!(
            verify_merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &p, root),
            Err(MerkleError::RootMismatch)
        ));
    }

    #[test]
    fn flipped_direction_bit_fails_verification() {
        let s = Blake2bScheme;
        let ls = leaves(4);
        let root = merkle_root(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls);
        let mut p = merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls, 2);
        p.directions[0] ^= 1;
        assert!(matches!(
            verify_merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &p, root),
            Err(MerkleError::RootMismatch)
        ));
    }

    #[test]
    fn borsh_round_trip() {
        let s = Blake2bScheme;
        let ls = leaves(4);
        let p = merkle_proof(&s, PMR_LEAF_TAG, PMR_NODE_TAG, &ls, 3);
        let bytes = borsh::to_vec(&p).unwrap();
        let p2: MerkleProof = borsh::from_slice(&bytes).unwrap();
        assert_eq!(p, p2);
    }
}
