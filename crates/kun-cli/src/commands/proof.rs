use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{anyhow, Context as _};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use kun_core::{
    AnyHeader, DiamondProof, Hash, HashScheme, PosterityChain, PosterityHeader, ProofEnvelope, ProofKind, ProofPayload,
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
        let (chosen, chain) = ctx
            .src
            .current_selected_chain(args.safety_depth)
            .await
            .context("auto-picking selected-chain target")?;
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
                serde_json::to_writer_pretty(std::io::stdout(), &Out { valid: true, duration_ms: elapsed_ms, size_bytes: bytes.len() })?;
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
    let entry = PosterityHeader {
        header: pp_header,
        posterity_depth3: dag.pruning_point,
        chain_index: 0,
    };
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
        kun_core::ProofPayload::Pochm(_) | kun_core::ProofPayload::TxReceipt(_) | kun_core::ProofPayload::Pop(_) => None,
    };

    let header = if let Some(h) = header_from_proof {
        // Confirm the proof's own chain[0] hashes to the anchor we claim.
        if h.hash(&*ctx.hs) != anchor {
            anyhow::bail!(
                "--against hash does not match the proof's internal anchor (chain[0]); refusing to trust"
            );
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
