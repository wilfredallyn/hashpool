//! # Share Acceptance Hooks
//!
//! This module provides a callback-based architecture for share events in the mining pool.
//! Instead of tightly coupling share validation logic with specific implementations (like
//! quote dispatch), this module allows registering multiple hooks that are called when
//! shares are accepted.
//!
//! ## Design
//!
//! The trait-based architecture enables:
//! - Testing share validation independently of hooks
//! - Multiple independent handlers for the same event
//! - Easy addition of new functionality (new hooks) without modifying core pool logic
//! - Non-fatal hook failures (hooks can't break share validation)

use thiserror::Error;

/// Error types returned by share acceptance hooks
#[derive(Error, Debug, Clone)]
pub enum HookError {
    /// Hook execution failed with a message
    #[error("Hook execution failed: {0}")]
    ExecutionFailed(String),

    /// Hook is not available or not configured
    #[error("Hook not available")]
    NotAvailable,

    /// Hook encountered a timeout
    #[error("Hook timeout")]
    Timeout,

    /// Custom error with context
    #[error("Hook error: {0}")]
    Custom(String),
}

/// Event triggered when a share is successfully accepted by the pool
#[derive(Debug, Clone)]
pub struct ShareAcceptedEvent {
    /// Unique identifier for this share within the channel
    pub sequence_number: u32,

    /// The channel ID where this share was submitted
    pub channel_id: u32,

    /// The downstream/miner connection ID
    pub downstream_id: u32,

    /// Previous block hash (32 bytes)
    pub prev_hash: Vec<u8>,

    /// Nonce submitted with the share
    pub nonce: u32,

    /// Timestamp when the share was accepted
    pub timestamp: u64,

    /// Whether this share also qualifies as a block
    pub is_block: bool,
}

/// Trait for handling share acceptance events
///
/// Implementations should handle share acceptance events asynchronously
/// and gracefully. Errors returned by hooks do not fail share validation,
/// they are logged and processing continues.
#[async_trait::async_trait]
pub trait ShareAcceptanceHook: Send + Sync {
    /// Called when a share is accepted by the pool
    ///
    /// This hook is called after share validation passes but before
    /// broadcasting to upstream. Hooks are non-fatal - errors here
    /// don't fail the share or stop other hooks from running.
    ///
    /// # Arguments
    /// * `event` - Information about the accepted share
    ///
    /// # Returns
    /// * `Ok(())` - Hook completed successfully
    /// * `Err(HookError)` - Hook encountered an error (non-fatal)
    async fn on_share_accepted(&self, event: ShareAcceptedEvent) -> Result<(), HookError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ====== HookError Tests ======

    #[test]
    fn test_hook_error_execution_failed_display() {
        let err = HookError::ExecutionFailed("test error".to_string());
        assert_eq!(err.to_string(), "Hook execution failed: test error");
    }

    #[test]
    fn test_hook_error_not_available_display() {
        let err = HookError::NotAvailable;
        assert_eq!(err.to_string(), "Hook not available");
    }

    #[test]
    fn test_hook_error_timeout_display() {
        let err = HookError::Timeout;
        assert_eq!(err.to_string(), "Hook timeout");
    }

    #[test]
    fn test_hook_error_custom_display() {
        let err = HookError::Custom("custom error".to_string());
        assert_eq!(err.to_string(), "Hook error: custom error");
    }

    #[test]
    fn test_hook_error_clone() {
        let err1 = HookError::ExecutionFailed("cloneable".to_string());
        let err2 = err1.clone();
        assert_eq!(err1.to_string(), err2.to_string());
    }

    // ====== ShareAcceptedEvent Tests ======

    #[test]
    fn test_share_accepted_event_creation() {
        let event = ShareAcceptedEvent {
            sequence_number: 42,
            channel_id: 1,
            downstream_id: 2,
            prev_hash: vec![0; 32],
            nonce: 12345,
            timestamp: 1000,
            is_block: false,
        };

        assert_eq!(event.sequence_number, 42);
        assert_eq!(event.channel_id, 1);
        assert_eq!(event.downstream_id, 2);
        assert_eq!(event.nonce, 12345);
        assert_eq!(event.prev_hash.len(), 32);
        assert!(!event.is_block);
    }

    #[test]
    fn test_share_accepted_event_is_block() {
        let event = ShareAcceptedEvent {
            sequence_number: 1,
            channel_id: 5,
            downstream_id: 10,
            prev_hash: vec![1; 32],
            nonce: 999,
            timestamp: 2000,
            is_block: true,
        };

        assert!(event.is_block);
        assert_eq!(event.sequence_number, 1);
    }

    #[test]
    fn test_share_accepted_event_clone() {
        let event1 = ShareAcceptedEvent {
            sequence_number: 100,
            channel_id: 5,
            downstream_id: 10,
            prev_hash: vec![2; 32],
            nonce: 5000,
            timestamp: 3000,
            is_block: false,
        };

        let event2 = event1.clone();
        assert_eq!(event1.sequence_number, event2.sequence_number);
        assert_eq!(event1.channel_id, event2.channel_id);
        assert_eq!(event1.prev_hash, event2.prev_hash);
    }

    // ====== Mock Hook Implementation Tests ======

    struct CountingHook {
        call_count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ShareAcceptanceHook for CountingHook {
        async fn on_share_accepted(&self, _event: ShareAcceptedEvent) -> Result<(), HookError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_counting_hook_single_call() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let hook = CountingHook {
            call_count: call_count.clone(),
        };

        let event = ShareAcceptedEvent {
            sequence_number: 1,
            channel_id: 1,
            downstream_id: 1,
            prev_hash: vec![0; 32],
            nonce: 100,
            timestamp: 1000,
            is_block: false,
        };

        hook.on_share_accepted(event).await.unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_counting_hook_multiple_calls() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let hook = Arc::new(CountingHook {
            call_count: call_count.clone(),
        });

        for i in 0..5 {
            let event = ShareAcceptedEvent {
                sequence_number: i,
                channel_id: 1,
                downstream_id: 1,
                prev_hash: vec![0; 32],
                nonce: 100 + i,
                timestamp: 1000 + i as u64,
                is_block: false,
            };

            hook.on_share_accepted(event).await.unwrap();
        }

        assert_eq!(call_count.load(Ordering::SeqCst), 5);
    }

    // ====== Error Handling Tests ======

    struct FailingHook;

    #[async_trait::async_trait]
    impl ShareAcceptanceHook for FailingHook {
        async fn on_share_accepted(&self, _event: ShareAcceptedEvent) -> Result<(), HookError> {
            Err(HookError::ExecutionFailed("intentional failure".to_string()))
        }
    }

    #[tokio::test]
    async fn test_failing_hook_returns_error() {
        let hook = FailingHook;
        let event = ShareAcceptedEvent {
            sequence_number: 1,
            channel_id: 1,
            downstream_id: 1,
            prev_hash: vec![0; 32],
            nonce: 100,
            timestamp: 1000,
            is_block: false,
        };

        let result = hook.on_share_accepted(event).await;
        assert!(result.is_err());

        if let Err(HookError::ExecutionFailed(msg)) = result {
            assert_eq!(msg, "intentional failure");
        } else {
            panic!("Expected ExecutionFailed variant");
        }
    }

    // ====== Multi-Hook Tests ======

    #[tokio::test]
    async fn test_multiple_hooks_independent() {
        let count1 = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::new(AtomicUsize::new(0));

        let hook1 = Arc::new(CountingHook {
            call_count: count1.clone(),
        });
        let hook2 = Arc::new(CountingHook {
            call_count: count2.clone(),
        });

        let event = ShareAcceptedEvent {
            sequence_number: 1,
            channel_id: 1,
            downstream_id: 1,
            prev_hash: vec![0; 32],
            nonce: 100,
            timestamp: 1000,
            is_block: false,
        };

        // Both hooks should be callable independently
        let _: Result<(), HookError> = hook1.on_share_accepted(event.clone()).await;
        let _: Result<(), HookError> = hook2.on_share_accepted(event).await;

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);
    }

    // ====== Event Serialization-like Tests ======

    #[test]
    fn test_share_event_debug_format() {
        let event = ShareAcceptedEvent {
            sequence_number: 42,
            channel_id: 1,
            downstream_id: 2,
            prev_hash: vec![0; 32],
            nonce: 12345,
            timestamp: 1000,
            is_block: false,
        };

        let debug_str = format!("{:?}", event);
        assert!(debug_str.contains("sequence_number"));
        assert!(debug_str.contains("42"));
        assert!(debug_str.contains("channel_id"));
        assert!(debug_str.contains("1"));
    }

    #[test]
    fn test_share_event_various_timestamps() {
        let events = vec![
            ShareAcceptedEvent {
                timestamp: 0,
                sequence_number: 1,
                channel_id: 1,
                downstream_id: 1,
                prev_hash: vec![0; 32],
                nonce: 1,
                is_block: false,
            },
            ShareAcceptedEvent {
                timestamp: u64::MAX,
                sequence_number: 2,
                channel_id: 1,
                downstream_id: 1,
                prev_hash: vec![0; 32],
                nonce: 2,
                is_block: false,
            },
            ShareAcceptedEvent {
                timestamp: 1_000_000_000,
                sequence_number: 3,
                channel_id: 1,
                downstream_id: 1,
                prev_hash: vec![0; 32],
                nonce: 3,
                is_block: false,
            },
        ];

        assert_eq!(events[0].timestamp, 0);
        assert_eq!(events[1].timestamp, u64::MAX);
        assert_eq!(events[2].timestamp, 1_000_000_000);
    }
}
