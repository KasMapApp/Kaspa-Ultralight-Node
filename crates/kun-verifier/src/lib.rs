//! `kun-verifier` — reserved.
//!
//! Pure, stateless, WASM-first verifier crate. In v0.1, verification logic
//! lives directly in `kun-proofs` (which is the easier-to-iterate shape while
//! the protocol details are still moving). In v0.2 the pure-compute slice is
//! extracted here so the WASM bundle can stay tight (<400 KB gzipped).
