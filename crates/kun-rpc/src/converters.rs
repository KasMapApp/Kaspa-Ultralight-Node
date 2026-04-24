//! Convert rusty-kaspa RPC types into KUN's portable types.
//!
//! All conversion is one-way (rusty-kaspa → KUN) for now; KUN never produces
//! blocks, only consumes them. Conversions are total where possible and
//! return [`RpcError::HeaderConversion`] only when the upstream type
//! contains a field that can't fit KUN's shape (e.g. a truncated blue_work).

use kaspa_consensus_core::BlueWorkType;
use kaspa_hashes::Hash as KHash;
use kaspa_rpc_core::{RpcHash, RpcHeader};

use kun_core::{AnyHeader, BlockHeader, Hash};

use crate::error::{Result, RpcError};

/// Convert a `kaspa_hashes::Hash` (32 bytes) into KUN's [`Hash`].
pub fn hash_from_kaspa(h: KHash) -> Hash {
    Hash(h.as_bytes())
}

/// Convert an [`RpcHash`] (alias for `kaspa_hashes::Hash`) into KUN's [`Hash`].
pub fn hash_from_rpc(h: RpcHash) -> Hash {
    hash_from_kaspa(h)
}

pub fn rpc_hash_from_kun(h: Hash) -> RpcHash {
    KHash::from_bytes(h.0)
}

/// Convert an [`RpcHeader`] into our portable `AnyHeader::V1`. The upstream
/// header's `hash` field is preserved in `cached_hash` so KUN uses it as
/// the authority for header identity (KUN's own domain-tagged fallback
/// hash does not match rusty-kaspa's real header hash; reconciliation is
/// a Phase 0 task).
pub fn rpc_header_into_any(rh: RpcHeader) -> Result<AnyHeader> {
    let cached_hash = Some(hash_from_kaspa(rh.hash));

    let parents_by_level: Vec<Vec<Hash>> =
        rh.parents_by_level.into_iter().map(|lvl| lvl.into_iter().map(hash_from_kaspa).collect()).collect();

    let blue_work = blue_work_to_array(&rh.blue_work)?;

    Ok(AnyHeader::V1(BlockHeader {
        cached_hash,
        version: rh.version,
        parents_by_level,
        hash_merkle_root: hash_from_kaspa(rh.hash_merkle_root),
        accepted_id_merkle_root: hash_from_kaspa(rh.accepted_id_merkle_root),
        utxo_commitment: hash_from_kaspa(rh.utxo_commitment),
        timestamp: rh.timestamp,
        bits: rh.bits,
        nonce: rh.nonce,
        daa_score: rh.daa_score,
        blue_work,
        blue_score: rh.blue_score,
        pruning_point: hash_from_kaspa(rh.pruning_point),
    }))
}

/// `BlueWorkType` is a `Uint192` wrapper in rusty-kaspa; we serialize to 24 BE
/// bytes. We go through the `{:x}` formatter to stay agnostic to the
/// upstream `Uint192` API surface (which has varied across rusty-kaspa
/// releases). This is only called once per fetched header — not a hot path.
fn blue_work_to_array(bw: &BlueWorkType) -> Result<[u8; 24]> {
    let hex_str = format!("{bw:x}");
    let padded = if hex_str.len() % 2 == 1 { format!("0{hex_str}") } else { hex_str };
    let bytes = parse_hex_bytes(&padded)
        .ok_or_else(|| RpcError::HeaderConversion(format!("blue_work unparseable hex: {padded}")))?;
    if bytes.len() > 24 {
        return Err(RpcError::HeaderConversion(format!("blue_work exceeded 192 bits (got {} bytes)", bytes.len())));
    }
    let mut out = [0u8; 24];
    out[24 - bytes.len()..].copy_from_slice(&bytes);
    Ok(out)
}

fn parse_hex_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bs = s.as_bytes();
    for i in (0..bs.len()).step_by(2) {
        let hi = hex_nibble(bs[i])?;
        let lo = hex_nibble(bs[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
