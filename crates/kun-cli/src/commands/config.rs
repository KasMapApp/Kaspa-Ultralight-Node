use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::config::{self, Config};

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,

    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Print a key's value.
    Get { key: String },
    /// Set a key to a value.
    Set { key: String, value: String },
    /// Show the entire configuration.
    List,
}

pub fn run(args: ConfigArgs) -> Result<()> {
    let data_dir = config::data_dir_override(args.data_dir.as_deref())?;
    let mut cfg = config::load(&data_dir).context("loading config")?;

    match args.command {
        ConfigCommand::Get { key } => match key.as_str() {
            "default_node" => println!("{}", cfg.default_node),
            "network" => println!("{}", cfg.network),
            "hash_era" => println!("{:?}", cfg.hash_era),
            other => anyhow::bail!("unknown key: {other}"),
        },
        ConfigCommand::Set { key, value } => {
            match key.as_str() {
                "default_node" => cfg.default_node = value,
                "network" => cfg.network = value,
                "hash_era" => {
                    use crate::config::HashEraSetting;
                    cfg.hash_era = match value.to_ascii_lowercase().as_str() {
                        "auto" => HashEraSetting::Auto,
                        "blake2b" | "blake2" => HashEraSetting::Blake2b,
                        "blake3" => HashEraSetting::Blake3,
                        other => anyhow::bail!("unknown hash_era: {other} (auto|blake2b|blake3)"),
                    };
                }
                other => anyhow::bail!("unknown key: {other}"),
            }
            config::save(&data_dir, &cfg)?;
            println!("ok");
        }
        ConfigCommand::List => {
            print_cfg(&cfg);
        }
    }
    Ok(())
}

fn print_cfg(cfg: &Config) {
    println!("default_node  : {}", cfg.default_node);
    println!("network       : {}", cfg.network);
    println!("hash_era      : {:?}", cfg.hash_era);
}
