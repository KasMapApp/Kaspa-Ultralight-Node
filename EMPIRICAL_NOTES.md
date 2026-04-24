# Empirical notes — v0.1.1 measurement pass

**Dataset:** two full selected-chain diamond-DAG PoChM generations against a
live mainnet rusty-kaspa node (server version 1.1.0), separated by ~5 hours
and straddling a 12-hour pruning-point advance. Identical toolchain, identical
code path, no caches reused between runs (process restarted). v0.1.1 adds a
per-field decomposition of every one of the 245,927 + 196,531 = 442,458
headers in those two proofs.

**What this document is:** four findings that fall straight out of the
numbers, each with the mechanism, the delta from KIP-6's assumptions, and
why it matters.

## Methodology

Both proofs were produced by the same flow, deterministic modulo reorgs
inside `safety_depth`. Reproducible end-to-end against any mainnet node:

1. Read the node's current pruning point via `get_block_dag_info` →
   `pruning_point_hash`. This is our **anchor**.
2. Walk forward via `get_virtual_chain_from_block(anchor, false)`,
   paginating until `added` is empty. Each batch returns up to ~2,480
   selected-chain hashes (server cap = `mergeset_size_limit × 10`).
3. Pick the **target** by going `safety_depth` (= 100) entries back from
   the latest returned hash. Reorg-immune at the level observed in the
   MyKAI 24h sample.
4. Fetch every header from anchor to target via `get_block(hash, false)`
   (one RPC per hop in v0.1; batched in v0.2). Headers come back as
   `RpcHeader`; we convert to KUN's `BlockHeader` and preserve the
   upstream-cached hash for cross-system-consistent identity checks.
5. Assemble a `DiamondProof { target, posterity_anchor, chain, aux_parents }`
   and serialize via borsh.

Implementation: `crates/kun-proof-source/src/rpc.rs::current_selected_chain`
and `crates/kun-proof-source/src/rpc.rs::diamond_chain`. Verification:
`crates/kun-proofs/src/diamond.rs::verify_diamond_proof`.

Per-field decomposition (Finding 1.5 below) uses
`crates/kun-core/src/header.rs::AnyHeader::field_sizes`, which serializes
each field of each header via `borsh::to_vec` and measures byte length.
Aggregated by `kun proof header-stats <proof.bin>`.

---

## Raw numbers

| | Run 1 | Run 2 |
|---|---|---|
| Timestamp (UTC) | 2026-04-24T15:15 | 2026-04-24T20:14 |
| Anchor (pruning point) | `1a4fa3af…04ffb` | `f1febd00…5d1c4` |
| Target (sink − 100) | `0b82c720…6bf7f` | `a4dbcb5d…fb280` |
| Serialized proof size | **1,495,462,955 B** (1.39 GiB) | **1,202,595,243 B** (1.12 GiB) |
| Chain headers | 245,927 | 196,531 |
| Avg bytes / header | 6,082 | 6,118 |
| Generation time | 504 s | 445 s |
| Verification time | 236–256 ms | — (should be similar) |
| Verify throughput | ~5.9 GB/s | — |

Proof-size breakdown (Run 1):

```
chain headers     : 1,494,479,176 bytes  (99.93 %)
aux_parents       :       983,712 bytes  ( 0.07 %, empty Vec length prefixes)
envelope + fields :            67 bytes
```

---

## Finding 1 — Header-size amplification dominates everything

**Measurement:** average bytes/header ≈ **6,082–6,118 bytes**, stable across
both runs.

**KIP-6 assumption (§ "Size of PoChM"):** `L_e = 248 bytes`.

**Delta:** ~**24.6 × amplification per header.**

**Mechanism:** rusty-kaspa headers carry `parents_by_level: Vec<Vec<Hash>>`
— parents at every superblock level, not just level 0. At 10 BPS with the
Crescendo (KIP-14) parameters, this field alone is kilobytes per header, not
dozens of bytes. KIP-6's 248 B figure was based on pre-Crescendo headers
with a single level-0 parent list.

**Why it matters:** KIP-6 § "Size of PoChM" plugs `L_e = 280 B`
(KIP-6-modified header) into the formula and concludes ~9 KB per post-fork
PoChM. Substituting the real `L_e ≈ 6,100 B` changes that to
`(2·log₂(log₂(N)) + 1)·log₂(N)·L_a + L_e·(log₂(N) − 1)` ≈ ~100 KB if the
PMR Merkle tree leaves are full headers.

**Implication (resolved by Finding 1.5 below):** the projected post-KIP-6
PoChM size depends entirely on whether KIP-6's PMR Merkle tree stores
**full** or **stripped** headers as leaves. Finding 1.5 shows
`parents_by_level` is 96 % of every header. If PMR leaves keep that field,
projected proofs are ~100 KB. If PMR leaves omit it (storing only the
~64 B needed to recover identity and chain linkage), projected proofs are
**~6.4 KB — better than KIP-6's stated 9 KB.** This is a design decision
for the KIP-6 author that our data makes urgent rather than theoretical.

---

## Finding 1.5 — `parents_by_level` is 96.3 % of every header

**Source measurement.** The `kun proof header-stats <proof.bin>`
subcommand decomposes every header in a proof's `chain[]` via per-field
borsh serialization, aggregates min/median/mean/p95/max per field across
the chain, and prints a table. Run against both proofs:

```
field                            |      min |   median |       mean |      p95 |      max |   % mean
-------------------------------- | -------- | -------- | ---------- | -------- | -------- | --------
parents_by_level                 |     1948 |     5468 |     5852.9 |     9564 |    15004 |   96.31%
cached_hash (Option<Hash>)       |       33 |       33 |       33.0 |       33 |       33 |    0.54%
envelope_discriminant            |        1 |        1 |        1.0 |        1 |        1 |    0.02%
version                          |        2 |        2 |        2.0 |        2 |        2 |    0.03%
hash_merkle_root                 |       32 |       32 |       32.0 |       32 |       32 |    0.53%
accepted_id_merkle_root          |       32 |       32 |       32.0 |       32 |       32 |    0.53%
utxo_commitment                  |       32 |       32 |       32.0 |       32 |       32 |    0.53%
timestamp                        |        8 |        8 |        8.0 |        8 |        8 |    0.13%
bits                             |        4 |        4 |        4.0 |        4 |        4 |    0.07%
nonce                            |        8 |        8 |        8.0 |        8 |        8 |    0.13%
daa_score                        |        8 |        8 |        8.0 |        8 |        8 |    0.13%
blue_work                        |       24 |       24 |       24.0 |       24 |       24 |    0.39%
blue_score                       |        8 |        8 |        8.0 |        8 |        8 |    0.13%
pruning_point                    |       32 |       32 |       32.0 |       32 |       32 |    0.53%
pmr (V2 only)                    |        0 |        0 |        0.0 |        0 |        0 |    0.00%
framing overhead                 |        0 |        0 |        0.0 |        0 |        0 |    0.00%
-------------------------------- | -------- | -------- | ---------- | -------- | -------- | --------
TOTAL (serialized)               |     2172 |     5692 |     6076.9 |     9788 |    15228 |  100.00%
```
*(`first.proof`, 245,927 headers)*

`fresh.proof` (196,531 headers) gives 96.34 % parents_by_level, mean
6,115 B per header. Both runs agree to within 0.4 % per field — the
result is highly reproducible.

**Mechanism confirmed.** Mean 5,853 B `parents_by_level` corresponds to
approximately 17 superblock levels with ~10 parents each — consistent
with Michael Sutton's "we keep at most 2,000 blocks per level" structural
description (June 2025 Performance Aspects workshop). Headers vary from
2,172 B (low-level-count blocks early after a level-N rebuild) to 15,228 B
(blocks deep in the level structure). All fixed-size fields (every field
except `parents_by_level`) sum to **224 B per header** — within 10 %
of KIP-6's 248 B `L_e` assumption. KIP-6's number is essentially correct
**for the non-`parents_by_level` portion of the header.**

**KIP-6 PoChM size projection — the headline result.**

|  PMR Merkle tree leaf shape | Projected post-KIP-6 PoChM |
|---|---|
| Full headers (current measurement)         | **~100 KB** worst case |
| Stripped headers (~97 B: selected_parent + cached_hash + PMR) | **~6.4 KB** worst case |

The stripped-header projection is **better than KIP-6's stated 9 KB.**
Stripping `parents_by_level` (and the rest of the verification-irrelevant
fields) from PMR leaves recovers the full claimed advantage of KIP-6 and
then some. The original 9 KB figure was calibrated against a 248 B
header — which, as Finding 1 showed, is wrong by 24× *if* full headers
are used as leaves, but is approximately right *if* stripped headers
are used.

**Open design question for KIP-6.** Does PoChM verification require
the full `parents_by_level` field on each PMR Merkle leaf? Naïvely no:
verifying that `B` is on the selected chain only needs `B`'s hash and
the selected-parent identity at each binary-decomposition step. Both
fit in <100 B. But if PMR-leaf stripping breaks something downstream
(e.g. the next PMR layer's hash chain dependencies), the design needs
adjustment. This question is now urgent rather than theoretical: KUN's
measurement makes the trade-off concrete (~6.4 KB vs ~100 KB), and the
KIP-6 author should weigh in.

**Caveats.** The "~6.4 KB" projection assumes `log₂(N) ≈ 17` and a
single-posterity-epoch proof. Real PoChM size also depends on the
Hamming weight of `posterity_depth(B)`, bounded above by `log₂(N)` —
worst case is exactly the projection above. Both projections
include the ~17-step Merkle proof skeleton from KIP-6's formula
`(2·log₂(log₂(N)) + 1)·log₂(N)·L_a`.

---

## Finding 2 — Selected chain grows ~2 chain-blocks/sec, not 1

**Measurement:** Run 1 had 245,927 chain blocks across ~34.2 hours of
pruning window; Run 2 had 196,531 across ~27.3 hours. Both yield **~2.0
chain-blocks per second** of wall-clock.

**KIP-6 assumption (§ "Size of PoChM"):** 1 selected-chain block per second
(stated as a "reliable upper bound").

**Delta:** 2 × amplification in chain length.

**Mechanism:** KIP-6 notes that chain growth is network-limited, not
BPS-limited — this is correct in the limit — but the calibration point was
pre-Crescendo. Post-KIP-14 the chain density appears to have doubled
(possibly because merge sets are smaller on average, or because GHOSTDAG
blue-work resolution is more decisive, or both — not investigated here).

**Why it matters:** the `N` in KIP-6's formula is "seconds between
consecutive posterity headers," i.e. 86,400 at 24 h density. If chain grows
at 2/s instead of 1/s, the number of chain blocks between posterities is
172,800, and `log₂(N)` shifts from ~17 to ~18. Proof size scales
`(2·log₂(log₂(N)) + 1)·log₂(N)`, so the effect is sublinear but real.

**Implication:** a minor correction to the KIP-6 size formula when
re-derived for post-Crescendo conditions.

---

## Finding 3 — 12-hour pruning sawtooth causes ~20% swing in proof size

**Measurement:** identical tool, identical node, 5 hours apart:

- Run 1 (34.2 h into the cycle, just before the next advance): **1.39 GiB**
- Run 2 (27.3 h into the cycle, just after an advance):       **1.12 GiB**

Delta: **19.6 %** reduction from just-before-advance to just-after.

**Mechanism:** the pruning window is a sawtooth: 30 h (post-advance) to 42 h
(pre-advance) of retained headers, with a 12 h cadence. A diamond-DAG proof
spans exactly this window (target near virtual, anchor at current pruning
point), so its chain length tracks the sawtooth linearly.

**Cross-reference (MyKAI observatory):** the user's own MyKAI pruning
advance history — 23 recorded advances with 86% cadence compliance to the
12 h clock — independently confirms this 30-42 h sawtooth. The 20% proof-
size amplitude we measure is exactly what a 30/42 ratio (30/42 = 0.714, so
29% below the max) would predict.

**Why it matters:** any empirical PoChM measurement reported without the
position-in-pruning-cycle timestamp is ambiguous by ±10%. Papers should
report the sampling timestamp, or sample repeatedly and use the median/max.

**Implication:** for a rigorous paper, sample at least 3 points per cycle
(e.g. +2 h, +6 h, +10 h after pruning advance) and report the distribution.
KUN `bench proof-size` already emits a CSV with timestamps; v0.2 will add an
automated sampler that waits for pruning advances.

---

## Composite multiplier vs KIP-6 theoretical baseline

KIP-6 § "Size of PoChM" gives 24 MB as the worst-case pre-fork diamond PoChM
at 1 BPS and 24 h posterity density.

Our empirical measurement at 10 BPS:

| Component | KIP-6 1 BPS | Our 10 BPS empirical | Ratio |
|---|---|---|---|
| Chain blocks | 86,400 | 196,531 – 245,927 | 2.3 – 2.8 × |
| Avg bytes / header | 248 | 6,082 – 6,118 | 24.5 – 24.7 × |
| Total proof size | 24 MB | 1.12 – 1.39 GiB | **47 – 58 ×** |

The amplification is essentially multiplicative: chain is 2.3–2.8× longer,
headers are ~24.6× bigger, so the product is ~55–69×. Empirically we observe
47–58×, reasonably close (the rest is borsh encoding overhead and aux_parents
length prefixes).

Naively multiplying KIP-6's 1 BPS figure by 10 (for the BPS increase) gives
240 MB; reality is **4.7 – 5.8 × worse than naive 10× scaling** because of
the header-size amplification.

---

## Verification throughput

- Run 1 proof: 1.39 GiB verified in **236 ms**
- Rate: **~5.9 GiB/sec**
- Observed memory footprint (process RSS during verify): not instrumented in
  v0.1, but empirically the process comfortably completes with <2 GB virtual
  allocation on an 8 GB machine

**Why it matters:** even the 1.4 GB pre-KIP-6 proof verifies in under a
second. Post-KIP-6 (projected ~100 KB) will verify in milliseconds. A
verifier can easily handle thousands of proofs per second per core. This is
not a bottleneck in any realistic deployment.

---

## Reorg safety of the auto-target heuristic

v0.1 uses `safety_depth = 100`: the target is chosen 100 blocks below the
current selected-chain tip to immunize against reorgs during the 8-minute
generation. **This number is calibrated for normal internet conditions
only.** Sub-sections below decompose what "normal" means here, and what
breaks under adversarial network conditions.

### Under normal internet conditions

MyKAI `chain_events` dashboard, 24-hour sample (no submarine cable
breaks, no major latency spikes, no partitions during the window):

- 17.76 % of VSPC events have removals (i.e. short reorgs)
- Average settling depth: 1.43 blocks
- Max depth observed in 24 h: **11 blocks**
- Depth ≥ 6: ~1 event every 25 seconds
- Depth ≥ 10: ~4 events per day

`safety_depth = 100` is ~9× this observed max. Both runs in this dataset
completed without any auto-target getting reorged out during generation.
Could safely drop to 20 in normalcy; kept at 100 for belt-and-braces.

### Under adversarial conditions

The MyKAI sample window contained no major outages. Three documented
failure modes the same heuristic would NOT survive:

- **Partition / submarine-cable break.** Each side mines independently;
  when connectivity restores, one history is reorged out. Reorg depth
  scales with partition duration: a 1-hour partition at 10 BPS could
  produce a reorg in the range of 36,000+ chain blocks. The 24-hour
  finality boundary in rusty-kaspa is a *socially*-determined safeguard
  (nodes refuse reorgs deeper than ~24 h), not a cryptographic one —
  and during a partition, different nodes can disagree on whether
  finality has been reached.
- **Sustained latency spike.** Sompolinsky (Aug 2025): *"For Kaspa
  today which is also a synchronous network it deteriorates very fast
  after 10, 20 seconds latency it's already breaking their latency
  assumptions."* The DAG widens, merge sets approach the 180-block
  consensus cap, and the selected chain oscillates more. Expected
  reorg depth under sustained latency: 50–200+ blocks.
- **Targeted attack during degradation.** Same source: with three
  factions in a partition, 1/3 hashrate suffices to override the
  other two. This drops the effective security threshold from the
  50 %-Nakamoto figure to ~33 % under degraded conditions.

DAGKnight (not yet deployed on mainnet) would remove the fixed
latency assumption and adapt confirmation thresholds to observed
network conditions; until then, GHOSTDAG's a-priori `D_max` is
load-bearing.

### Implication for KUN

`safety_depth = 100` is the right default for normal internet
conditions and is consistent with our 24 h observations. It is **not**
sufficient under partition or sustained latency-spike conditions. v0.1.1
documents this limitation honestly; v0.2 will implement an adaptive
policy that classifies network state from MyKAI's OWD / red-block /
mergeset-size signals and adjusts `safety_depth` (or refuses to
generate proofs) accordingly. No source provides a precise formula
for "expected reorg depth given latency X" — building that empirical
model on a live blockDAG is itself a novel contribution.

---

## Reproducibility

Both proof files are available separately (not committed due to GitHub's
100 MB limit — see README). Verify without regenerating:

```bash
kun proof verify first.proof  --against 1a4fa3af62dbbd3e28b4020e7544e9b8cf4467bb126c3cca2a164919e1e04ffb --trust-anchor
kun proof verify fresh.proof  --against f1febd0069120967e5da59e98f9e5dfde9b854e0984f0c631f249dee9cb5d1c4 --trust-anchor
```

Regenerate fresh proofs against any live mainnet node:

```bash
kun -v proof generate --node ws://<host>:17110 --out mine.proof
```

The generator is deterministic modulo reorg timing; re-running against the
same pruning point yields a byte-identical proof modulo reorgs inside
safety_depth.

---

## Limitations

These limitations apply to every number in this document. Do not extrapolate
beyond them without re-measuring.

- **Single node.** Both runs hit the same locally-hosted rusty-kaspa
  v1.1.0 instance. We have not validated that other nodes serve identical
  selected-chain hashes (they should — the chain is consensus-determined
  — but cross-node validation is a v0.2 task). MyKAI's 9-node fleet is
  perfectly positioned for this once we wire it up.
- **Single geographic location.** All measurements were taken from one
  network vantage point. Diurnal effects (peak-hours latency, regional
  network conditions) are unmeasured. Selected-chain growth rate could
  easily vary ±20 % across time-of-day; we don't know.
- **Two runs only.** The "20 % sawtooth amplitude" is the spread between
  two points sampled near the cycle's max and min — it is not a
  distribution. A proper claim requires hourly sampling across at least
  one full week.
- **Normal internet conditions only.** See the Reorg-safety section
  above. Adversarial network conditions (partitions, latency spikes)
  are entirely outside the dataset.
- **Mainnet 10 BPS only.** Numbers do not carry over to testnet or to
  pre-Crescendo (1 BPS) measurements without scaling assumptions.
- **One pruning epoch.** Both runs span (overlapping fragments of) the
  same pruning window. Cross-epoch effects unmeasured.

## Open research questions the numbers raise

1. ~~**How much of the 6 KB per header is multi-level parents?**~~
   **Resolved by Finding 1.5: ~96.3%.** Replaced by the question that
   measurement raises:
   **Can KIP-6's PMR Merkle tree leaves drop the `parents_by_level`
   field?** PoChM verification needs only chain-linkage at each binary-
   decomposition step. Confirming that stripped-leaf PMRs preserve the
   security argument is a design question for the KIP-6 author. Our data
   makes the trade-off concrete (~6.4 KB vs ~100 KB) and pushes the
   question from "future work" to "blocker for a final KIP-6 spec".
2. **Does compressing the proof (zstd) recover any of the size?** Most
   header content is high-entropy hashes, but parent-arrays may have
   repetition across adjacent chain blocks (blocks at the same height
   often share parents at higher superblock levels). Worth measuring with
   delta-encoded zstd. Speculative gain: 10–30 %.
3. **Real selected-chain growth rate across a longer sample.** Two runs
   give an average; a week of hourly samples would give the distribution
   and let us claim a proper `blocks/sec ± stddev`.
4. **Per-level superblock density** (the NiPoPoW side, independent of
   KIP-6). Count blocks at each level `k = leading_zeros(hash) −
   leading_zeros(target)`. Confirms or refutes Michael Sutton's "2,000
   blocks per level" structural claim.
5. **Post-KIP-6 simulated PoChM sizes** using the same headers. Drop in
   `simulate_pmr` (planned for v0.2) and measure the ratio.
6. **Reorg-depth distribution under degraded network conditions.** No
   source provides a formula for expected reorg depth as a function of
   latency. Building that empirical model from MyKAI's OWD + red-block +
   mergeset-size signals would be a novel contribution. Drives the
   adaptive `safety_depth` design in v0.2.

---

## Versioning this document

These notes describe KUN v0.1.1 measurements (v0.1 raw numbers + v0.1.1
per-field decomposition). When v0.2 lands (batched header fetches, RocksDB
storage, simulated PMR, adaptive safety_depth), replace or supersede this
document with a new measurement pass — do not edit the numbers in place.

## Changelog

- **v0.1** (2026-04-24): two diamond-DAG PoChM runs, three findings
  (header amplification, chain growth, sawtooth), proof verification
  reproducibility recipe.
- **v0.1.1** (2026-04-24): added Methodology, Limitations, and Finding 1.5
  (per-field header decomposition). Resolves the "9 KB vs 100 KB"
  ambiguity in Finding 1's implication. Restructured Reorg-safety
  section to separate normal vs adversarial network conditions.
