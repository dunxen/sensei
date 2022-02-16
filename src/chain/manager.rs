use std::{sync::{Arc, Mutex as SyncMutex}, time::Duration};

use crate::{config::{LightningNodeBackendConfig, BitcoindConfig, SenseiConfig}, node::{ChannelManager, ChainMonitor, SyncableMonitor}};
use bitcoin::{Txid, Script, Transaction, BlockHash, Block, hashes::hex::FromHex};
use lightning::chain::{BestBlock, WatchedOutput, Filter, self, Confirm};
use electrum_client::{ConfigBuilder, ElectrumApi, Socks5Config};
use lightning_block_sync::{http::{HttpEndpoint, JsonResponse}, SpvClient};
use lightning_block_sync::rpc::RpcClient;
use lightning_block_sync::{AsyncBlockSourceResult, BlockHeaderData, BlockSource, init, poll, UnboundedCache};
use tokio::sync::Mutex;
use std::ops::Deref;

use super::listener::SenseiChainListener;

pub type TransactionWithHeight = (u32, Transaction);
pub type TransactionWithPosition = (usize, Transaction);
pub type TransactionWithHeightAndPosition = (u32, Transaction, usize);

pub struct BlockchainInfo {
	pub latest_height: usize,
	pub latest_blockhash: BlockHash,
	pub chain: String,
}

impl TryInto<BlockchainInfo> for JsonResponse {
	type Error = std::io::Error;
	fn try_into(self) -> std::io::Result<BlockchainInfo> {
		Ok(BlockchainInfo {
			latest_height: self.0["blocks"].as_u64().unwrap() as usize,
			latest_blockhash: BlockHash::from_hex(self.0["bestblockhash"].as_str().unwrap())
				.unwrap(),
			chain: self.0["chain"].as_str().unwrap().to_string(),
		})
	}
}

struct BitcoindClient {
    rpc_client: Arc<Mutex<RpcClient>>
}

impl BitcoindClient {
    pub fn new(config: &BitcoindConfig) -> Result<Self, crate::error::Error> {
        let http_endpoint = HttpEndpoint::for_host(config.host.clone()).with_port(config.port);
		let rpc_credentials =
			base64::encode(format!("{}:{}", config.rpc_username.clone(), config.rpc_password.clone()));
		let mut rpc_client = RpcClient::new(&rpc_credentials, http_endpoint)?;

        Ok(Self {
            rpc_client: Arc::new(Mutex::new(rpc_client))
        })
    }

    pub async fn get_blockchain_info(&self) -> Result<BlockchainInfo, std::io::Error> {
        let mut rpc = self.rpc_client.lock().await;
        rpc.call_method::<BlockchainInfo>("getblockchaininfo", &vec![]).await
    }
}

impl BlockSource for &BitcoindClient {
	fn get_header<'a>(
		&'a mut self, header_hash: &'a BlockHash, height_hint: Option<u32>,
	) -> AsyncBlockSourceResult<'a, BlockHeaderData> {
		Box::pin(async move {
			let mut rpc = self.rpc_client.lock().await;
			rpc.get_header(header_hash, height_hint).await
		})
	}

	fn get_block<'a>(
		&'a mut self, header_hash: &'a BlockHash,
	) -> AsyncBlockSourceResult<'a, Block> {
		Box::pin(async move {
			let mut rpc = self.rpc_client.lock().await;
			rpc.get_block(header_hash).await
		})
	}

	fn get_best_block<'a>(&'a mut self) -> AsyncBlockSourceResult<(BlockHash, Option<u32>)> {
		Box::pin(async move {
			let mut rpc = self.rpc_client.lock().await;
			rpc.get_best_block().await
		})
	}
}

pub struct SenseiChainManager {
    config: SenseiConfig,
    listener: Arc<SenseiChainListener>,
    filter: SyncMutex<TxFilter>,
    block_source: Option<Arc<BitcoindClient>>,
    cache: UnboundedCache,
}

impl SenseiChainManager {
    pub fn new(config: SenseiConfig) -> Result<Self, crate::error::Error> {
        let listener = Arc::new(SenseiChainListener::new());

        let block_source = match &config.backend {
            LightningNodeBackendConfig::Bitcoind(bitcoin_config) => {
                let block_source = Arc::new(BitcoindClient::new(&bitcoin_config)?);        
                let block_source_poller = block_source.clone();
                let listener_poller = listener.clone();
                tokio::spawn(async move {
                    let mut derefed = block_source_poller.deref();
                    let mut cache = UnboundedCache::new();
                    let chain_tip = init::validate_best_block_header(&mut derefed).await.unwrap();
                    let chain_poller = poll::ChainPoller::new(&mut derefed, config.network);
                    let mut spv_client =
                        SpvClient::new(chain_tip, chain_poller, &mut cache, listener_poller);
                    loop {
                        spv_client.poll_best_tip().await.unwrap();
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                });
                
                Some(block_source)
            },
            _ => None
        };

        Ok(Self { 
            config, 
            filter: SyncMutex::new(TxFilter::new()), 
            listener,
            cache: UnboundedCache::new(),
            block_source
        })
    }

    pub async fn synchronize_to_tip(&self, syncable_channel_manager: (BlockHash, &mut ChannelManager), syncable_channel_monitors: Vec<(BlockHash, &mut SyncableMonitor)>) -> Result<(), crate::error::Error> {
        match &self.config.backend {
            LightningNodeBackendConfig::Bitcoind(config) => {
                let block_source = self.block_source.as_ref().unwrap();
                let mut chain_listeners =
                vec![(syncable_channel_manager.0, syncable_channel_manager.1 as &mut (dyn chain::Listen))];

                for (block_hash, monitor) in syncable_channel_monitors.into_iter() {
                    chain_listeners.push((
                        block_hash,
                        monitor as &mut (dyn chain::Listen),
                    ));
                }

                let mut cache = UnboundedCache::new();
                
                let chain_tip = Some(
                    init::synchronize_listeners(
                        &mut block_source.deref(),
                        self.config.network,
                        &mut cache,
                        chain_listeners,
                    )
                    .await
                    .unwrap(),
                );

                // TODO: probably want to return the tip, assuming I can get similar object in electrum case
                Ok(())
            }
        }
    }

    pub async fn keep_in_sync(&self, channel_manager: Arc<ChannelManager>, chain_monitor: Arc<ChainMonitor>) -> Result<(), crate::error::Error> {
        match &self.config.backend {
            LightningNodeBackendConfig::Bitcoind(config) => {
                let chain_listener = (chain_monitor, channel_manager);
                self.listener.add_listener(chain_listener);     
                Ok(())
            }
        }
    }

    pub async fn get_best_block(&self) -> Result<BestBlock, crate::error::Error> {
        match &self.config.backend {
            LightningNodeBackendConfig::Bitcoind(config) => {
                let block_source = self.block_source.as_ref().expect("no block source when using bitcoind");
                let blockchain_info = block_source.get_blockchain_info().await.unwrap();
                Ok(BestBlock::new(blockchain_info.latest_blockhash, blockchain_info.latest_height as u32))
            }
        }
    }

    pub async fn sync_confirmables(&self, confirmables: Vec<&dyn Confirm>) -> Result<(), crate::error::Error> {
        Ok(())
    }
}

struct TxFilter {
    watched_transactions: Vec<(Txid, Script)>,
    watched_outputs: Vec<WatchedOutput>,
}

impl TxFilter {
    fn new() -> Self {
        Self {
            watched_transactions: vec![],
            watched_outputs: vec![],
        }
    }

    fn register_tx(&mut self, txid: Txid, script: Script) {
        self.watched_transactions.push((txid, script));
    }

    fn register_output(&mut self, output: WatchedOutput) {
        self.watched_outputs.push(output);
    }
}

impl Default for TxFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl Filter for SenseiChainManager {
    fn register_tx(&self, txid: &Txid, script_pubkey: &Script) {
        let mut filter = self.filter.lock().unwrap();
        filter.register_tx(*txid, script_pubkey.clone());
    }

    fn register_output(&self, output: WatchedOutput) -> Option<TransactionWithPosition> {
        let mut filter = self.filter.lock().unwrap();
        filter.register_output(output);
        // TODO: do we need to check for tx here or wait for next sync?
        None
    }
}