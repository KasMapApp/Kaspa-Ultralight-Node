//! Exit-code classification. Every CLI error maps to a stable exit code so
//! scripts and CI can make decisions without parsing text.

use anyhow::Error;

pub const OK: u8 = 0;
pub const GENERIC: u8 = 1;
pub const INVALID_PROOF: u8 = 2;
pub const NODE_UNREACHABLE: u8 = 3;
pub const USAGE: u8 = 4;
pub const PARTIAL: u8 = 5;

/// Walk the error chain to pick the most specific exit code.
pub fn classify(err: &Error) -> u8 {
    for cause in err.chain() {
        if let Some(_verify) = cause.downcast_ref::<kun_core::VerifyError>() {
            return INVALID_PROOF;
        }
        if let Some(rpc) = cause.downcast_ref::<kun_rpc::RpcError>() {
            return match rpc {
                kun_rpc::RpcError::Connect(_) | kun_rpc::RpcError::Construction(_) => NODE_UNREACHABLE,
                _ => GENERIC,
            };
        }
    }
    GENERIC
}
