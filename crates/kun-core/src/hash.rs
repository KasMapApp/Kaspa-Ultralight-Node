//! Hash newtype and hash-scheme abstraction.
//!
//! KUN cannot hardcode a hash function: Kaspa used Blake2B-256 pre-Crescendo
//! and Blake3 post-Crescendo (KIP-14). The specific era must be determined at
//! runtime — either from a node's `get_server_info` response or from an
//! out-of-band configuration — and every hashing call site threads a
//! [`HashScheme`] implementation through explicitly.

use std::fmt;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// Size of a Kaspa hash in bytes. KIP-6 calls this `L_a`.
pub const HASH_SIZE: usize = 32;

/// A 32-byte hash output. Constant-time equality via [`subtle`] prevents
/// timing-channel leaks in verifier-as-a-service deployments.
#[derive(Copy, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Hash(pub [u8; HASH_SIZE]);

impl Hash {
    pub const ZERO: Self = Self([0u8; HASH_SIZE]);

    #[inline]
    pub const fn from_bytes(bytes: [u8; HASH_SIZE]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8; HASH_SIZE] {
        &self.0
    }

    /// Count leading zero bits across the hash (MSB first). Used by the
    /// NiPoPoW superblock level formula:
    /// `level = leading_zeros(hash) − leading_zeros(target)`.
    pub fn leading_zero_bits(&self) -> u32 {
        let mut total = 0u32;
        for &b in &self.0 {
            if b == 0 {
                total += 8;
            } else {
                total += b.leading_zeros();
                break;
            }
        }
        total
    }
}

impl PartialEq for Hash {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        bool::from(self.0.ct_eq(&other.0))
    }
}

impl Eq for Hash {}

impl std::hash::Hash for Hash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for Hash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl From<[u8; HASH_SIZE]> for Hash {
    fn from(bytes: [u8; HASH_SIZE]) -> Self {
        Self(bytes)
    }
}

impl From<Hash> for [u8; HASH_SIZE] {
    fn from(h: Hash) -> Self {
        h.0
    }
}

/// Parse a hex-encoded hash, accepting optional `0x` prefix and any case.
impl std::str::FromStr for Hash {
    type Err = HashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.strip_prefix("0x").unwrap_or(s);
        let bytes = hex::decode(trimmed).map_err(|_| HashParseError::InvalidHex)?;
        if bytes.len() != HASH_SIZE {
            return Err(HashParseError::WrongLength { got: bytes.len(), want: HASH_SIZE });
        }
        let mut out = [0u8; HASH_SIZE];
        out.copy_from_slice(&bytes);
        Ok(Self(out))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HashParseError {
    #[error("invalid hex encoding")]
    InvalidHex,
    #[error("wrong length: got {got} bytes, want {want}")]
    WrongLength { got: usize, want: usize },
}

// ─── Hash scheme abstraction ─────────────────────────────────────────────

/// Runtime-selectable hash function.
///
/// All Merkle construction and hashing inside KUN goes through this trait so
/// we can swap Blake2B ↔ Blake3 when Crescendo activates without touching
/// proof code. The scheme is decided at program start from
/// [`HashEra::detect`] (or a config override) and passed by reference.
pub trait HashScheme: Send + Sync + 'static {
    fn era(&self) -> HashEra;

    /// Hash an arbitrary byte slice to 32 bytes.
    fn hash(&self, bytes: &[u8]) -> Hash;

    /// Domain-separated hash. `tag` is a short static byte-string like
    /// `b"kaspa:pmr:leaf:v1"`. The tag is length-prefixed to avoid ambiguity
    /// between concatenated tag and payload.
    fn hash_domain(&self, tag: &[u8], bytes: &[u8]) -> Hash {
        let mut pre = Vec::with_capacity(1 + tag.len() + bytes.len());
        pre.push(u8::try_from(tag.len()).expect("tag must be < 256 bytes"));
        pre.extend_from_slice(tag);
        pre.extend_from_slice(bytes);
        self.hash(&pre)
    }

    /// Hash a pair of children for an internal Merkle node. Conventionally
    /// `left || right` (left is index 2i, right is 2i+1).
    fn hash_pair(&self, tag: &[u8], left: &Hash, right: &Hash) -> Hash {
        let mut buf = [0u8; HASH_SIZE * 2];
        buf[..HASH_SIZE].copy_from_slice(&left.0);
        buf[HASH_SIZE..].copy_from_slice(&right.0);
        self.hash_domain(tag, &buf)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashEra {
    /// Blake2B-256 — pre-Crescendo mainnet.
    Blake2b,
    /// Blake3 — post-Crescendo (KIP-14).
    Blake3,
}

impl HashEra {
    /// Best-effort detection from a node's reported server version string.
    /// Real logic lives in `kun-rpc::server_info_to_era`; this helper is a
    /// placeholder / fallback for unit tests.
    pub fn from_version_hint(_version: &str) -> Option<Self> {
        // Phase 0 RPC discovery fills this in once we observe a real node.
        None
    }
}

pub struct Blake2bScheme;

impl HashScheme for Blake2bScheme {
    fn era(&self) -> HashEra {
        HashEra::Blake2b
    }
    fn hash(&self, bytes: &[u8]) -> Hash {
        let out = blake2b_simd::Params::new().hash_length(HASH_SIZE).hash(bytes);
        let mut arr = [0u8; HASH_SIZE];
        arr.copy_from_slice(out.as_bytes());
        Hash(arr)
    }
}

pub struct Blake3Scheme;

impl HashScheme for Blake3Scheme {
    fn era(&self) -> HashEra {
        HashEra::Blake3
    }
    fn hash(&self, bytes: &[u8]) -> Hash {
        let out = blake3::hash(bytes);
        Hash(*out.as_bytes())
    }
}

/// Construct the scheme matching an era. Allocates a boxed trait object so
/// the concrete type doesn't leak into higher-level APIs.
pub fn scheme_for(era: HashEra) -> Box<dyn HashScheme> {
    match era {
        HashEra::Blake2b => Box::new(Blake2bScheme),
        HashEra::Blake3 => Box::new(Blake3Scheme),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_leading_zero_bits_counts_bits_not_bytes() {
        assert_eq!(Hash([0u8; HASH_SIZE]).leading_zero_bits(), 256);
        let mut h = [0u8; HASH_SIZE];
        h[0] = 0x0f; // 0000_1111 — 4 leading zero bits
        assert_eq!(Hash(h).leading_zero_bits(), 4);
        let mut h = [0u8; HASH_SIZE];
        h[1] = 0x80;
        assert_eq!(Hash(h).leading_zero_bits(), 8);
    }

    #[test]
    fn hash_from_str_accepts_prefix_and_any_case() {
        let a: Hash =
            "0x".to_string().chars().chain("AB".chars().cycle().take(64)).collect::<String>().parse().unwrap();
        let b: Hash = "ab".chars().cycle().take(64).collect::<String>().parse().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn blake2b_and_blake3_produce_different_outputs() {
        let msg = b"kaspa";
        let a = Blake2bScheme.hash(msg);
        let b = Blake3Scheme.hash(msg);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_pair_is_order_sensitive() {
        let s = Blake2bScheme;
        let a = s.hash(b"a");
        let b = s.hash(b"b");
        assert_ne!(s.hash_pair(b"tag", &a, &b), s.hash_pair(b"tag", &b, &a));
    }

    #[test]
    fn domain_separation_disambiguates_overlapping_preimages() {
        let s = Blake2bScheme;
        // Without domain separation, hash("xy") == hash("x" || "y"). With our
        // length-prefixed tag, hash_domain("x", "y") differs from hash_domain("xy", "").
        assert_ne!(s.hash_domain(b"x", b"y"), s.hash_domain(b"xy", b""));
    }
}
