# Kaspa Ultralight Node (KUN)

An empirical measurement tool and future ultralight client for the Kaspa
blockDAG network. v0.1 is a **measurement tool**: it connects to a full
`rusty-kaspa` node via wRPC, walks the selected chain from the current
pruning point to a stable selected-chain tip, and produces (and verifies) a
pre-KIP-6 "diamond-DAG" Proof of Chain Membership.

The goal is to produce the **first publicly-documented empirical baseline**
for PoChM proof sizes on live mainnet 10-BPS Kaspa — the number that KIP-6
claims to reduce by orders of magnitude.

Target tiers, for reference (v0.2 and beyond will realize these):

| Tier                  | Storage    | Role |
|-----------------------|------------|------|
| Verify-only           | < 100 MB   | Trusts UTXO commitment; queries a full node for UTXOs on demand. |
| Validating ultralight | 2 – 4 GB   | Local consensus UTXO set bootstrapped from a pruning-point commitment. |
| Wallet-capable        | 7 – 9 GB   | Adds the optional `utxoindex` for address queries. |

## Headline measurements (v0.1)

Two independent runs against mainnet 10-BPS Kaspa, same node, 5 hours apart,
straddling a pruning-point advance:

| Metric | Run 1 (early, deep in 12h cycle) | Run 2 (right after pruning advance) |
|---|---|---|
| Anchor (pruning point) | `1a4fa3af…04ffb` | `f1febd00…5d1c4` |
| Target (sink − 100)    | `0b82c720…6bf7f` | `a4dbcb5d…fb280` |
| **Proof size**         | **1,495,462,955 B (1.39 GiB)** | **1,202,595,243 B (1.12 GiB)** |
| Chain headers          | 245,927 | 196,531 |
| Avg bytes/header       | 6,082   | 6,118   |
| Generation time        | 504 s   | 445 s   |
| Verification time      | 236 – 256 ms | — |
| Verify throughput      | ~5.9 GB/s | — |

**Bytes are dominated by headers:** 99.93 % of proof size is the `chain`
field. `aux_parents` is reserved (v0.2 strict mode) and empty today.

### Comparison with KIP-6 predictions

| Finding | KIP-6 assumption | KUN measurement | Delta |
|---|---|---|---|
| **1. `L_e` (avg bytes/header)** | 248 B (pre-Crescendo) | 6,077 – 6,115 B | **24.6 ×** |
| **1.5. share of `parents_by_level` in each header** | not isolated; treated as part of `L_e` | **96.3 %** of every byte | dominates the field |
| **2a. selected-chain growth rate** | ≤ 1 chain block/sec ("reliable upper bound") | **~1.8 chain blocks/sec** | **1.8 ×** |
| **2b. selected-block fraction of total DAG** | "about half" (~50 % at 1 BPS) | **~18 %** at 10 BPS | dropped 2.8 × |
| **3. proof-size sawtooth amplitude** | not modelled | 19.6 % swing observed (max possible 28.6 %) across 12 h pruning cycle | new methodological note |
| **Composite: post-KIP-6 PoChM** | ~9 KB | with full headers as PMR leaves: **~100 KB**<br>with stripped headers: **~6.4 KB** | depends on PMR-leaf design |

Note on the block-rate distinction: Kaspa's BLOCK PRODUCTION rate is
10 blocks/sec total DAG (target inter-block time = 100 ms). The
SELECTED chain (the linear backbone GHOSTDAG picks via blue-work
argmax) grows at only ~1.8 of those 10 blocks/sec — KIP-6's PoChM
formula uses the selected-chain rate, not the block-production rate.

Finding 1.5 — that `parents_by_level` is 96 % of every header — is the
most consequential. KIP-6's "9 KB" projection is approximately right
*if* the PMR Merkle tree stores stripped headers (selected_parent +
identity + PMR ≈ 97 B), and ~10× too small *if* full headers are used.
This is now a design question for the KIP-6 author rather than
theoretical.

Full analysis: [`EMPIRICAL_NOTES.md`](EMPIRICAL_NOTES.md).

### Sample proofs for independent verification

Both proof files above are ~1.2–1.4 GB each. They are not committed to the
repo (GitHub's 100 MB limit), but are available for download so anyone can
verify them without running an 8-minute generation:

- `first.proof` (1.39 GiB) — pre pruning-advance
- `fresh.proof` (1.12 GiB) — post pruning-advance

**→ Hosted at:** `<link TBD — will be added once uploaded>`

**SHA-256 checksums.** Verify the download integrity before running
`kun proof verify`. On Windows: `Get-FileHash <path> -Algorithm SHA256`.
On macOS/Linux: `sha256sum <path>`.

```
first.proof  sha256:86dca9e18900c4016bdca5a28fcabfafa120c5f1addb46561c430ce8659bb5f9
fresh.proof  sha256:ebbe187c009b5a56946e81bf2df4a9858da05074fa196bc6c5448aeb0f47a22e
```

Verify via:

```bash
kun proof verify first.proof --against 1a4fa3af62dbbd3e28b4020e7544e9b8cf4467bb126c3cca2a164919e1e04ffb --trust-anchor
kun proof verify fresh.proof --against f1febd0069120967e5da59e98f9e5dfde9b854e0984f0c631f249dee9cb5d1c4 --trust-anchor
```

Expected: `VERIFIED in ~250 ms (<size> bytes)`.

## Quick start

Prerequisites:

- Rust 1.85+ (pinned via [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` will fetch it on first build)
- A running `rusty-kaspa` node reachable at `ws://<host>:17110` (your own
  local node, a remote node you control, or a trusted public node)

Build and confirm connectivity:

```bash
cargo build --release -p kun-cli
./target/release/kun dag info --node ws://localhost:17110
```

The `dag info` output includes `sink` (virtual's selected parent) — that's
the block KUN will auto-pick as a proof target if you don't supply one.

Generate + verify a proof end-to-end (~8 minutes on a local node):

```bash
./target/release/kun -v proof generate --node ws://localhost:17110 --out my.proof
./target/release/kun proof size my.proof --breakdown
./target/release/kun proof verify my.proof --against <anchor-hash-printed-above> --trust-anchor
```

`-v` prints progress lines every ~5 seconds during the two fetch phases
(paginated selected-chain hashes, then per-block headers).

Decompose an existing proof into per-field byte counts (no node needed):

```bash
./target/release/kun proof header-stats my.proof
./target/release/kun proof header-stats my.proof --out my.csv   # one row per header
```

The output table shows min / median / mean / p95 / max per field across
every header in the chain, plus a stripped-vs-full PoChM projection for
post-KIP-6 sizing.

## Local development against a patched rusty-kaspa

If you have a local `rusty-kaspa` clone and want to iterate against it
without touching the workspace manifest:

```bash
cp .cargo/config.toml.example .cargo/config.toml
# Edit .cargo/config.toml — uncomment the [patch] block and set the paths
cargo check --workspace
```

`.cargo/config.toml` is gitignored; the workspace still pins to the official
`v1.0.1` tag for everyone else and for CI.

## Workspace layout

```
crates/
├── kun-core/            Types, errors, Merkle primitives, Blake2B/Blake3 scheme
├── kun-proofs/          DiamondProof + PoChM (v0.2) generation / verification
├── kun-proof-source/    ProofSource trait; RpcProofSource + MockProofSource
├── kun-rpc/             KaspaRpcClient wrapper with retry + resolver
├── kun-cli/             Binary — clap v4 subcommands
├── kun-storage/         RocksDB-backed persistence (v0.2)
├── kun-sync/            Bootstrap + catch-up FSM (v0.2)
├── kun-verifier/        Pure stateless verifier (v0.2, WASM-first)
├── kun-client/          Public library facade (v0.2)
└── kun-wasm/            wasm-bindgen shim for browser verify-only (v0.2)
```

Each crate has a short `// reserved` placeholder until its v0.2 milestone
lands.

## Running tests

```bash
cargo test --workspace --lib
```

Expected: 33 tests pass — Merkle known-answer, borsh round-trips, diamond
proof round-trip + negative cases, header hashing, posterity chain
invariants, plus six per-field-size invariants for `HeaderSizeBreakdown`
covering V1, V2, dense superblock-level, and `cached_hash` cases.

## Roadmap

| Version | Scope | Status |
|---|---|---|
| **v0.1**   | Diamond-DAG PoChM generator + verifier; first empirical mainnet 10-BPS measurement; `kun proof generate / verify / size`; `kun bench proof-size` | shipped (Apr 2026) |
| **v0.1.1** | Per-field header decomposition (`kun proof header-stats`); Methodology + Limitations docs; reorg-safety honesty pass; SHA-256 checksums for proof artifacts | shipped (Apr 2026) |
| **v0.2**   | RocksDB storage; bootstrap + catch-up FSM; KIP-6 PMR **simulation** (compute PMR client-side, generate logarithmic PoChM, measure size); adaptive `safety_depth` driven by MyKAI network signals; batched header fetches (~5× faster generation); pure WASM verifier crate; Supabase telemetry | planned |
| **v0.3**   | Stripped-header proof variant (subject to KIP-6 design clarification); superblock-level census on archival data; week-long automated sampling harness | planned |
| **v1.0**   | Post-fork integration: real `BlockHeaderV2.pmr` from on-chain headers; production verifier API; library facade; phone-class verifier via WASM | gated on KIP-6 mainnet activation |

## Status

v0.1.1 produces real, reproducible measurements with full per-field
decomposition. The diamond-DAG baseline is what KIP-6 aims to replace
with logarithmic-size Proof of Chain Membership after a future hard
fork. Every sub-GB measurement we produce now is a baseline KIP-6
must improve on.

## License

ISC. See [`LICENSE`](LICENSE).
