//! Periodic quote poller for tracking paid quotes from mint service
//!
//! Polls the mint's HTTP API every 5 seconds to check for newly paid quotes,
//! then sends MintQuoteNotification extension messages to translators.
//!
//! Phase 3 Implementation:
//! - Polls mint HTTP endpoint every 5s
//! - Tracks pending quotes with timeouts
//! - Sends MintQuoteNotification to downstream translators
//! - Correlates quotes to channels for proper message routing

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};
use stratum_common::roles_logic_sv2::{
    codec_sv2::binary_sv2::Str0255,
    mining_sv2::MintQuoteNotification,
    parsers_sv2::Mining,
    handlers::mining::SendTo,
};
use reqwest;
use super::Downstream;

/// Quote metadata for tracking pending quotes
#[derive(Debug, Clone)]
pub struct PendingQuote {
    /// Channel ID that submitted the share
    pub channel_id: u32,
    /// When the quote was created
    pub created_at: Instant,
    /// Amount of the quote (in satoshis or HASH)
    pub amount: u64,
}

/// Quote poller that tracks pending quotes and polls for paid status
pub struct QuotePoller {
    /// Pending quotes: quote_id → (channel_id, amount, timestamp)
    pending_quotes: Arc<tokio::sync::RwLock<HashMap<String, PendingQuote>>>,
    /// Mint HTTP endpoint
    mint_http_endpoint: String,
    /// Quote timeout (5 minutes default)
    quote_timeout: Duration,
}

impl QuotePoller {
    /// Create a new quote poller
    pub fn new(mint_http_endpoint: String) -> Self {
        Self {
            pending_quotes: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            mint_http_endpoint,
            quote_timeout: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Register a new pending quote
    pub async fn register_quote(
        &self,
        quote_id: String,
        channel_id: u32,
        amount: u64,
    ) {
        let pending = PendingQuote {
            channel_id,
            created_at: Instant::now(),
            amount,
        };

        self.pending_quotes.write().await.insert(quote_id.clone(), pending);
        debug!(
            "Registered pending quote: quote_id={}, channel_id={}, amount={}",
            quote_id, channel_id, amount
        );
    }

    /// Get channel_id for a quote (for routing responses)
    pub async fn get_quote_channel(&self, quote_id: &str) -> Option<u32> {
        self.pending_quotes
            .read()
            .await
            .get(quote_id)
            .map(|q| q.channel_id)
    }

    /// Remove a quote (after processing)
    pub async fn remove_quote(&self, quote_id: &str) {
        self.pending_quotes.write().await.remove(quote_id);
        debug!("Removed quote from tracking: quote_id={}", quote_id);
    }

    /// Clean up expired quotes
    pub async fn cleanup_expired_quotes(&self) {
        let now = Instant::now();
        let mut pending = self.pending_quotes.write().await;

        let expired: Vec<String> = pending
            .iter()
            .filter(|(_, q)| now.duration_since(q.created_at) > self.quote_timeout)
            .map(|(id, _)| id.clone())
            .collect();

        for quote_id in expired {
            warn!("Quote expired (timeout after 5min): {}", quote_id);
            pending.remove(&quote_id);
        }
    }

    /// Get all pending quotes (for monitoring/debugging)
    pub async fn get_pending_quotes(&self) -> Vec<(String, u32, u64)> {
        self.pending_quotes
            .read()
            .await
            .iter()
            .map(|(id, q)| (id.clone(), q.channel_id, q.amount))
            .collect()
    }

    /// Start the polling loop
    ///
    /// Phase 3: Polls mint HTTP API and sends MintQuoteNotification extension messages
    pub async fn start(
        &self,
        pool: Arc<stratum_common::roles_logic_sv2::utils::Mutex<crate::mining_pool::Pool>>,
    ) {
        info!("🚀 Quote poller started");
        info!("📍 Mint HTTP endpoint: {}", self.mint_http_endpoint);
        info!("⏱️  Polling interval: 5 seconds");

        let client = reqwest::Client::new();
        let mut ticker = interval(Duration::from_secs(5));
        let mut poll_count = 0;

        loop {
            ticker.tick().await;

            // Clean up expired quotes every 10 polls
            poll_count += 1;
            if poll_count % 10 == 0 {
                self.cleanup_expired_quotes().await;
            }

            // Log current pending quotes count
            let pending_count = self.pending_quotes.read().await.len();
            if pending_count > 0 {
                debug!("Quote poller: {} pending quotes", pending_count);
            }

            // Phase 3: Poll mint HTTP API for PAID quotes
            let mint_endpoint = format!("{}/quotes?status=paid", self.mint_http_endpoint);
            match client.get(&mint_endpoint).send().await {
                Ok(response) => match response.json::<Vec<PaidQuote>>().await {
                    Ok(paid_quotes) => {
                        for quote in paid_quotes {
                            debug!("Found PAID quote: quote_id={}, amount={}", quote.id, quote.amount);

                            // Look up channel_id from pending_quotes
                            if let Some(channel_id) = self.get_quote_channel(&quote.id).await {
                                // Send MintQuoteNotification extension message to translator
                                match self
                                    .send_notification_to_translator(
                                        pool.clone(),
                                        channel_id,
                                        &quote.id,
                                        quote.amount,
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        info!(
                                            "✅ Sent MintQuoteNotification for quote {} to channel {}",
                                            quote.id, channel_id
                                        );
                                        self.remove_quote(&quote.id).await;
                                    }
                                    Err(e) => {
                                        error!("Failed to send notification for quote {}: {}", quote.id, e);
                                    }
                                }
                            } else {
                                warn!("Quote {} not in pending list, skipping", quote.id);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse mint response: {}", e);
                    }
                },
                Err(e) => {
                    error!("Failed to poll mint endpoint: {}", e);
                }
            }

            debug!("Quote poller tick #{}", poll_count);
        }
    }

    /// Send MintQuoteNotification extension message to translator
    async fn send_notification_to_translator(
        &self,
        pool: Arc<stratum_common::roles_logic_sv2::utils::Mutex<crate::mining_pool::Pool>>,
        channel_id: u32,
        quote_id: &str,
        amount: u64,
    ) -> Result<(), String> {
        // Create MintQuoteNotification extension message
        let notification = MintQuoteNotification {
            quote_id: Str0255::try_from(quote_id.to_string())
                .map_err(|e| format!("Failed to create Str0255 from quote_id: {:?}", e))?,
            amount: amount.into(),
        };

        let mining_message = Mining::MintQuoteNotification(notification);

        // Get pool and send via existing infrastructure
        let downstream = pool
            .safe_lock(|p| p.downstreams.get(&channel_id).cloned())
            .map_err(|_| "Failed to lock pool")?
            .ok_or("Downstream not found")?;

        // Send via existing mining protocol connection
        Downstream::match_send_to(downstream, Ok(SendTo::Respond(mining_message)))
            .await
            .map_err(|e| format!("Failed to send: {:?}", e))
    }
}

/// Represents a paid quote from the mint HTTP API
#[derive(Debug, serde::Deserialize)]
struct PaidQuote {
    id: String,
    amount: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quote_registration() {
        let poller = QuotePoller::new("http://localhost:34261".to_string());
        poller
            .register_quote("quote1".to_string(), 42, 1000)
            .await;

        let channel_id = poller.get_quote_channel("quote1").await;
        assert_eq!(channel_id, Some(42));
    }

    #[tokio::test]
    async fn test_quote_removal() {
        let poller = QuotePoller::new("http://localhost:34261".to_string());
        poller
            .register_quote("quote1".to_string(), 42, 1000)
            .await;

        poller.remove_quote("quote1").await;

        let channel_id = poller.get_quote_channel("quote1").await;
        assert_eq!(channel_id, None);
    }
}
