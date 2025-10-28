//! Mint service integration for the Pool
//!
//! Handles all communication with the Cashu mint service including:
//! - Channel context tracking (channel_id -> miner details)
//! - TCP connection management
//! - Quote request dispatching
//! - Quote response handling

use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::debug;

/// Context information for a mining channel
#[derive(Debug, Clone)]
pub struct ChannelContext {
    pub channel_id: u32,
    pub locking_key_bytes: Option<Vec<u8>>, // Compressed pubkey bytes for quote attribution
    pub downstream_id: u32,
    pub created_at: std::time::Instant,
}

/// Manager for channel context and mint service communication
#[derive(Debug)]
pub struct MintIntegrationManager {
    /// Maps channel_id to channel context
    channel_contexts: Arc<RwLock<HashMap<u32, ChannelContext>>>,
    /// TCP connection address for mint service
    mint_address: String,
}

impl MintIntegrationManager {
    /// Create a new mint integration manager
    pub fn new(mint_address: String) -> Self {
        Self {
            channel_contexts: Arc::new(RwLock::new(HashMap::new())),
            mint_address,
        }
    }

    /// Register a new mining channel
    pub async fn register_channel(
        &self,
        channel_id: u32,
        locking_key_bytes: Option<Vec<u8>>,
        downstream_id: u32,
    ) {
        let has_locking_key = locking_key_bytes.is_some();
        let context = ChannelContext {
            channel_id,
            locking_key_bytes,
            downstream_id,
            created_at: std::time::Instant::now(),
        };

        self.channel_contexts
            .write()
            .await
            .insert(channel_id, context.clone());
        debug!(
            "Registered channel context: channel_id={}, downstream_id={}, has_locking_key={}, mint_address={}",
            channel_id, downstream_id, has_locking_key, self.mint_address
        );
    }

    /// Unregister a mining channel
    pub async fn unregister_channel(&self, channel_id: u32) {
        self.channel_contexts.write().await.remove(&channel_id);
        debug!("Unregistered channel context: channel_id={}", channel_id);
    }

    /// Get channel context
    pub async fn get_channel_context(&self, channel_id: u32) -> Option<ChannelContext> {
        self.channel_contexts.read().await.get(&channel_id).cloned()
    }

    /// Get mint address
    pub fn mint_address(&self) -> &str {
        &self.mint_address
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_context_registration() {
        let manager = MintIntegrationManager::new("127.0.0.1:34260".to_string());

        manager.register_channel(1, Some(vec![0x02; 33]), 100).await;

        let context = manager.get_channel_context(1).await;
        assert!(context.is_some());
        assert_eq!(context.unwrap().channel_id, 1);
    }

    #[tokio::test]
    async fn test_channel_context_unregistration() {
        let manager = MintIntegrationManager::new("127.0.0.1:34260".to_string());

        manager.register_channel(1, Some(vec![0x02; 33]), 100).await;
        manager.unregister_channel(1).await;

        let context = manager.get_channel_context(1).await;
        assert!(context.is_none());
    }
}
