use clap::{Args, Subcommand};
use serde::Serialize;

use crate::commands::Globals;
use crate::context::Ctx;

#[derive(Args)]
pub struct DagArgs {
    #[command(subcommand)]
    pub command: DagCommand,
}

#[derive(Subcommand)]
pub enum DagCommand {
    /// Print a DAG snapshot (tip, pruning point, BPS, ...).
    Info,
}

#[derive(Serialize)]
struct InfoOutput {
    network: String,
    tip: String,
    /// Virtual's selected parent — guaranteed to be on the selected chain.
    /// Use THIS as the target for `kun proof generate`, not `tip`.
    sink: String,
    pruning_point: String,
    virtual_daa_score: u64,
    block_count: u64,
    header_count: u64,
    tip_count: usize,
    server_version: Option<String>,
    is_synced: Option<bool>,
}

pub async fn run(args: DagArgs, globals: Globals) -> anyhow::Result<()> {
    let ctx = Ctx::from_globals(&globals).await?;
    match args.command {
        DagCommand::Info => {
            let info = ctx.rpc.get_block_dag_info().await?;
            let server = ctx.rpc.get_server_info().await.ok();

            let tip = info.tip_hashes.first().map(|h| h.to_string()).unwrap_or_else(|| "(none)".into());

            let out = InfoOutput {
                network: info.network,
                tip,
                sink: info.sink.to_string(),
                pruning_point: info.pruning_point.to_string(),
                virtual_daa_score: info.virtual_daa_score,
                block_count: info.block_count,
                header_count: info.header_count,
                tip_count: info.tip_hashes.len(),
                server_version: server.as_ref().map(|s| s.server_version.clone()),
                is_synced: server.as_ref().map(|s| s.is_synced),
            };

            if globals.json {
                serde_json::to_writer_pretty(std::io::stdout(), &out)?;
                println!();
            } else {
                println!("network            : {}", out.network);
                println!("tip                : {}", out.tip);
                println!("  ({} total tips)", out.tip_count);
                println!("sink               : {}", out.sink);
                println!("  (= virtual's selected parent; use as --block target)");
                println!("pruning point      : {}", out.pruning_point);
                println!("virtual DAA score  : {}", out.virtual_daa_score);
                println!("block count        : {}", out.block_count);
                println!("header count       : {}", out.header_count);
                if let Some(v) = &out.server_version {
                    println!("server version     : {v}");
                }
                if let Some(s) = out.is_synced {
                    println!("synced             : {s}");
                }
            }
            Ok(())
        }
    }
}
