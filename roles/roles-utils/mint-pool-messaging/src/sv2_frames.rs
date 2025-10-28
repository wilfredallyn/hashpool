//! Helpers for serializing mint-quote messages into full SV2 frames.
//!
//! These utilities make sure the SV2 header is correctly populated so both
//! pool and mint sides can rely on the standard framing implementation without
//! manually crafting byte arrays.

use binary_sv2::to_bytes;
use const_sv2::{
    CHANNEL_BIT_MINT_QUOTE_ERROR, CHANNEL_BIT_MINT_QUOTE_REQUEST, CHANNEL_BIT_MINT_QUOTE_RESPONSE,
    MESSAGE_TYPE_MINT_QUOTE_ERROR, MESSAGE_TYPE_MINT_QUOTE_REQUEST,
    MESSAGE_TYPE_MINT_QUOTE_RESPONSE, SV2_MINT_QUOTE_PROTOCOL_DISCRIMINANT,
};
use mint_quote_sv2::{MintQuoteError, MintQuoteRequest, MintQuoteResponse};

use crate::{MessagingError, MessagingResult};

/// Build a noise-ready SV2 frame for a mint quote request.
pub fn quote_request_frame_bytes(request: &MintQuoteRequest<'static>) -> MessagingResult<Vec<u8>> {
    let payload = to_bytes(request.clone()).map_err(|e| {
        MessagingError::Encoding(format!("failed to encode MintQuoteRequest: {e:?}"))
    })?;
    build_frame_bytes(
        payload,
        MESSAGE_TYPE_MINT_QUOTE_REQUEST,
        CHANNEL_BIT_MINT_QUOTE_REQUEST,
    )
}

/// Build a noise-ready SV2 frame for a mint quote response.
pub fn quote_response_frame_bytes(
    response: &MintQuoteResponse<'static>,
) -> MessagingResult<Vec<u8>> {
    let payload = to_bytes(response.clone()).map_err(|e| {
        MessagingError::Encoding(format!("failed to encode MintQuoteResponse: {e:?}"))
    })?;
    build_frame_bytes(
        payload,
        MESSAGE_TYPE_MINT_QUOTE_RESPONSE,
        CHANNEL_BIT_MINT_QUOTE_RESPONSE,
    )
}

/// Build a noise-ready SV2 frame for a mint quote error.
pub fn quote_error_frame_bytes(error: &MintQuoteError<'static>) -> MessagingResult<Vec<u8>> {
    let payload = to_bytes(error.clone())
        .map_err(|e| MessagingError::Encoding(format!("failed to encode MintQuoteError: {e:?}")))?;
    build_frame_bytes(
        payload,
        MESSAGE_TYPE_MINT_QUOTE_ERROR,
        CHANNEL_BIT_MINT_QUOTE_ERROR,
    )
}

fn build_frame_bytes(
    payload: Vec<u8>,
    message_type: u8,
    channel_msg: bool,
) -> MessagingResult<Vec<u8>> {
    let mut extension_type = SV2_MINT_QUOTE_PROTOCOL_DISCRIMINANT as u16;
    if channel_msg {
        extension_type |= 0x8000;
    }

    let mut frame = Vec::with_capacity(6 + payload.len());
    frame.extend_from_slice(&extension_type.to_le_bytes());
    frame.push(message_type);
    let len_bytes = (payload.len() as u32).to_le_bytes();
    frame.extend_from_slice(&len_bytes[..3]);
    frame.extend_from_slice(&payload);
    Ok(frame)
}
