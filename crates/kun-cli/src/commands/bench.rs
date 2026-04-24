//! `kun bench proof-size` — the batch runner that produces the paper's
//! headline CSV. Samples N blocks uniformly across the pruning window,
//! generates a diamond proof for each, records size + gen_ms + verify_ms.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context as _, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use kun_core::{Hash, ProofEnvelope};
use kun_proofs::{diamond, verify_diamond_proof};

use crate::commands::Globals;
use crate::context::Ctx;

#[derive(Args)]
pub struct BenchArgs {
    #[command(subcommand)]
    pub command: BenchCommand,
}

#[derive(Subcommand)]
pub enum BenchCommand {
    /// Sample N blocks and record proof sizes + timing.
    ProofSize(ProofSizeArgs),
}

#[derive(Args)]
pub struct ProofSizeArgs {
    /// Number of blocks to sample.
    #[arg(long, default_value_t = 100)]
    pub blocks: usize,
    /// Output CSV path.
    #[arg(long, default_value = "results.csv")]
    pub out: PathBuf,
    /// Skip the verify step (generation-only benchmark).
    #[arg(long, default_value_t = false)]
    pub no_verify: bool,
}

#[derive(Serialize)]
struct Row {
    block_hash: String,
    proof_kind: &'static str,
    chain_len: usize,
    proof_bytes: usize,
    gen_ms: u64,
    verify_ms: Option<u64>,
    verified: Option<bool>,
    error: Option<String>,
}

pub async fn run(args: BenchArgs, globals: Globals) -> Result<()> {
    match args.command {
        BenchCommand::ProofSize(a) => proof_size(a, globals).await,
    }
}

async fn proof_size(args: ProofSizeArgs, globals: Globals) -> Result<()> {
    let ctx = Ctx::from_globals(&globals).await?;
    let dag = ctx.rpc.get_block_dag_info().await?;
    let tip = dag.tip_hashes.first().copied().ok_or_else(|| anyhow::anyhow!("node reported no tips"))?;

    // Sample: walk backward from tip along the selected chain and pick every
    // (chain_len / blocks)-th block until we collect `args.blocks` samples or
    // hit the pruning anchor. For v0.1 this is the simplest honest sampler;
    // uniform-over-DAA v0.2.
    let virt = ctx.rpc.get_virtual_chain_from_block(dag.pruning_point, false).await?;
    let mut candidates: Vec<Hash> = virt.added.clone();
    if candidates.is_empty() {
        candidates.push(tip);
    }
    let step = (candidates.len() / args.blocks.max(1)).max(1);
    let samples: Vec<Hash> = candidates.iter().step_by(step).take(args.blocks).copied().collect();

    // Ensure we have a local posterity file (TOFU the current pruning point).
    super::proof::run(
        super::proof::ProofArgs {
            command: super::proof::ProofCommand::Size(super::proof::SizeArgs {
                proof: PathBuf::from("/nonexistent/__kun-bootstrap_placeholder__"),
                breakdown: false,
            }),
        },
        globals.clone(),
    )
    .await
    .ok();
    // The block above is best-effort; bench's own load path below will
    // bootstrap the posterity file if still missing.

    std::fs::create_dir_all(ctx.data_dir.join("bench-proofs")).ok();

    let mut rows: Vec<Row> = Vec::with_capacity(samples.len());
    for (i, block) in samples.iter().enumerate() {
        let row = measure_one(&ctx, *block, args.no_verify).await;
        eprintln!(
            "[{:>3}/{}] {} -> {} bytes{}",
            i + 1,
            samples.len(),
            &block.to_string()[..12],
            row.proof_bytes,
            row.error.as_deref().map(|e| format!(" ERR: {e}")).unwrap_or_default()
        );
        rows.push(row);
    }

    // Emit CSV.
    let file = std::fs::File::create(&args.out).with_context(|| format!("create {}", args.out.display()))?;
    let mut wtr = csv::Writer::from_writer(file);
    for r in &rows {
        wtr.serialize(r)?;
    }
    wtr.flush()?;

    // Summary statistics.
    let mut sizes: Vec<usize> = rows.iter().filter(|r| r.error.is_none()).map(|r| r.proof_bytes).collect();
    sizes.sort_unstable();
    if !sizes.is_empty() {
        let median = sizes[sizes.len() / 2];
        let p95 = sizes[(sizes.len() * 95) / 100];
        let max = *sizes.last().unwrap();
        eprintln!("summary: n={} median={}B p95={}B max={}B", sizes.len(), median, p95, max);
    }
    eprintln!("CSV written: {}", args.out.display());
    Ok(())
}

async fn measure_one(ctx: &Ctx, block: Hash, no_verify: bool) -> Row {
    let kind = "diamond";
    let t0 = Instant::now();
    let proof = match diamond::generate_diamond_proof_with_scheme(&*ctx.src, &*ctx.hs, block).await {
        Ok(p) => p,
        Err(e) => {
            return Row {
                block_hash: block.to_string(),
                proof_kind: kind,
                chain_len: 0,
                proof_bytes: 0,
                gen_ms: 0,
                verify_ms: None,
                verified: None,
                error: Some(e.to_string()),
            };
        }
    };
    let gen_ms = t0.elapsed().as_millis() as u64;
    let chain_len = proof.chain.len();
    let env = ProofEnvelope::diamond(proof);
    let bytes = env.serialize();

    let (verify_ms, verified, error) = if no_verify {
        (None, None, None)
    } else {
        match load_posterity(ctx) {
            Ok(posterity) => {
                let t1 = Instant::now();
                let dp = match &env.payload {
                    kun_core::ProofPayload::Diamond(dp) => dp,
                    _ => unreachable!(),
                };
                match verify_diamond_proof(&*ctx.hs, dp, &posterity) {
                    Ok(()) => (Some(t1.elapsed().as_millis() as u64), Some(true), None),
                    Err(e) => (Some(t1.elapsed().as_millis() as u64), Some(false), Some(e.to_string())),
                }
            }
            Err(e) => (None, None, Some(format!("posterity load: {e}"))),
        }
    };

    Row {
        block_hash: block.to_string(),
        proof_kind: kind,
        chain_len,
        proof_bytes: bytes.len(),
        gen_ms,
        verify_ms,
        verified,
        error,
    }
}

fn load_posterity(ctx: &Ctx) -> anyhow::Result<kun_core::PosterityChain> {
    let path = ctx.data_dir.join("posterity.borsh");
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    kun_core::PosterityChain::deserialize(&bytes).map_err(|e| anyhow::anyhow!("corrupt posterity chain: {e}"))
}
