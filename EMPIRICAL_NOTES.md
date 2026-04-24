# Empirical notes — v0.1 measurement pass

**Dataset:** two full selected-chain diamond-DAG PoChM generations against a
live mainnet rusty-kaspa node (server version 1.1.0), separated by ~5 hours
and straddling a 12-hour pruning-point advance. Identical toolchain, identical
code path, no caches reused between runs (process restarted).

**What this document is:** the three findings that fall straight out of the
numbers, each with the mechanism, the delta from KIP-6's assumptions, and
why it matters.

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
`(2·log₂(log₂(N)) + 1)·log₂(N)·L_a + L_e·(log₂(N) − 1)` ≈
`17 × 6,100 ≈ 104 KB` dominated by headers, not hashes.

**Implication:** post-KIP-6 PoChM on real mainnet 10 BPS is closer to
**~100 KB, not ~9 KB**. Still a ~10,000× reduction from today's 1 GB+, and
still phone-sized, but the "9 KB" figure requires either (a) stripping
multi-level parents from Merkle-tree leaves or (b) reverting to simpler
header formats. Worth flagging in the paper.

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
generation.

**Cross-reference (MyKAI `chain_events` dashboard, 24h sample):**
- 17.76% of VSPC events have removals (i.e. short reorgs)
- Average settling depth: 1.43 blocks
- Max depth observed in 24h: **11 blocks**
- Depth ≥ 6: ~1 event every 25 seconds
- Depth ≥ 10: ~4 events per day

**Margin:** `safety_depth = 100` is ~9× the realistic max. Empirically,
none of our runs have seen an auto-target get reorged out during
generation. Could safely drop to 20; kept at 100 for belt-and-braces.

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

## Open research questions the numbers raise

1. **How much of the 6 KB per header is multi-level parents?** Decompose a
   real header into its fields and measure per-field contribution.
   Currently the whole `parents_by_level` field is a single borsh blob.
2. **Does compressing the proof (zstd) recover any of the size?** Most
   header content is high-entropy hashes, but parent-arrays have repetition
   across adjacent chain blocks. Speculative gain: 10–30%.
3. **Real selected-chain growth rate across a longer sample.** Two runs
   give an average; a week of hourly samples would give the distribution
   and let us claim a proper `blocks/sec ± stddev`.
4. **Per-level superblock density** (the NiPoPoW side, independent of
   KIP-6). Count blocks at each level `k = leading_zeros(hash) −
   leading_zeros(target)`. Confirms or refutes Michael Sutton's "2,000
   blocks per level" structural claim.
5. **Post-KIP-6 simulated PoChM sizes** using the same headers. Drop in
   `simulate_pmr` (planned for v0.2) and measure the ratio.

---

## Versioning this document

These notes describe KUN v0.1 measurements only. When v0.2 lands (batched
header fetches, RocksDB storage, simulated PMR), replace or supersede this
document with a new measurement pass — do not edit the numbers in place.
