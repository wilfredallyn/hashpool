use alloc::{fmt, vec::Vec};
use binary_sv2::{binary_codec_sv2, Deserialize, Serialize, Str0255};

/// Notification sent to downstream when a quote becomes payable
/// Extension message (0xC0) for the Mining protocol
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MintQuoteNotification<'decoder> {
    pub quote_id: Str0255<'decoder>,
    pub amount: u64,
}

impl fmt::Display for MintQuoteNotification<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MintQuoteNotification(quote_id: {}, amount: {})",
            self.quote_id.as_utf8_or_hex(),
            self.amount
        )
    }
}

/// Failure notification if quote cannot be processed
/// Extension message (0xC1) for the Mining protocol
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MintQuoteFailure<'decoder> {
    pub quote_id: Str0255<'decoder>,
    pub error_code: u32,
    pub error_message: Str0255<'decoder>,
}

impl fmt::Display for MintQuoteFailure<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MintQuoteFailure(quote_id: {}, error_code: {}, error_message: {})",
            self.quote_id.as_utf8_or_hex(),
            self.error_code,
            self.error_message.as_utf8_or_hex()
        )
    }
}
