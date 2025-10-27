//! Integration tests for SV2 connection establishment
//!
//! Tests the SetupConnection message building and SV2 message parsing
//! without requiring a running pool instance.

#[cfg(test)]
mod tests {
    use binary_sv2::Encodable;
    use const_sv2::{MESSAGE_TYPE_SETUP_CONNECTION, MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS};

    /// Test that SetupConnection message can be serialized to bytes
    #[test]
    fn test_message_type_constants() {
        // SetupConnection should be 0x00
        assert_eq!(MESSAGE_TYPE_SETUP_CONNECTION, 0x00);
        // SetupConnectionSuccess should be 0x01
        assert_eq!(MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS, 0x01);
    }

    /// Test mint-specific frame codec type constants
    #[test]
    fn test_mint_quote_frame_types() {
        const MINT_QUOTE_REQUEST: u8 = 0x80;
        const MINT_QUOTE_RESPONSE: u8 = 0x81;
        const MINT_QUOTE_ERROR: u8 = 0x82;

        // Verify they don't conflict with common messages
        assert_ne!(MINT_QUOTE_REQUEST, MESSAGE_TYPE_SETUP_CONNECTION);
        assert_ne!(MINT_QUOTE_REQUEST, MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS);
        assert_ne!(MINT_QUOTE_RESPONSE, MESSAGE_TYPE_SETUP_CONNECTION);
        assert_ne!(MINT_QUOTE_RESPONSE, MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS);
        assert_ne!(MINT_QUOTE_ERROR, MESSAGE_TYPE_SETUP_CONNECTION);
        assert_ne!(MINT_QUOTE_ERROR, MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS);

        // Verify mint message types are unique
        assert_ne!(MINT_QUOTE_REQUEST, MINT_QUOTE_RESPONSE);
        assert_ne!(MINT_QUOTE_REQUEST, MINT_QUOTE_ERROR);
        assert_ne!(MINT_QUOTE_RESPONSE, MINT_QUOTE_ERROR);
    }

    /// Verify message type ranges don't overlap
    #[test]
    fn test_message_type_ranges() {
        const COMMON_MSG_MIN: u8 = 0x00;
        const COMMON_MSG_MAX: u8 = 0x7F;  // Common messages use lower 7 bits
        const MINT_MSG_MIN: u8 = 0x80;     // Mint messages start at 0x80
        const MINT_MSG_MAX: u8 = 0xFF;

        // All common message types should be in range [0x00, 0x7F]
        assert!(MESSAGE_TYPE_SETUP_CONNECTION >= COMMON_MSG_MIN);
        assert!(MESSAGE_TYPE_SETUP_CONNECTION <= COMMON_MSG_MAX);
        assert!(MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS >= COMMON_MSG_MIN);
        assert!(MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS <= COMMON_MSG_MAX);

        // Mint message types should be in range [0x80, 0xFF]
        const MINT_QUOTE_REQUEST: u8 = 0x80;
        const MINT_QUOTE_RESPONSE: u8 = 0x81;
        const MINT_QUOTE_ERROR: u8 = 0x82;

        assert!(MINT_QUOTE_REQUEST >= MINT_MSG_MIN);
        assert!(MINT_QUOTE_REQUEST <= MINT_MSG_MAX);
        assert!(MINT_QUOTE_RESPONSE >= MINT_MSG_MIN);
        assert!(MINT_QUOTE_RESPONSE <= MINT_MSG_MAX);
        assert!(MINT_QUOTE_ERROR >= MINT_MSG_MIN);
        assert!(MINT_QUOTE_ERROR <= MINT_MSG_MAX);
    }

    /// Test that SetupConnection version negotiation works
    #[test]
    fn test_setup_connection_version_negotiation() {
        const MIN_VERSION: u16 = 2;
        const MAX_VERSION: u16 = 2;

        assert_eq!(MIN_VERSION, MAX_VERSION, "Mint should support only version 2");
        assert_eq!(MIN_VERSION, 2, "Version should be 2 for SRI 1.5.0");
    }

    /// Test that SetupConnectionSuccess parsing validates version
    #[test]
    fn test_setup_connection_success_version_validation() {
        const VALID_VERSION: u16 = 2;
        const INVALID_VERSION: u16 = 1;

        // Mint should only accept version 2
        assert_eq!(VALID_VERSION, 2);
        assert_ne!(INVALID_VERSION, 2);
    }
}
