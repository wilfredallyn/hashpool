//! # Stratum V2 Mint Quote Protocol Messages
//!
//! SV2 message types for communication between mining pools and mint services.

pub use binary_sv2::binary_codec_sv2::{self, Decodable as Deserialize, Encodable as Serialize, *};
pub use derive_codec_sv2::{Decodable as Deserialize, Encodable as Serialize};

use core::convert::TryInto;

/// Type alias for a 33-byte compressed public key (binary data)
pub type CompressedPubKey<'a> = B032<'a>;

mod mint_quote_request;
mod mint_quote_response; 
mod mint_quote_error;

pub use mint_quote_request::MintQuoteRequest;
pub use mint_quote_response::MintQuoteResponse;
pub use mint_quote_error::MintQuoteError;
