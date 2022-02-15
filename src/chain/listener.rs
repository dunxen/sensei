use std::{collections::HashMap, sync::{Mutex, Arc}};

use crate::node::{ChainMonitor, ChannelManager};
use bitcoin::{Block, BlockHeader};
use lightning::chain::Listen;

pub struct SenseiChainListener {
    listeners: Mutex<HashMap<String, (Arc<ChainMonitor>, Arc<ChannelManager>)>>
}

impl SenseiChainListener {
    pub fn new() -> Self {
        Self {
            listeners: Mutex::new(HashMap::new())
        }
    }

    fn get_key(&self, listener: &(Arc<ChainMonitor>, Arc<ChannelManager>)) -> String {
        listener.1.get_our_node_id().to_string()
    }

    pub fn add_listener(&self, listener: (Arc<ChainMonitor>, Arc<ChannelManager>)) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.insert(self.get_key(&listener), listener);
    }

    pub fn remove_listener(&self, listener: (Arc<ChainMonitor>, Arc<ChannelManager>)) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.remove(&self.get_key(&listener));
    }
}

impl Listen for SenseiChainListener {
    fn block_connected(&self, block: &Block, height: u32) {
        let listeners = self.listeners.lock().unwrap();
        for (chain_monitor, channel_manager) in listeners.values() {
            chain_monitor.block_connected(block, height);
            channel_manager.block_connected(block, height);
        }
    }

    fn block_disconnected(&self, header: &BlockHeader, height: u32) {
        let listeners = self.listeners.lock().unwrap();
        for (chain_monitor, channel_manager) in listeners.values() {
            chain_monitor.block_disconnected(header, height);
            channel_manager.block_disconnected(header, height);
        }
    }
}

