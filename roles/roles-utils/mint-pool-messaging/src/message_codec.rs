//! Message codec re-exports and compatibility layer.
//!
//! This module re-exports message type definitions from the ehash protocol crate
//! and provides backward compatibility helpers.

// Re-export message types from ehash protocol crate
pub use ehash::{MessageType, MintQuoteMessage};

/// Simple message codec for mint-quote messages
/// Note: Full SV2 framing will be added in later phases
pub struct MessageCodec;

impl MessageCodec {
    /// Get the message type for a request
    pub fn get_request_type() -> MessageType {
        MessageType::request()
    }

    /// Get the message type for a response
    pub fn get_response_type() -> MessageType {
        MessageType::response()
    }

    /// Get the message type for an error
    pub fn get_error_type() -> MessageType {
        MessageType::error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // Message Type Tests
    // ============================================================================

    #[test]
    fn test_message_types_are_distinct() {
        let request = MessageCodec::get_request_type();
        let response = MessageCodec::get_response_type();
        let error = MessageCodec::get_error_type();

        // Message types should be different from each other
        assert_ne!(request, response);
        assert_ne!(response, error);
        assert_ne!(request, error);
    }
}
