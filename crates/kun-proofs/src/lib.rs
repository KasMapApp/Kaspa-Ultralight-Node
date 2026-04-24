//! `kun-proofs` — proof generation and verification for KUN.
//!
//! Ships three proof constructors in v0.1:
//!
//! - [`diamond::generate_diamond_proof`] / [`diamond::verify_diamond_proof`]
//!   — pre-KIP-6 selected-chain proof (works on any current rusty-kaspa node)
//!
//! Scaffolded for v0.2+:
//!
//! - [`pochm`] — KIP-6 efficient PoChM (simulation + real post-fork)
//! - [`tx_receipt`] — accepted-tx inclusion proofs
//! - [`pop`] — proof-of-publication

pub mod diamond;
pub mod pochm;
pub mod pop;
pub mod tx_receipt;

pub use diamond::{generate_diamond_proof, verify_diamond_proof};
pub use kun_core::VerifyError;
