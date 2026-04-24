# RPC contract — Phase 0 discovery checklist

Before KUN can claim to build correct proofs, we must verify what rusty-kaspa
v1.0.1 actually exposes over wRPC. This document tracks each assumption and
the discovery method.

## Method

Stand up `kun dag info --node ws://localhost:17110` (or against a public
resolver node). Read source in a local `rusty-kaspa` clone (under `rpc/`)
where observable. Record what is confirmed vs. what remains speculative.

Legend: ✅ confirmed · ⚠ partial · ❓ unknown · ❌ denied

---

## A. `get_virtual_chain_from_block` response size cap

**Claim:** for any start hash still in the pruning window, the response
contains the full `added_chain_block_hashes` up to virtual, with no internal
cap below ~86,400 entries.

**Source evidence:** `rpc/core/src/model/message.rs:896-900` — the response
is `{ removed, added, accepted_transaction_ids }` with no bound. No
truncation flag in the request.

**Status:** ❓ Not verified against a live node. Pagination via
`start_hash` advance is implementable if it turns out capped.

**Verify via:** call it with `start_hash = pruning_point`, read
`added_chain_block_hashes.len()`, compare to `block_count` from
`get_block_dag_info`.

## B. `accepted_transaction_ids` ordering vs ATMR leaf order

**Claim:** the order of tx ids in
`RpcAcceptedTransactionIds.accepted_transaction_ids` matches the order used
to construct the block's `accepted_id_merkle_root` (ATMR).

**Source evidence:** message type in `model/message.rs`; leaf-ordering rule
lives in `kaspa-consensus/src/processes/acceptance_data.rs` (full consensus
crate, which KUN intentionally does not depend on).

**Status:** ❓ Critical for `tx_receipt` proof (v0.2); not used in v0.1.
If the orders differ, the receipt-constructor must sort before building the
inclusion proof.

**Verify via:** for a known chain block, pull its ATMR and its accepted-tx
list via RPC, rebuild the Merkle tree ourselves, compare roots.

## C. Depth-3 posterity pointer ≡ current pruning-point pointer

**Claim:** in the current era (pre-decoupling per KIP-6 §"Posterity Header
Density"), the `pruning_point` field on each block header IS the depth-3
posterity pointer.

**Source evidence:** `RpcHeader.pruning_point` field in
`rpc/core/src/model/header.rs:44`. KIP-6 §"Posterity Headers" confirms
depth-3 convention.

**Status:** ✅ Confirmed by combining source + spec.

## D. PMR and ATMR domain-separation tags

**Claim:** KUN's placeholder tags (`kun:pmr:leaf:v1`, `kun:pmr:node:v1`,
`kun:atmr:leaf:v1`, `kun:atmr:node:v1`) must be replaced with whatever
rusty-kaspa uses once KIP-6 merges.

**Source evidence:** KIP-6 does not specify tag bytes; this is an impl
detail. rusty-kaspa's existing `accepted_id_merkle_root` Merkle (which is
today's ATMR) does not use explicit domain tags — it uses a bespoke
hashing routine we'll need to mirror when we reconcile.

**Status:** ⚠ Placeholder. Tracked as a pre-v0.2 reconciliation task.

**Verify via:** wait for the rusty-kaspa KIP-6 PR; import the constant.

## E. `Resolver::default()` offline behaviour

**Claim:** constructing `Resolver::default()` does not perform I/O or
require network connectivity; it only populates a static list of public
nodes.

**Source evidence:** `rpc/wrpc/client/src/resolver.rs:69-77` — the default
constructor parses `RESOLVER_CONFIG` (a TOML literal compiled into the
binary). No I/O.

**Status:** ✅ Confirmed from source.

## F. `kaspa-wrpc-client` v1.0.1 wasm32 target still builds

**Claim:** `cargo build --target wasm32-unknown-unknown -p kaspa-wrpc-client`
succeeds as of tag `v1.0.1`.

**Status:** ❓ Unverified. Relevant only for v0.2 `kun-wasm`. Not on the
v0.1 critical path.

## G. Hash era (Blake2B pre-Crescendo, Blake3 post-Crescendo)

**Claim:** we can decide which scheme to use from the output of
`get_server_info` + the current network epoch.

**Status:** ❓ Not yet verified. v0.1 hardcodes Blake2B with a warning when
the auto-detector is asked for a decision. A separate follow-up task will
extract the Crescendo activation block height and bake the decision in.

## H. `get_pruning_point_proof` availability

**Claim (flagged by Agent B):** there is NO public RPC method that returns
a precomputed pruning-point proof (a native NiPoPoW representation). The
superblock levels exist internally but are not surfaced.

**Source evidence:** `rpc/core/src/api/rpc.rs` grep for `pruning_point`
returns only `pruning_point_hash` on `GetBlockDagInfoResponse`. No
`get_pruning_point_proof` method in the `RpcApi` trait.

**Status:** ✅ Confirmed denied. KUN must reconstruct proofs client-side
or receive trusted checkpoints out of band.

## I. Superblock-level enumeration

**Claim:** there is no RPC to enumerate blocks by superblock level. The
level formula is `leading_zeros(hash) − leading_zeros(target)`; KUN
computes this client-side from `RpcHeader.bits` (difficulty target) and
the block's hash.

**Status:** ✅ Confirmed by omission. Formula is implementable from
`kun-core::Hash::leading_zero_bits` once we bring in target-extraction
from `bits`.

## J. `get_block(hash, include_transactions=true)` accepted-tx list shape

**Claim:** the returned `RpcBlock.verbose_data.accepted_transaction_ids`
contains the canonical ATMR leaf list.

**Status:** ❓ Unverified. Required for v0.2 tx-receipts. Same verification
approach as assumption B.

---

## Exit criteria

Phase 0 is complete when every row above is ✅ or ❌ with a documented
consequence. v0.1 code already tolerates ❓ on B, D, F, G, J because it
does not rely on those methods for diamond-DAG proofs.
