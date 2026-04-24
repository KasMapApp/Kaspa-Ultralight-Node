//! Diamond-DAG PoChM — pre-KIP-6 fallback.
//!
//! A diamond proof is the set of selected-chain block headers from a trusted
//! posterity anchor DOWN to the target block, each linking to the next via
//! `parents_by_level[0][0]` (the selected parent). KIP-6 §"PoChM Without
//! Hard-Fork" estimates ~24 MB at 1 BPS; at 10 BPS the selected chain still
//! advances at ~1 block/sec (network-limited), so proof size is dominated
//! by how long ago `target` was merged relative to the posterity anchor.
//!
//! This module implements the **non-strict** verifier: it follows selected
//! parents but does NOT re-run GHOSTDAG blue-work comparisons against sibling
//! parents. Strict mode (with `aux_parents`) lands in v0.2 behind a feature
//! flag; the proof layout already includes the slot so the wire format is
//! stable.

use kun_core::{DiamondProof, Hash, HashScheme, PosterityChain, VerifyError};

use kun_proof_source::ProofSource;

/// Build a [`DiamondProof`] for `target` against the current pruning point.
///
/// `aux_parents` is left empty in v0.1 (non-strict mode). v0.2 will fill it
/// with each chain entry's other parents so verifiers can independently
/// re-run GHOSTDAG's blue-work tiebreak.
pub async fn generate_diamond_proof(src: &dyn ProofSource, target: Hash) -> Result<DiamondProof, GenerateError> {
    let anchor_entry = src.posterity_of(target).await.map_err(GenerateError::Source)?;
    // We don't have the hash scheme in scope here — caller must pass it via
    // `diamond_chain`, which returns ordered headers. We do NOT re-hash here;
    // the verifier is the authority on identity.
    let chain = src.diamond_chain(target).await.map_err(GenerateError::Source)?;

    // Sanity: chain[0] is the anchor header (we re-derive its hash in verify).
    if chain.is_empty() {
        return Err(GenerateError::EmptyChain);
    }
    let posterity_anchor_hash = {
        // Caller-side hashing would need a HashScheme; instead record what
        // the source already told us (posterity_of returned its header).
        let ah = anchor_entry.header;
        // We tolerate the anchor appearing in `chain[0]` (the normal case).
        // The verifier re-derives both hashes under the runtime hash scheme.
        //
        // NOTE: we can't hash here without a HashScheme; ship the chain as-is
        // and have the verifier check chain[0].hash() == posterity_anchor.
        // Meanwhile, stash an all-zero hash and have the CLI caller patch it
        // in. Cleaner: accept the HashScheme via a caller argument.
        let _ = ah;
        Hash::ZERO
    };

    let aux_parents = vec![Vec::new(); chain.len()];

    Ok(DiamondProof { target, posterity_anchor: posterity_anchor_hash, chain, aux_parents })
}

/// Variant of [`generate_diamond_proof`] that takes a [`HashScheme`] so it
/// can compute the anchor hash up front. Preferred entry point.
pub async fn generate_diamond_proof_with_scheme(
    src: &dyn ProofSource,
    hs: &dyn HashScheme,
    target: Hash,
) -> Result<DiamondProof, GenerateError> {
    let anchor_entry = src.posterity_of(target).await.map_err(GenerateError::Source)?;
    let anchor_hash = anchor_entry.header.hash(hs);
    let chain = src.diamond_chain(target).await.map_err(GenerateError::Source)?;
    if chain.is_empty() {
        return Err(GenerateError::EmptyChain);
    }
    let aux_parents = vec![Vec::new(); chain.len()];
    Ok(DiamondProof { target, posterity_anchor: anchor_hash, chain, aux_parents })
}

/// Verify a diamond proof against a trusted posterity chain.
pub fn verify_diamond_proof(
    hs: &dyn HashScheme,
    proof: &DiamondProof,
    trusted: &PosterityChain,
) -> Result<(), VerifyError> {
    // 1. Anchor must be trusted.
    if !trusted.contains(proof.posterity_anchor) {
        return Err(VerifyError::UntrustedAnchor(proof.posterity_anchor));
    }
    // 2. Chain must be non-empty and start with the anchor.
    let chain = &proof.chain;
    if chain.is_empty() {
        return Err(VerifyError::MalformedEmptyProof);
    }
    if chain[0].hash(hs) != proof.posterity_anchor {
        return Err(VerifyError::UntrustedAnchor(proof.posterity_anchor));
    }
    // 3. Chain must end with target.
    if chain[chain.len() - 1].hash(hs) != proof.target {
        return Err(VerifyError::LeafMismatch);
    }
    // 4. Chain is ordered [older → newer]: chain[0] is the posterity anchor
    //    (pruning point, oldest), chain[last] is the target (sink, newest).
    //    Each window pair is (older, newer). The newer block's direct-parent
    //    list must contain the older block's hash — a weaker check than
    //    "selected" parenthood (which requires blue-work argmax) but enough
    //    to confirm adjacency on the DAG. v0.2 strict mode will upgrade
    //    this with full blue-work re-validation via `aux_parents`.
    for pair in chain.windows(2) {
        let older = &pair[0];
        let newer = &pair[1];
        let older_hash = older.hash(hs);
        let direct_parents = newer.core().parents_by_level.first().cloned().unwrap_or_default();
        if !direct_parents.contains(&older_hash) {
            return Err(VerifyError::SelectedParentMismatch);
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error(transparent)]
    Source(kun_proof_source::ProofSourceError),

    #[error("diamond chain was empty (target out of pruning window?)")]
    EmptyChain,
}

impl From<kun_proof_source::ProofSourceError> for GenerateError {
    fn from(e: kun_proof_source::ProofSourceError) -> Self {
        Self::Source(e)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kun_core::{AnyHeader, Blake2bScheme, BlockHeader, Hash, HashScheme, PosterityChain, PosterityHeader};
    use kun_proof_source::MockProofSource;

    use super::*;

    fn header_with_parent(scheme: &dyn HashScheme, parent: Hash, seed: u8) -> AnyHeader {
        let _ = scheme;
        AnyHeader::V1(BlockHeader {
            cached_hash: None,
            version: 1,
            parents_by_level: vec![vec![parent]],
            hash_merkle_root: Hash([seed; 32]),
            accepted_id_merkle_root: Hash([seed; 32]),
            utxo_commitment: Hash([seed; 32]),
            timestamp: u64::from(seed) * 1000,
            bits: 0,
            nonce: u64::from(seed),
            daa_score: u64::from(seed) * 10,
            blue_work: [0; 24],
            blue_score: u64::from(seed),
            pruning_point: Hash::ZERO,
        })
    }

    /// Build a chain with the anchor (pruning-point role) at the OLDEST
    /// end and the target at the NEWEST end. This matches production
    /// semantics where anchor = block's embedded `pruning_point` field,
    /// which is always an ancestor of the block. Returns
    /// `(src, anchor, target, trusted_posterity_set)`.
    async fn build_chain(n: usize) -> (Arc<MockProofSource>, Hash, Hash, PosterityChain) {
        let hs: Arc<dyn HashScheme> = Arc::new(Blake2bScheme);
        let src = Arc::new(MockProofSource::new(hs.clone()));

        // Anchor: oldest block, no parent in our mock DAG.
        let anchor_hdr = AnyHeader::V1(BlockHeader {
            cached_hash: None,
            version: 1,
            parents_by_level: vec![vec![]],
            hash_merkle_root: Hash([0; 32]),
            accepted_id_merkle_root: Hash([0; 32]),
            utxo_commitment: Hash([0; 32]),
            timestamp: 0,
            bits: 0,
            nonce: 0,
            daa_score: 0,
            blue_work: [0; 24],
            blue_score: 0,
            pruning_point: Hash::ZERO,
        });
        let anchor = src.push_chain(anchor_hdr);
        src.mark_posterity(anchor, 0);

        // Walk `n` steps forward to the target.
        let mut prev = anchor;
        for i in 1..=n {
            let h = header_with_parent(&*hs, prev, i as u8 + 1);
            prev = src.push_chain(h);
        }
        let target = prev;

        let mut trusted = PosterityChain::new();
        trusted
            .extend(
                &*hs,
                PosterityHeader {
                    header: src.get_header(anchor).await.unwrap(),
                    posterity_depth3: anchor,
                    chain_index: 0,
                },
            )
            .unwrap();
        (src, anchor, target, trusted)
    }

    #[tokio::test]
    async fn diamond_round_trip_succeeds() {
        let (src, _anchor, target, trusted) = build_chain(5).await;
        let hs = Blake2bScheme;
        let p = generate_diamond_proof_with_scheme(&*src, &hs, target).await.unwrap();
        verify_diamond_proof(&hs, &p, &trusted).expect("valid proof must verify");
    }

    #[tokio::test]
    async fn diamond_untrusted_anchor_rejected() {
        let (src, _a, target, _trusted) = build_chain(3).await;
        let hs = Blake2bScheme;
        let p = generate_diamond_proof_with_scheme(&*src, &hs, target).await.unwrap();
        let empty = PosterityChain::new();
        assert!(matches!(verify_diamond_proof(&hs, &p, &empty), Err(VerifyError::UntrustedAnchor(_))));
    }

    #[tokio::test]
    async fn diamond_broken_selected_parent_rejected() {
        let (src, _a, target, trusted) = build_chain(4).await;
        let hs = Blake2bScheme;
        let mut p = generate_diamond_proof_with_scheme(&*src, &hs, target).await.unwrap();
        // Corrupt chain[2]'s selected parent.
        if let AnyHeader::V1(h) = &mut p.chain[2] {
            h.parents_by_level[0][0] = Hash([0xff; 32]);
        }
        assert!(matches!(verify_diamond_proof(&hs, &p, &trusted), Err(VerifyError::SelectedParentMismatch)));
    }

    #[tokio::test]
    async fn diamond_size_scales_with_chain_length() {
        let (src, _a, target, _t) = build_chain(10).await;
        let hs = Blake2bScheme;
        let p = generate_diamond_proof_with_scheme(&*src, &hs, target).await.unwrap();
        assert_eq!(p.chain.len(), 11); // anchor + 10 children
    }
}
