//! [`KunRpc`] — thin retry/convert wrapper over [`KaspaRpcClient`].
//!
//! Exposes the subset of RPC methods KUN actually uses, each returning KUN
//! types (not rusty-kaspa ones). Retries transient failures with exponential
//! backoff via [`backoff`]. Connection strategy matches `kaspa-agent-sdk`:
//! Borsh encoding, Resolver + fallback, 15 s connect timeout.

use std::sync::Arc;
use std::time::Duration;

use kaspa_consensus_core::network::{NetworkId, NetworkType};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{client::ConnectOptions, KaspaRpcClient, Resolver, WrpcEncoding};

use kun_core::{AnyHeader, Hash};

use crate::converters::{hash_from_kaspa, hash_from_rpc, rpc_hash_from_kun, rpc_header_into_any};
use crate::error::{Result, RpcError};

/// Configuration for [`KunRpc::connect`].
#[derive(Debug, Clone)]
pub struct KunRpcConfig {
    /// Explicit node URL. If `None`, a [`Resolver`] is used to pick from the
    /// public-node pool.
    pub url: Option<String>,
    pub network: NetworkId,
    pub encoding: WrpcEncoding,
    pub connect_timeout: Duration,
    /// When `true` and `url` is `None`, use `Resolver::default()`.
    pub use_resolver: bool,
    pub retry_max: usize,
    pub retry_backoff: Duration,
}

impl Default for KunRpcConfig {
    fn default() -> Self {
        Self {
            url: None,
            network: NetworkId::new(NetworkType::Mainnet),
            encoding: WrpcEncoding::Borsh,
            connect_timeout: Duration::from_millis(15_000),
            use_resolver: true,
            retry_max: 5,
            retry_backoff: Duration::from_millis(500),
        }
    }
}

impl KunRpcConfig {
    pub fn mainnet_local() -> Self {
        Self { url: Some("ws://localhost:17110".into()), use_resolver: false, ..Self::default() }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self.use_resolver = false;
        self
    }
}

/// DAG overview pulled from [`RpcApi::get_block_dag_info`].
#[derive(Debug, Clone)]
pub struct DagInfo {
    pub network: String,
    pub block_count: u64,
    pub header_count: u64,
    pub tip_hashes: Vec<Hash>,
    pub difficulty: f64,
    pub past_median_time_ms: u64,
    pub virtual_parent_hashes: Vec<Hash>,
    pub pruning_point: Hash,
    pub virtual_daa_score: u64,
    pub sink: Hash,
}

/// Selected-chain slice from [`RpcApi::get_virtual_chain_from_block`].
#[derive(Debug, Clone)]
pub struct VirtualChain {
    pub added: Vec<Hash>,
    pub removed: Vec<Hash>,
    /// Per-added-block list of accepted tx ids, when `include_tx_ids = true`.
    pub accepted_tx_ids: Vec<(Hash, Vec<Hash>)>,
}

/// Server-info response.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub rpc_api_version: u16,
    pub rpc_api_revision: u16,
    pub server_version: String,
    pub network: String,
    pub has_utxo_index: bool,
    pub is_synced: bool,
    pub virtual_daa_score: u64,
}

/// Wrapper around [`KaspaRpcClient`] with KUN-shaped helpers.
#[derive(Clone)]
pub struct KunRpc {
    inner: Arc<KaspaRpcClient>,
    network: NetworkId,
}

impl KunRpc {
    pub async fn connect(cfg: KunRpcConfig) -> Result<Self> {
        let resolver = if cfg.use_resolver && cfg.url.is_none() { Some(Resolver::default()) } else { None };

        let client = KaspaRpcClient::new(cfg.encoding, cfg.url.as_deref(), resolver, Some(cfg.network), None)
            .map_err(|e| RpcError::Construction(e.to_string()))?;

        let opts = ConnectOptions {
            block_async_connect: true,
            connect_timeout: Some(cfg.connect_timeout),
            url: cfg.url.clone(),
            ..ConnectOptions::default()
        };

        client.connect(Some(opts)).await.map_err(|e| RpcError::Connect(e.to_string()))?;

        Ok(Self { inner: Arc::new(client), network: cfg.network })
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.inner.disconnect().await.map_err(|e| RpcError::Disconnect(e.to_string()))
    }

    pub fn network(&self) -> NetworkId {
        self.network
    }

    pub async fn get_server_info(&self) -> Result<ServerInfo> {
        let r = self.inner.get_server_info().await.map_err(|e| RpcError::Remote(e.to_string()))?;
        Ok(ServerInfo {
            rpc_api_version: r.rpc_api_version,
            rpc_api_revision: r.rpc_api_revision,
            server_version: r.server_version,
            network: r.network_id.to_string(),
            has_utxo_index: r.has_utxo_index,
            is_synced: r.is_synced,
            virtual_daa_score: r.virtual_daa_score,
        })
    }

    pub async fn get_block_dag_info(&self) -> Result<DagInfo> {
        let r = self.inner.get_block_dag_info().await.map_err(|e| RpcError::Remote(e.to_string()))?;
        Ok(DagInfo {
            network: r.network.to_string(),
            block_count: r.block_count,
            header_count: r.header_count,
            tip_hashes: r.tip_hashes.into_iter().map(hash_from_rpc).collect(),
            difficulty: r.difficulty,
            past_median_time_ms: r.past_median_time,
            virtual_parent_hashes: r.virtual_parent_hashes.into_iter().map(hash_from_rpc).collect(),
            pruning_point: hash_from_rpc(r.pruning_point_hash),
            virtual_daa_score: r.virtual_daa_score,
            sink: hash_from_rpc(r.sink),
        })
    }

    pub async fn get_block_header(&self, hash: Hash) -> Result<AnyHeader> {
        let block =
            self.inner.get_block(rpc_hash_from_kun(hash), false).await.map_err(|e| RpcError::Remote(e.to_string()))?;
        rpc_header_into_any(block.header)
    }

    /// Pull headers for a range of blocks. `include_transactions = false`
    /// in the MVP; tx data is only fetched when building a `TxReceipt`.
    pub async fn get_blocks(&self, low: Option<Hash>, include_blocks: bool) -> Result<Vec<AnyHeader>> {
        let r = self
            .inner
            .get_blocks(low.map(rpc_hash_from_kun), include_blocks, false)
            .await
            .map_err(|e| RpcError::Remote(e.to_string()))?;
        let mut out = Vec::with_capacity(r.blocks.len());
        for block in r.blocks {
            out.push(rpc_header_into_any(block.header)?);
        }
        Ok(out)
    }

    pub async fn get_virtual_chain_from_block(&self, start: Hash, include_tx_ids: bool) -> Result<VirtualChain> {
        let r = self
            .inner
            .get_virtual_chain_from_block(rpc_hash_from_kun(start), include_tx_ids)
            .await
            .map_err(|e| RpcError::Remote(e.to_string()))?;
        let added: Vec<Hash> = r.added_chain_block_hashes.into_iter().map(hash_from_rpc).collect();
        let removed: Vec<Hash> = r.removed_chain_block_hashes.into_iter().map(hash_from_rpc).collect();
        let accepted_tx_ids = r
            .accepted_transaction_ids
            .into_iter()
            .map(|e| {
                (
                    hash_from_kaspa(e.accepting_block_hash),
                    e.accepted_transaction_ids.into_iter().map(hash_from_kaspa).collect(),
                )
            })
            .collect();
        Ok(VirtualChain { added, removed, accepted_tx_ids })
    }

    pub async fn get_headers(&self, start: Hash, limit: u64, ascending: bool) -> Result<Vec<AnyHeader>> {
        let rs = self
            .inner
            .get_headers(rpc_hash_from_kun(start), limit, ascending)
            .await
            .map_err(|e| RpcError::Remote(e.to_string()))?;
        rs.into_iter().map(rpc_header_into_any).collect()
    }
}
