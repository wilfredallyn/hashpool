use anyhow::Result;
use cdk::mint::Mint;
use codec_sv2::StandardEitherFrame;
use const_sv2::MESSAGE_TYPE_MINT_QUOTE_REQUEST;
use mint_pool_messaging::{mint_quote_response_from_cdk, parse_mint_quote_request};
use mint_quote_sv2::MintQuoteResponse;
use std::sync::Arc;
use tracing::info;

use super::frame_codec;
use codec_sv2::{StandardSv2Frame};

/// Type alias for frames used in mint/pool communication
type MintFrame = StandardEitherFrame<Vec<u8>>;

/// Process mint quote messages
pub async fn process_mint_quote_message(
    mint: Arc<Mint>,
    message_type: u8,
    payload: &[u8],
    sender: &async_channel::Sender<MintFrame>,
) -> Result<()> {
    info!("Received mint quote message - processing with mint");

    match message_type {
        MESSAGE_TYPE_MINT_QUOTE_REQUEST => {
            let parsed_request = parse_mint_quote_request(payload)
                .map_err(|e| anyhow::anyhow!("Failed to parse MintQuoteRequest: {e}"))?;
            let share_hash = parsed_request.share_hash;

            let cdk_request = parsed_request
                .to_cdk_request()
                .map_err(|e| anyhow::anyhow!("Failed to convert MintQuoteRequest: {e}"))?;

            match mint.create_mint_mining_share_quote(cdk_request).await {
                Ok(quote_response) => {
                    info!(
                        "Successfully created mint quote: quote_id={} share_hash={} amount={}",
                        quote_response.id, share_hash, parsed_request.request.amount,
                    );

                    let sv2_response = mint_quote_response_from_cdk(share_hash, quote_response)
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to convert mint quote response: {e}")
                        })?;

                    // Send response back to pool
                    send_quote_response_to_pool(sv2_response, sender).await?;

                    Ok(())
                }
                Err(e) => {
                    tracing::error!("Failed to create mint quote: {}", e);

                    // Send error response back to pool
                    send_quote_error_to_pool(e.to_string(), sender).await?;

                    Err(anyhow::anyhow!("Mint quote creation failed: {}", e))
                }
            }
        }
        _ => {
            tracing::warn!(
                "Received unsupported mint quote message type: 0x{:02x}",
                message_type
            );
            Ok(())
        }
    }
}
/// Send MintQuoteResponse back to pool via TCP connection
async fn send_quote_response_to_pool(
    response: MintQuoteResponse<'static>,
    sender: &async_channel::Sender<MintFrame>,
) -> Result<()> {
    let quote_id_str =
        std::str::from_utf8(response.quote_id.inner_as_ref()).unwrap_or("invalid_utf8");

    info!(
        "🚀 Sending quote response: quote_id={}",
        quote_id_str
    );

    // Encode MintQuoteResponse using binary_sv2 codec serialization
    // The response is serialized to bytes using the Encodable trait,
    // with message type byte (0x81) prepended for frame identification
    let frame_bytes = frame_codec::encode_mint_quote_response(&response)?;

    // Create Sv2Frame from encoded bytes and wrap in StandardEitherFrame for transmission
    let sv2_frame = StandardSv2Frame::from_bytes_unchecked(frame_bytes.into());
    let frame = StandardEitherFrame::Sv2(sv2_frame);

    sender.send(frame).await
        .map_err(|e| anyhow::anyhow!("Failed to send quote response frame: {}", e))?;

    info!("✅ Quote response sent (quote_id={})", quote_id_str);
    Ok(())
}

/// Send MintQuoteError back to pool
async fn send_quote_error_to_pool(
    error_message: String,
    sender: &async_channel::Sender<MintFrame>,
) -> Result<()> {
    // Create error code (generic error = 1)
    let error_code = 1u32;

    info!(
        "⚠️  Sending error response: code={}, message={}",
        error_code, error_message
    );

    // Use frame codec to encode error response
    let frame_bytes = frame_codec::encode_mint_quote_error(error_code, &error_message)?;

    // Create Sv2Frame from encoded bytes and wrap in StandardEitherFrame for transmission
    let sv2_frame = StandardSv2Frame::from_bytes_unchecked(frame_bytes.into());
    let frame = StandardEitherFrame::Sv2(sv2_frame);

    sender.send(frame).await
        .map_err(|e| anyhow::anyhow!("Failed to send error frame: {}", e))?;

    info!(
        "✅ Quote error sent (code={}, message={})",
        error_code,
        error_message
    );
    Ok(())
}
