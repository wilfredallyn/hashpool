//! Mint service integration for the Pool
//!
//! Handles all communication with the Cashu mint service including:
//! - Channel context tracking (channel_id -> miner details)
//! - TCP connection management
//! - Quote request dispatching
//! - Quote response handling

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use async_channel::Receiver;

use super::ShareQuoteRequest;

/// Context information for a mining channel
#[derive(Debug, Clone)]
pub struct ChannelContext {
    pub channel_id: u32,
    pub locking_key: Option<Vec<u8>>, // Compressed pubkey for quote attribution
    pub downstream_id: u32,
    pub created_at: std::time::Instant,
}

/// Manager for channel context and mint service communication
pub struct MintIntegrationManager {
    /// Maps channel_id to channel context
    channel_contexts: Arc<RwLock<HashMap<u32, ChannelContext>>>,
    /// TCP connection address for mint service
    mint_address: String,
    /// Active TCP connection to mint (if established)
    _mint_connection: Option<Arc<RwLock<Box<dyn std::any::Any + Send + Sync>>>>,
}

impl MintIntegrationManager {
    /// Create a new mint integration manager
    pub fn new(mint_address: String) -> Self {
        Self {
            channel_contexts: Arc::new(RwLock::new(HashMap::new())),
            mint_address,
            _mint_connection: None,
        }
    }

    /// Register a new mining channel
    pub async fn register_channel(
        &self,
        channel_id: u32,
        locking_key: Option<Vec<u8>>,
        downstream_id: u32,
    ) {
        let context = ChannelContext {
            channel_id,
            locking_key,
            downstream_id,
            created_at: std::time::Instant::now(),
        };

        self.channel_contexts.write().await.insert(channel_id, context.clone());
        debug!(
            "Registered channel context: channel_id={}, downstream_id={}, mint_address={}",
            channel_id, downstream_id, self.mint_address
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

    /// Process a quote request and send to mint service
    pub async fn process_quote_request(
        &self,
        quote_request: ShareQuoteRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get channel context
        let context = self.get_channel_context(quote_request.channel_id).await;

        if context.is_none() {
            warn!(
                "Quote request for unknown channel: channel_id={}",
                quote_request.channel_id
            );
            return Ok(()); // Skip silently if channel not found
        }

        let context = context.unwrap();

        // Phase 3: Quote request received and logged for processing
        // The quote_poller module handles the full Phase 3 flow:
        // - Periodically polls mint HTTP API for paid quotes
        // - Creates MintQuoteNotification extension messages
        // - Routes notifications to translators via SV2 mining protocol
        info!(
            "Quote request: channel_id={}, seq={}, downstream_id={}, mint_address={}",
            quote_request.channel_id,
            quote_request.sequence_number,
            context.downstream_id,
            self.mint_address
        );

        // Phase 4 (Future): Direct TCP/Noise communication with mint
        // Currently: Pool sends share quotes to mint via HTTP, receives paid quotes via polling
        // Phase 4 would implement:
        // 1. Maintain persistent Noise-encrypted TCP connection to mint
        // 2. Send MintQuoteRequest directly via SV2 MintQuote subprotocol
        // 3. Receive MintQuoteResponse with signatures in real-time
        // This would replace the HTTP polling pattern with direct async messaging

        Ok(())
    }

    /// Start the mint integration service
    ///
    /// Phase 2: This method will be enhanced to establish a Noise-encrypted
    /// TCP connection to the mint service. For now, it logs the startup and
    /// awaits the connection mechanism implementation.
    pub async fn start(
        &self,
        quote_receiver: Receiver<ShareQuoteRequest>,
    ) {
        info!("🚀 Mint integration manager started");
        info!("📍 Configured mint address: {}", self.mint_address);
        info!("⏳ Phase 2: Awaiting Noise-encrypted TCP connection to mint service...");

        // TODO Phase 2: Establish Noise-encrypted connection with:
        // 1. TCP listener on mint_address
        // 2. Noise handshake as responder
        // 3. SV2 SetupConnection exchange
        // 4. Connection lifecycle management

        // Main loop: receive quote requests and process them
        while let Ok(quote_request) = quote_receiver.recv().await {
            if let Err(e) = self.process_quote_request(quote_request).await {
                error!("Failed to process quote request: {}", e);
            }
        }

        info!("🛑 Mint integration manager shutting down");
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
