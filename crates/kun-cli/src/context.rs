//! Connection and hash-scheme bootstrap shared by every subcommand.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;

use kun_core::{Blake2bScheme, Blake3Scheme, HashEra, HashScheme};
use kun_proof_source::RpcProofSource;
use kun_rpc::{KunRpc, KunRpcConfig, NetworkId, NetworkType};

use crate::commands::Globals;
use crate::config::{self, Config, HashEraSetting};

pub struct Ctx {
    pub rpc: KunRpc,
    pub hs: Arc<dyn HashScheme>,
    pub src: Arc<RpcProofSource>,
    pub data_dir: PathBuf,
    pub cfg: Config,
}

impl Ctx {
    pub async fn from_globals(globals: &Globals) -> anyhow::Result<Self> {
        let data_dir = config::data_dir_override(globals.data_dir.as_deref())?;
        std::fs::create_dir_all(&data_dir).ok();
        let mut cfg = config::load(&data_dir).context("loading configuration")?;

        if let Some(node) = &globals.node {
            cfg.default_node = node.clone();
        }
        if let Some(net) = &globals.network {
            cfg.network = net.clone();
        }

        let network = network_from_str(&cfg.network)?;
        let rpc = KunRpc::connect(KunRpcConfig {
            url: Some(cfg.default_node.clone()),
            network,
            encoding: kun_rpc::WrpcEncoding::Borsh,
            connect_timeout: std::time::Duration::from_millis(15_000),
            use_resolver: false,
            retry_max: 5,
            retry_backoff: std::time::Duration::from_millis(500),
        })
        .await
        .with_context(|| format!("connecting to {}", cfg.default_node))?;

        let hs = resolve_hash_scheme(&rpc, &cfg.hash_era).await?;
        let src = Arc::new(RpcProofSource::new(rpc.clone(), hs.clone()));

        Ok(Self { rpc, hs, src, data_dir, cfg })
    }
}

fn network_from_str(s: &str) -> anyhow::Result<NetworkId> {
    let ty = match s.to_ascii_lowercase().as_str() {
        "mainnet" => NetworkType::Mainnet,
        "testnet-10" | "testnet10" | "testnet" => NetworkType::Testnet,
        "devnet" => NetworkType::Devnet,
        "simnet" => NetworkType::Simnet,
        other => anyhow::bail!("unknown network `{}`; expected mainnet/testnet-10/devnet/simnet", other),
    };
    Ok(NetworkId::new(ty))
}

async fn resolve_hash_scheme(rpc: &KunRpc, setting: &HashEraSetting) -> anyhow::Result<Arc<dyn HashScheme>> {
    let era = match setting {
        HashEraSetting::Blake2b => HashEra::Blake2b,
        HashEraSetting::Blake3 => HashEra::Blake3,
        HashEraSetting::Auto => {
            // Phase-0 note: until we confirm the detection heuristic, default
            // to Blake2B. The server version string is exposed; once we
            // agree on the cutover, plug real logic in here.
            let _info = rpc.get_server_info().await.ok();
            HashEra::Blake2b
        }
    };
    Ok(match era {
        HashEra::Blake2b => Arc::new(Blake2bScheme),
        HashEra::Blake3 => Arc::new(Blake3Scheme),
    })
}
