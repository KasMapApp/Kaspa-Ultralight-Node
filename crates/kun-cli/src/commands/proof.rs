use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{anyhow, Context as _};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use kun_core::{
    AnyHeader, DiamondProof, Hash, HashScheme, HeaderSizeBreakdown, PosterityChain, PosterityHeader, ProofEnvelope,
    ProofKind, ProofPayload,
};
use kun_proofs::{diamond, verify_diamond_proof};

use crate::commands::Globals;
use crate::context::Ctx;

#[derive(Args)]
pub struct ProofArgs {
    #[command(subcommand)]
    pub command: ProofCommand,
}

#[derive(Subcommand)]
pub enum ProofCommand {
    /// Build a proof for a block.
    Generate(GenerateArgs),
    /// Verify a previously-generated proof.
    Verify(VerifyArgs),
    /// Report a proof's serialized size and (optionally) sub-section breakdown.
    Size(SizeArgs),
    /// Decompose every header in a proof into per-field byte counts.
    /// Aggregates min/median/mean/p95/max per field across the whole chain
    /// and prints a table. Use this to answer "where do the ~6 KB go?"
    HeaderStats(HeaderStatsArgs),
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Block hash (hex, optional 0x prefix). Omit to let KUN auto-pick a
    /// stable selected-chain target `--safety-depth` blocks back from the
    /// current sink (recommended for benchmarking against live mainnet,
    /// because explicit sinks can reorg during the ~minute-long walk).
    #[arg(long)]
    pub block: Option<String>,
    /// When auto-picking a target (no --block), stay this many blocks back
    /// from the latest selected-chain entry to be immune to short reorgs.
    /// Ignored if --block is set.
    #[arg(long, default_value_t = 100)]
    pub safety_depth: u32,
    /// Which proof kind to build.
    #[arg(long, value_enum, default_value_t = Kind::Diamond)]
    pub kind: Kind,
    /// Output path. Defaults to `./proof-<short-hash>.bin`.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct VerifyArgs {
    /// Path to the proof file.
    pub proof: PathBuf,
    /// Explicit anchor to verify against; defaults to the cached posterity
    /// chain at ~/.kun/posterity.borsh (TOFU against the current pruning
    /// point if absent).
    #[arg(long)]
    pub against: Option<String>,
    /// Skip anchor-authenticity check. For offline demos only.
    #[arg(long, default_value_t = false)]
    pub trust_anchor: bool,
}

#[derive(Args)]
pub struct SizeArgs {
    pub proof: PathBuf,
    #[arg(long, default_value_t = false)]
    pub breakdown: bool,
}

#[derive(Args)]
pub struct HeaderStatsArgs {
    /// Path to the proof file to decompose.
    pub proof: PathBuf,
    /// Optional CSV output path. Writes one row per header with all field
    /// sizes — good for paper plots. Stdout still gets the summary table.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Kind {
    /// Pre-KIP-6 selected-chain proof (v0.1 default).
    Diamond,
    /// KIP-6 efficient PoChM (v0.2+).
    Pochm,
    /// Accepted-tx inclusion proof (v0.2+).
    Txr,
    /// Proof of Publication (v0.2+).
    Pop,
}

pub async fn run(args: ProofArgs, globals: Globals) -> anyhow::Result<()> {
    match args.command {
        ProofCommand::Generate(a) => generate(a, globals).await,
        ProofCommand::Verify(a) => verify(a, globals).await,
        ProofCommand::Size(a) => size(a, globals).await,
        ProofCommand::HeaderStats(a) => header_stats(a, globals).await,
    }
}

// ─── generate ─────────────────────────────────────────────────────────────

async fn generate(args: GenerateArgs, globals: Globals) -> anyhow::Result<()> {
    use kun_proof_source::ProofSource;

    let ctx = Ctx::from_globals(&globals).await?;

    // Resolve target: either user-supplied, or auto-picked from a walked chain.
    let (block, auto_chain_headers): (Hash, Option<Vec<kun_core::AnyHeader>>) = if let Some(raw) = args.block {
        (raw.parse().context("parse --block as hex hash")?, None)
    } else {
        tracing::info!(
            safety_depth = args.safety_depth,
            "no --block supplied; auto-picking stable selected-chain target"
        );
        let (chosen, chain) =
            ctx.src.current_selected_chain(args.safety_depth).await.context("auto-picking selected-chain target")?;
        (chosen, Some(chain))
    };

    let env = match args.kind {
        Kind::Diamond => {
            let start = std::time::Instant::now();
            let proof = if let Some(chain) = auto_chain_headers {
                // We already have the chain from the auto-pick walk; assemble
                // the DiamondProof directly (avoids a second full walk).
                let anchor_hash = chain.first().map(|h| h.hash(&*ctx.hs)).unwrap_or(Hash::ZERO);
                let aux_parents = vec![Vec::new(); chain.len()];
                kun_core::DiamondProof { target: block, posterity_anchor: anchor_hash, chain, aux_parents }
            } else {
                diamond::generate_diamond_proof_with_scheme(&*ctx.src, &*ctx.hs, block)
                    .await
                    .context("generate diamond proof")?
            };
            tracing::info!(
                elapsed_ms = start.elapsed().as_millis() as u64,
                chain_len = proof.chain.len(),
                "diamond proof generated"
            );
            ProofEnvelope::diamond(proof)
        }
        Kind::Pochm | Kind::Txr | Kind::Pop => {
            anyhow::bail!("{:?} proofs are a v0.2 deliverable; v0.1 supports only --kind diamond", args.kind);
        }
    };

    let bytes = env.serialize();
    let out_path = args.out.unwrap_or_else(|| {
        let short = &block.to_string()[..12];
        PathBuf::from(format!("proof-{short}.bin"))
    });
    std::fs::write(&out_path, &bytes).with_context(|| format!("writing {}", out_path.display()))?;

    // Also persist the current pruning point as a trust-on-first-use
    // posterity anchor in ~/.kun/posterity.borsh if the file doesn't exist.
    ensure_posterity_file(&ctx).await?;

    if globals.json {
        #[derive(Serialize)]
        struct Out {
            block: String,
            size_bytes: usize,
            path: String,
            kind: &'static str,
        }
        let kind = match env.kind {
            ProofKind::Diamond => "diamond",
            ProofKind::Pochm => "pochm",
            ProofKind::TxReceipt => "txr",
            ProofKind::Pop => "pop",
        };
        serde_json::to_writer_pretty(
            std::io::stdout(),
            &Out { block: block.to_string(), size_bytes: bytes.len(), path: out_path.display().to_string(), kind },
        )?;
        println!();
    } else {
        println!("proof written       : {}", out_path.display());
        println!("serialized size     : {} bytes", bytes.len());
        if let ProofPayload::Diamond(dp) = &env.payload {
            println!("chain headers       : {}", dp.chain.len());
            println!("anchor              : {}", dp.posterity_anchor);
        }
    }

    Ok(())
}

// ─── verify ───────────────────────────────────────────────────────────────

async fn verify(args: VerifyArgs, globals: Globals) -> anyhow::Result<()> {
    let ctx = Ctx::from_globals(&globals).await?;
    let bytes = std::fs::read(&args.proof).with_context(|| format!("reading {}", args.proof.display()))?;
    let env = ProofEnvelope::deserialize(&bytes).map_err(|e| anyhow!("failed to decode proof: {e}"))?;

    if env.format_version != kun_core::PROOF_FORMAT_V1 {
        anyhow::bail!("unsupported proof format version: {}", env.format_version);
    }

    let trusted = load_or_init_posterity_for_envelope(&ctx, &env, args.against.as_deref(), args.trust_anchor).await?;

    let start = std::time::Instant::now();
    let result: Result<(), kun_core::VerifyError> = match &env.payload {
        ProofPayload::Diamond(dp) => verify_diamond_proof(&*ctx.hs, dp, &trusted),
        ProofPayload::Pochm(_) | ProofPayload::TxReceipt(_) | ProofPayload::Pop(_) => {
            anyhow::bail!("only --kind diamond is supported for verification in v0.1");
        }
    };
    let elapsed_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(()) => {
            if globals.json {
                #[derive(Serialize)]
                struct Out {
                    valid: bool,
                    duration_ms: u64,
                    size_bytes: usize,
                }
                serde_json::to_writer_pretty(
                    std::io::stdout(),
                    &Out { valid: true, duration_ms: elapsed_ms, size_bytes: bytes.len() },
                )?;
                println!();
            } else {
                println!("VERIFIED in {elapsed_ms} ms ({} bytes)", bytes.len());
            }
            Ok(())
        }
        Err(e) => Err(anyhow::Error::new(e).context("proof verification failed")),
    }
}

// ─── size ─────────────────────────────────────────────────────────────────

async fn size(args: SizeArgs, globals: Globals) -> anyhow::Result<()> {
    let bytes = std::fs::read(&args.proof).with_context(|| format!("reading {}", args.proof.display()))?;
    let env = ProofEnvelope::deserialize(&bytes).map_err(|e| anyhow!("failed to decode proof: {e}"))?;
    let total = bytes.len();

    if globals.json {
        #[derive(Serialize)]
        struct Out {
            total: usize,
            kind: String,
            chain_headers: Option<usize>,
        }
        let chain_len = match &env.payload {
            ProofPayload::Diamond(dp) => Some(dp.chain.len()),
            _ => None,
        };
        let kind = format!("{:?}", env.kind);
        serde_json::to_writer_pretty(std::io::stdout(), &Out { total, kind, chain_headers: chain_len })?;
        println!();
    } else {
        println!("total              : {total} bytes");
        println!("format version     : {}", env.format_version);
        println!("kind               : {:?}", env.kind);
        if let ProofPayload::Diamond(dp) = &env.payload {
            println!("chain headers      : {}", dp.chain.len());
            println!("anchor             : {}", dp.posterity_anchor);
            println!("target             : {}", dp.target);
            if args.breakdown {
                // Approximate per-section sizes via independent serialization.
                let chain_bytes = borsh::to_vec(&dp.chain)?.len();
                let aux_bytes = borsh::to_vec(&dp.aux_parents)?.len();
                println!("  chain headers    : {chain_bytes} bytes");
                println!("  aux parents      : {aux_bytes} bytes");
                println!("  envelope + fields: {} bytes", total.saturating_sub(chain_bytes + aux_bytes));
            }
        }
    }
    Ok(())
}

// ─── header-stats ─────────────────────────────────────────────────────────

async fn header_stats(args: HeaderStatsArgs, globals: Globals) -> anyhow::Result<()> {
    let bytes = std::fs::read(&args.proof).with_context(|| format!("reading {}", args.proof.display()))?;
    let env = ProofEnvelope::deserialize(&bytes).map_err(|e| anyhow!("failed to decode proof: {e}"))?;

    if env.format_version != kun_core::PROOF_FORMAT_V1 {
        anyhow::bail!("unsupported proof format version: {}", env.format_version);
    }

    // Pull the chain. Only Diamond proofs carry a chain; PoChM/TxR/PoP have
    // a different shape and aren't relevant for v0.1.1's measurement focus.
    let chain: &[AnyHeader] = match &env.payload {
        ProofPayload::Diamond(dp) => &dp.chain,
        _ => anyhow::bail!("header-stats currently only supports diamond proofs"),
    };

    if chain.is_empty() {
        anyhow::bail!("proof's chain is empty; nothing to decompose");
    }

    eprintln!("decomposing {} headers from {} ...", chain.len(), args.proof.display());

    let mut breakdowns: Vec<HeaderSizeBreakdown> = Vec::with_capacity(chain.len());
    for h in chain {
        breakdowns.push(h.field_sizes());
    }

    // Optional CSV — one row per header.
    if let Some(ref out) = args.out {
        write_header_stats_csv(out, &breakdowns).with_context(|| format!("writing {}", out.display()))?;
        eprintln!("CSV: {}", out.display());
    }

    // Aggregate per field.
    let n = breakdowns.len();
    let agg = aggregate_header_stats(&breakdowns);
    let mean_total = agg.totals.mean();

    if globals.json {
        let payload = serde_json::json!({
            "proof_path": args.proof.display().to_string(),
            "chain_length": n,
            "fields": agg.json_value(),
            "mean_total_bytes": mean_total,
        });
        serde_json::to_writer_pretty(std::io::stdout(), &payload)?;
        println!();
        return Ok(());
    }

    // Human table.
    let total_serialized: u64 = breakdowns.iter().map(|b| b.serialized_total as u64).sum();
    println!("# kun proof header-stats");
    println!("proof file        : {}", args.proof.display());
    println!("chain headers     : {n}");
    println!("total header bytes: {total_serialized}");
    println!();
    println!(
        "{:<32} | {:>8} | {:>8} | {:>10} | {:>8} | {:>8} | {:>8}",
        "field", "min", "median", "mean", "p95", "max", "% mean"
    );
    println!("{:-<32} | {:->8} | {:->8} | {:->10} | {:->8} | {:->8} | {:->8}", "", "", "", "", "", "", "");
    for row in agg.rows() {
        let pct = if mean_total > 0.0 { 100.0 * row.stats.mean() / mean_total } else { 0.0 };
        println!(
            "{:<32} | {:>8} | {:>8} | {:>10.1} | {:>8} | {:>8} | {:>7.2}%",
            row.label,
            row.stats.min,
            row.stats.median,
            row.stats.mean(),
            row.stats.p95,
            row.stats.max,
            pct
        );
    }
    println!("{:-<32} | {:->8} | {:->8} | {:->10} | {:->8} | {:->8} | {:->8}", "", "", "", "", "", "", "");
    println!(
        "{:<32} | {:>8} | {:>8} | {:>10.1} | {:>8} | {:>8} | {:>7.2}%",
        "TOTAL (serialized)",
        agg.totals.min,
        agg.totals.median,
        agg.totals.mean(),
        agg.totals.p95,
        agg.totals.max,
        100.0
    );

    // Stripped-header sketch + KIP-6 PoChM projection. log2(N) where
    // N = posterity-window seconds (24 h = 86,400) ≈ 17.
    println!();
    let pbl_mean = agg.parents_by_level.mean();
    let pbl_pct = if mean_total > 0.0 { 100.0 * pbl_mean / mean_total } else { 0.0 };
    println!("dominant field    : parents_by_level ({pbl_mean:.0} bytes mean = {pbl_pct:.1}% of header)");
    let stripped = 32_f64 + 33_f64 + 32_f64; // selected_parent + Some(cached_hash) + future PMR
    println!("stripped-header   : ~{stripped:.0} bytes (selected_parent + cached_hash + post-KIP-6 PMR)");
    let log2n: f64 = 17.0;
    let log2log2n: f64 = log2n.log2();
    let l_a: f64 = 32.0;
    let merkle_term = (2.0 * log2log2n + 1.0) * log2n * l_a;
    let full = merkle_term + mean_total * (log2n - 1.0);
    let strip = merkle_term + stripped * (log2n - 1.0);
    println!();
    println!("KIP-6 PoChM size projection (N=86,400, log2(N)≈17):");
    println!("  with full headers as PMR leaves     : ~{:.0} bytes (~{:.1} KB)", full, full / 1024.0);
    println!("  with stripped headers as PMR leaves : ~{:.0} bytes (~{:.1} KB)", strip, strip / 1024.0);
    println!();
    println!("Caveats: see EMPIRICAL_NOTES.md — these projections assume log2(N)≈17");
    println!("and a single posterity epoch; real PoChM size also depends on the");
    println!("Hamming weight of posterity_depth(B), bounded above by log2(N).");

    Ok(())
}

#[derive(Clone, Copy, Default)]
struct FieldStats {
    min: u64,
    max: u64,
    median: u64,
    p95: u64,
    sum: u64,
    n: u64,
}

impl FieldStats {
    fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum as f64 / self.n as f64
        }
    }

    fn from_slice(values: &mut [usize]) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        values.sort_unstable();
        let n = values.len();
        let min = values[0] as u64;
        let max = values[n - 1] as u64;
        let median = values[n / 2] as u64;
        let p95_idx = ((n.saturating_sub(1)) * 95) / 100;
        let p95 = values[p95_idx] as u64;
        let sum: u64 = values.iter().map(|v| *v as u64).sum();
        Self { min, max, median, p95, sum, n: n as u64 }
    }
}

struct AggregateRow {
    label: &'static str,
    stats: FieldStats,
}

struct HeaderStatsAgg {
    envelope_discriminant: FieldStats,
    cached_hash: FieldStats,
    version: FieldStats,
    parents_by_level: FieldStats,
    hash_merkle_root: FieldStats,
    accepted_id_merkle_root: FieldStats,
    utxo_commitment: FieldStats,
    timestamp: FieldStats,
    bits: FieldStats,
    nonce: FieldStats,
    daa_score: FieldStats,
    blue_work: FieldStats,
    blue_score: FieldStats,
    pruning_point: FieldStats,
    pmr: FieldStats,
    framing_overhead: FieldStats,
    totals: FieldStats,
}

impl HeaderStatsAgg {
    fn rows(&self) -> Vec<AggregateRow> {
        vec![
            AggregateRow { label: "parents_by_level", stats: self.parents_by_level },
            AggregateRow { label: "cached_hash (Option<Hash>)", stats: self.cached_hash },
            AggregateRow { label: "envelope_discriminant", stats: self.envelope_discriminant },
            AggregateRow { label: "version", stats: self.version },
            AggregateRow { label: "hash_merkle_root", stats: self.hash_merkle_root },
            AggregateRow { label: "accepted_id_merkle_root", stats: self.accepted_id_merkle_root },
            AggregateRow { label: "utxo_commitment", stats: self.utxo_commitment },
            AggregateRow { label: "timestamp", stats: self.timestamp },
            AggregateRow { label: "bits", stats: self.bits },
            AggregateRow { label: "nonce", stats: self.nonce },
            AggregateRow { label: "daa_score", stats: self.daa_score },
            AggregateRow { label: "blue_work", stats: self.blue_work },
            AggregateRow { label: "blue_score", stats: self.blue_score },
            AggregateRow { label: "pruning_point", stats: self.pruning_point },
            AggregateRow { label: "pmr (V2 only)", stats: self.pmr },
            AggregateRow { label: "framing overhead", stats: self.framing_overhead },
        ]
    }

    fn json_value(&self) -> serde_json::Value {
        let v = |s: &FieldStats| {
            serde_json::json!({
                "min": s.min, "median": s.median, "mean": s.mean(),
                "p95": s.p95, "max": s.max, "n": s.n,
            })
        };
        serde_json::json!({
            "parents_by_level": v(&self.parents_by_level),
            "cached_hash": v(&self.cached_hash),
            "envelope_discriminant": v(&self.envelope_discriminant),
            "version": v(&self.version),
            "hash_merkle_root": v(&self.hash_merkle_root),
            "accepted_id_merkle_root": v(&self.accepted_id_merkle_root),
            "utxo_commitment": v(&self.utxo_commitment),
            "timestamp": v(&self.timestamp),
            "bits": v(&self.bits),
            "nonce": v(&self.nonce),
            "daa_score": v(&self.daa_score),
            "blue_work": v(&self.blue_work),
            "blue_score": v(&self.blue_score),
            "pruning_point": v(&self.pruning_point),
            "pmr": v(&self.pmr),
            "framing_overhead": v(&self.framing_overhead),
            "totals": v(&self.totals),
        })
    }
}

fn aggregate_header_stats(breakdowns: &[HeaderSizeBreakdown]) -> HeaderStatsAgg {
    let extract = |f: fn(&HeaderSizeBreakdown) -> usize| -> Vec<usize> { breakdowns.iter().map(f).collect::<Vec<_>>() };
    let mut envelope = extract(|b| b.envelope_discriminant);
    let mut cached_hash = extract(|b| b.cached_hash);
    let mut version = extract(|b| b.version);
    let mut parents_by_level = extract(|b| b.parents_by_level);
    let mut hash_merkle_root = extract(|b| b.hash_merkle_root);
    let mut accepted_id_merkle_root = extract(|b| b.accepted_id_merkle_root);
    let mut utxo_commitment = extract(|b| b.utxo_commitment);
    let mut timestamp = extract(|b| b.timestamp);
    let mut bits = extract(|b| b.bits);
    let mut nonce = extract(|b| b.nonce);
    let mut daa_score = extract(|b| b.daa_score);
    let mut blue_work = extract(|b| b.blue_work);
    let mut blue_score = extract(|b| b.blue_score);
    let mut pruning_point = extract(|b| b.pruning_point);
    let mut pmr = extract(|b| b.pmr);
    let mut framing = extract(|b| b.framing_overhead);
    let mut totals = extract(|b| b.serialized_total);

    HeaderStatsAgg {
        envelope_discriminant: FieldStats::from_slice(&mut envelope),
        cached_hash: FieldStats::from_slice(&mut cached_hash),
        version: FieldStats::from_slice(&mut version),
        parents_by_level: FieldStats::from_slice(&mut parents_by_level),
        hash_merkle_root: FieldStats::from_slice(&mut hash_merkle_root),
        accepted_id_merkle_root: FieldStats::from_slice(&mut accepted_id_merkle_root),
        utxo_commitment: FieldStats::from_slice(&mut utxo_commitment),
        timestamp: FieldStats::from_slice(&mut timestamp),
        bits: FieldStats::from_slice(&mut bits),
        nonce: FieldStats::from_slice(&mut nonce),
        daa_score: FieldStats::from_slice(&mut daa_score),
        blue_work: FieldStats::from_slice(&mut blue_work),
        blue_score: FieldStats::from_slice(&mut blue_score),
        pruning_point: FieldStats::from_slice(&mut pruning_point),
        pmr: FieldStats::from_slice(&mut pmr),
        framing_overhead: FieldStats::from_slice(&mut framing),
        totals: FieldStats::from_slice(&mut totals),
    }
}

#[derive(Serialize)]
struct CsvRow {
    index: usize,
    envelope_discriminant: usize,
    cached_hash: usize,
    version: usize,
    parents_by_level: usize,
    hash_merkle_root: usize,
    accepted_id_merkle_root: usize,
    utxo_commitment: usize,
    timestamp: usize,
    bits: usize,
    nonce: usize,
    daa_score: usize,
    blue_work: usize,
    blue_score: usize,
    pruning_point: usize,
    pmr: usize,
    framing_overhead: usize,
    serialized_total: usize,
}

fn write_header_stats_csv(out: &PathBuf, breakdowns: &[HeaderSizeBreakdown]) -> anyhow::Result<()> {
    let file = std::fs::File::create(out)?;
    let mut wtr = csv::Writer::from_writer(file);
    for (i, b) in breakdowns.iter().enumerate() {
        wtr.serialize(CsvRow {
            index: i,
            envelope_discriminant: b.envelope_discriminant,
            cached_hash: b.cached_hash,
            version: b.version,
            parents_by_level: b.parents_by_level,
            hash_merkle_root: b.hash_merkle_root,
            accepted_id_merkle_root: b.accepted_id_merkle_root,
            utxo_commitment: b.utxo_commitment,
            timestamp: b.timestamp,
            bits: b.bits,
            nonce: b.nonce,
            daa_score: b.daa_score,
            blue_work: b.blue_work,
            blue_score: b.blue_score,
            pruning_point: b.pruning_point,
            pmr: b.pmr,
            framing_overhead: b.framing_overhead,
            serialized_total: b.serialized_total,
        })?;
    }
    wtr.flush()?;
    Ok(())
}

// ─── posterity file management (TOFU) ─────────────────────────────────────

fn posterity_path(ctx: &Ctx) -> PathBuf {
    ctx.data_dir.join("posterity.borsh")
}

async fn ensure_posterity_file(ctx: &Ctx) -> anyhow::Result<()> {
    let path = posterity_path(ctx);
    if path.exists() {
        return Ok(());
    }
    let trusted = bootstrap_posterity(ctx).await?;
    std::fs::create_dir_all(&ctx.data_dir).ok();
    std::fs::write(&path, trusted.serialize())?;
    tracing::info!(path=%path.display(), "initialized posterity chain (TOFU)");
    Ok(())
}

async fn bootstrap_posterity(ctx: &Ctx) -> anyhow::Result<PosterityChain> {
    let dag = ctx.rpc.get_block_dag_info().await?;
    let pp_header = ctx.rpc.get_block_header(dag.pruning_point).await?;
    let entry = PosterityHeader { header: pp_header, posterity_depth3: dag.pruning_point, chain_index: 0 };
    let mut chain = PosterityChain::new();
    chain.extend(&*ctx.hs, entry)?;
    Ok(chain)
}

async fn load_or_init_posterity_for_envelope(
    ctx: &Ctx,
    env: &kun_core::ProofEnvelope,
    against: Option<&str>,
    trust_anchor: bool,
) -> anyhow::Result<PosterityChain> {
    // If the caller supplied a specific anchor, trust it (optionally) — and
    // pull the header from the proof itself so verification works even if
    // the node has pruned past that posterity.
    if let Some(raw) = against {
        let anchor: Hash = raw.parse().context("parse --against as hex hash")?;
        return build_adhoc_posterity_from_envelope(ctx, env, anchor, trust_anchor).await;
    }
    // Otherwise use the on-disk TOFU posterity file.
    let path = posterity_path(ctx);
    if path.exists() {
        let bytes = std::fs::read(&path)?;
        return PosterityChain::deserialize(&bytes)
            .map_err(|e| anyhow!("corrupt posterity chain at {}: {e}", path.display()));
    }
    ensure_posterity_file(ctx).await?;
    let bytes = std::fs::read(&path)?;
    PosterityChain::deserialize(&bytes).map_err(|e| anyhow!("corrupt posterity chain at {}: {e}", path.display()))
}

async fn build_adhoc_posterity_from_envelope(
    ctx: &Ctx,
    env: &kun_core::ProofEnvelope,
    anchor: Hash,
    trust_anchor: bool,
) -> anyhow::Result<PosterityChain> {
    if !trust_anchor {
        anyhow::bail!("--against <hash> requires --trust-anchor (or sync a local posterity chain first)");
    }

    // Preferred path: extract the anchor header from the proof itself. A
    // DiamondProof carries `chain[0]` as the anchor header, so we don't
    // need the node to still have that block — which it may have pruned.
    let header_from_proof: Option<&kun_core::AnyHeader> = match &env.payload {
        kun_core::ProofPayload::Diamond(dp) => dp.chain.first(),
        kun_core::ProofPayload::Pochm(_) | kun_core::ProofPayload::TxReceipt(_) | kun_core::ProofPayload::Pop(_) => {
            None
        }
    };

    let header = if let Some(h) = header_from_proof {
        // Confirm the proof's own chain[0] hashes to the anchor we claim.
        if h.hash(&*ctx.hs) != anchor {
            anyhow::bail!("--against hash does not match the proof's internal anchor (chain[0]); refusing to trust");
        }
        h.clone()
    } else {
        // Fall back to fetching from the node (works only if unpruned).
        ctx.rpc.get_block_header(anchor).await.with_context(|| format!("fetching anchor {anchor}"))?
    };

    let entry = PosterityHeader { header, posterity_depth3: anchor, chain_index: 0 };
    let mut chain = PosterityChain::new();
    chain.extend(&*ctx.hs, entry)?;
    Ok(chain)
}

// Suppress unused-warning for a crate-private helper type we may wire up.
#[allow(dead_code)]
fn _noop_hash_scheme(_hs: &dyn HashScheme) {}
#[allow(dead_code)]
fn _noop_header(_h: AnyHeader) {}
#[allow(dead_code)]
fn _noop_diamond(_dp: DiamondProof) {}
#[allow(dead_code)]
fn _noop_kind(_k: Kind) {}
#[allow(dead_code)]
fn _noop_from_str(_s: &str) {
    let _ = Hash::from_str(_s);
}
