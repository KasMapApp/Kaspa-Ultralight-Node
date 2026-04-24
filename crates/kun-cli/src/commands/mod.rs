//! `kun` subcommands.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

pub mod bench;
pub mod config;
pub mod dag;
pub mod proof;

#[derive(Parser)]
#[command(
    name    = "kun",
    author,
    version,
    about  = "Kaspa Ultralight Node — proof-of-concept CLI",
    long_about = "Generates and verifies Kaspa PoChM proofs. v0.1 ships the pre-KIP-6 diamond-DAG baseline."
)]
pub struct TopLevel {
    #[command(flatten)]
    pub globals: Globals,

    /// Verbosity: -v=info, -vv=debug, -vvv=trace.
    #[arg(short = 'v', action = clap::ArgAction::Count, global = true)]
    pub verbosity: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Clone)]
pub struct Globals {
    /// Kaspa node wRPC URL (e.g. ws://localhost:17110).
    #[arg(long, global = true)]
    pub node: Option<String>,

    /// Network: mainnet, testnet-10, devnet, simnet.
    #[arg(long, global = true)]
    pub network: Option<String>,

    /// Override the data directory (default: OS-conventional under ~/.kun).
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,

    /// Emit machine-readable JSON instead of human text.
    #[arg(long, global = true, default_value_t = false)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Query the DAG state of the connected node.
    Dag(dag::DagArgs),
    /// Generate, verify, and inspect proofs.
    Proof(proof::ProofArgs),
    /// Batch-run proof generation to produce paper-ready CSVs.
    Bench(bench::BenchArgs),
    /// Manage `~/.kun/config.json`.
    Config(config::ConfigArgs),
}
