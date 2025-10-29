//! # Quote Dispatch Hook Implementation
//!
//! Bridges the gap between share acceptance events and quote dispatch.
//! This hook implementation allows the pool to dispatch quotes for accepted shares
//! without coupling share validation logic to quote dispatch code.
//!
//! The quote protocol logic (locking key parsing, header hash validation) is
//! delegated to the ehash protocol crate.

use ehash::{parse_locking_key, validate_header_hash};
use quote_dispatcher::QuoteDispatcher;
use share_hooks::{HookError, ShareAcceptanceHook, ShareAcceptedEvent};
use std::sync::Arc;
use tracing::debug;

use super::mint_integration::MintIntegrationManager;

/// Implements quote dispatch as a share acceptance hook
pub struct QuoteDispatchHook {
    dispatcher: Arc<QuoteDispatcher>,
    mint_manager: Arc<MintIntegrationManager>,
}

impl QuoteDispatchHook {
    /// Creates a new QuoteDispatchHook
    pub fn new(dispatcher: Arc<QuoteDispatcher>, mint_manager: Arc<MintIntegrationManager>) -> Self {
        Self {
            dispatcher,
            mint_manager,
        }
    }
}

#[async_trait::async_trait]
impl ShareAcceptanceHook for QuoteDispatchHook {
    async fn on_share_accepted(&self, event: ShareAcceptedEvent) -> Result<(), HookError> {
        // Validate header hash using ehash protocol validation
        let header_hash = validate_header_hash(&event.prev_hash).map_err(|e| {
            HookError::ExecutionFailed(format!("Header hash validation failed: {}", e))
        })?;

        // Get locking key from mint manager
        let locking_key_bytes = self.mint_manager
            .get_channel_context(event.channel_id)
            .await
            .and_then(|ctx| ctx.locking_key_bytes.clone());

        // Validate that we have a locking key
        let bytes = match locking_key_bytes {
            Some(b) => b,
            None => {
                debug!(
                    "Skipping quote for channel {}: missing locking key",
                    event.channel_id
                );
                return Ok(());
            }
        };

        // Parse locking key using ehash protocol parsing
        let pubkey = parse_locking_key(&bytes, event.channel_id).map_err(|e| {
            HookError::ExecutionFailed(format!("Locking key validation failed: {}", e))
        })?;

        // Submit quote to dispatcher
        self.dispatcher
            .submit_quote(&header_hash, pubkey.into_static(), event.channel_id, event.sequence_number)
            .map_err(|e| {
                HookError::ExecutionFailed(format!("Quote dispatch failed: {}", e))
            })?;

        debug!(
            "Successfully dispatched quote for share: channel={}, seq={}",
            event.channel_id, event.sequence_number
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_dispatch_hook_creation() {
        // This test just verifies the hook can be instantiated
        // Real testing would require mocking the dispatcher and mint_manager
        let _hook_trait: Arc<dyn ShareAcceptanceHook> = Arc::new(
            QuoteDispatchHook {
                dispatcher: Arc::new(QuoteDispatcher::new(
                    mint_pool_messaging::MintPoolMessageHub::new(
                        mint_pool_messaging::MessagingConfig::default(),
                    ),
                    None,
                    32,
                )),
                mint_manager: Arc::new(MintIntegrationManager::new("127.0.0.1:34260".to_string())),
            }
        );
    }
}
