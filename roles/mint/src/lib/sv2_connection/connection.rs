use std::sync::Arc;
use cdk::mint::Mint;
use shared_config::Sv2MessagingConfig;
use tokio::net::TcpStream;
use network_helpers_sv2::noise_connection::Connection;
use codec_sv2::HandshakeRole;
use roles_logic_sv2::common_messages_sv2::{SetupConnection, Protocol};
use tracing::info;
use anyhow::Result;

use super::message_handler::handle_sv2_connection;

/// Connect to pool via SV2 with Noise encryption
pub async fn connect_to_pool_sv2(
    mint: Arc<Mint>,
    sv2_config: Sv2MessagingConfig,
) {
    info!("Connecting to pool SV2 endpoint: {}", sv2_config.mint_listen_address);

    loop {
        match TcpStream::connect(&sv2_config.mint_listen_address).await {
            Ok(stream) => {
                info!("✅ TCP connection established");

                match establish_sv2_connection(stream).await {
                    Ok((receiver, sender)) => {
                        if let Err(e) = handle_sv2_connection(mint.clone(), receiver, sender).await {
                            tracing::error!("SV2 connection error: {}", e);
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to establish SV2 connection: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            },
            Err(e) => {
                tracing::warn!(
                    "Failed to connect to pool at {}: {}",
                    sv2_config.mint_listen_address, e
                );
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

/// Establish SV2 connection: Noise handshake + SetupConnection negotiation
async fn establish_sv2_connection(
    stream: TcpStream,
) -> Result<(
    async_channel::Receiver<codec_sv2::StandardEitherFrame<roles_logic_sv2::parsers_sv2::PoolMessages<'static>>>,
    async_channel::Sender<codec_sv2::StandardEitherFrame<roles_logic_sv2::parsers_sv2::PoolMessages<'static>>>,
)> {
    // Create Noise connection (mint is initiator)
    let (receiver, sender) = Connection::new(
        stream,
        HandshakeRole::Initiator,
    )
    .await?;
    info!("🔐 Noise handshake completed");

    // Send SetupConnection message
    let setup_connection = SetupConnection {
        protocol: Protocol::MiningProtocol,
        min_version: 2,
        max_version: 2,
        flags: 0,  // Mint is stateless quote service
        connection_flags: 0,
    };

    let setup_frame: codec_sv2::StandardSv2Frame<roles_logic_sv2::parsers_sv2::PoolMessages> =
        roles_logic_sv2::parsers_sv2::PoolMessages::Common(
            roles_logic_sv2::common_messages_sv2::CommonMessages::SetupConnection(setup_connection),
        )
        .try_into()?;

    sender.send(setup_frame.into()).await?;
    info!("Sent SetupConnection message");

    // Receive SetupConnectionSuccess or Error
    let response_frame = receiver.recv().await?;

    match response_frame {
        codec_sv2::StandardEitherFrame::Sv2(frame) => {
            let msg_type = frame
                .get_header()
                .ok_or_else(|| anyhow::anyhow!("No frame header"))?
                .msg_type();

            match msg_type {
                0x00 => {
                    info!("✅ Pool accepted SetupConnection");
                },
                0x01 => {
                    return Err(anyhow::anyhow!("Pool rejected SetupConnection"));
                },
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unexpected response type: 0x{:02x}",
                        msg_type
                    ));
                }
            }
        },
        _ => {
            return Err(anyhow::anyhow!("Expected SV2 frame"));
        }
    }

    info!("✅ Mint SV2 connection fully established");
    Ok((receiver, sender))
}