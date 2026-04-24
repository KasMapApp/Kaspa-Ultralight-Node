//! `kun` — Kaspa Ultralight Node CLI.

use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod commands;
mod config;
mod context;
mod exit;
mod output;

use commands::{Command, TopLevel};

fn main() -> ExitCode {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("failed to build Tokio runtime");
    runtime.block_on(async_main())
}

async fn async_main() -> ExitCode {
    let cli = TopLevel::parse();
    init_tracing(cli.verbosity);

    match run(cli).await {
        Ok(()) => ExitCode::from(exit::OK),
        Err(e) => {
            let code = exit::classify(&e);
            eprintln!("error: {e:#}");
            ExitCode::from(code)
        }
    }
}

async fn run(cli: TopLevel) -> anyhow::Result<()> {
    match cli.command {
        Command::Dag(c) => commands::dag::run(c, cli.globals).await,
        Command::Proof(c) => commands::proof::run(c, cli.globals).await,
        Command::Bench(c) => commands::bench::run(c, cli.globals).await,
        Command::Config(c) => commands::config::run(c).context("config command"),
    }
}

fn init_tracing(verbosity: u8) {
    let default_level = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_env("KUN_LOG").unwrap_or_else(|_| EnvFilter::new(format!("kun={default_level}")));
    tracing_subscriber::fmt().with_env_filter(filter).with_target(false).with_writer(std::io::stderr).init();
}
