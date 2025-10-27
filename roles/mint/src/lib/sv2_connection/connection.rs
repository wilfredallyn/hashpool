use std::sync::Arc;
use cdk::mint::Mint;
use shared_config::Sv2MessagingConfig;
use tokio::net::TcpStream;
use codec_sv2::{StandardEitherFrame, StandardSv2Frame, Initiator, HandshakeRole};
use network_helpers_sv2::noise_connection::Connection;
use roles_logic_sv2::parsers_sv2::{AnyMessage, CommonMessages};
use const_sv2::MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS;
use tracing::info;
use anyhow::Result;

use super::message_handler::handle_sv2_connection;
use super::state_machine::ConnectionStateMachine;
use super::setup_connection::build_mint_setup_connection;

/// Connect to pool via SV2 with Noise encryption
pub async fn connect_to_pool_sv2(
    mint: Arc<Mint>,
    sv2_config: Sv2MessagingConfig,
) {
    info!("Connecting to pool SV2 endpoint: {}", sv2_config.mint_listen_address);

    loop {
        // Create fresh state machine for each connection attempt
        let mut state_machine = ConnectionStateMachine::new();

        match TcpStream::connect(&sv2_config.mint_listen_address).await {
            Ok(stream) => {
                // Transition to Connecting state after TCP establishment
                if let Err(e) = state_machine.tcp_connected() {
                    tracing::error!("State transition error: {}", e);
                    continue;
                }
                info!("✅ TCP connection established - state: {}", state_machine.current_state());

                match establish_sv2_connection(stream, &mut state_machine).await {
                    Ok((receiver, sender)) => {
                        if let Err(e) = handle_sv2_connection(mint.clone(), receiver, sender).await {
                            tracing::error!("SV2 connection error: {}", e);
                            state_machine.error(format!("Connection error: {}", e));
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to establish SV2 connection: {}", e);
                        state_machine.error(format!("Establishment error: {}", e));
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            },
            Err(e) => {
                tracing::warn!(
                    "Failed to connect to pool at {}: {}",
                    sv2_config.mint_listen_address, e
                );
                state_machine.error(format!("TCP connection failed: {}", e));
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

/// Type alias for frames used in mint/pool communication
type MintFrame = StandardEitherFrame<Vec<u8>>;

/// Establish SV2 connection: Noise handshake + SetupConnection negotiation
/// This implements the full SV2 connection flow following the translator pattern
async fn establish_sv2_connection(
    stream: TcpStream,
    state_machine: &mut ConnectionStateMachine,
) -> Result<(
    async_channel::Receiver<MintFrame>,
    async_channel::Sender<MintFrame>,
)> {
    // Phase 3a: Create Noise protocol initiator for mint role
    // The initiator is created with the pool's public key (placeholder for now)
    let initiator = Initiator::from_raw_k([0u8; 32])
        .map_err(|e| anyhow::anyhow!("Failed to create Initiator: {:?}", e))?;

    // Phase 3b: Perform full Noise handshake using network_helpers Connection
    // This establishes the encrypted channel with the pool
    // Using AnyMessage to support all SV2 message types during handshake
    let (receiver, sender) = Connection::new::<AnyMessage>(stream, HandshakeRole::Initiator(initiator))
        .await
        .map_err(|e| anyhow::anyhow!("Noise handshake failed: {:?}", e))?;

    // Transition state: Noise handshake complete
    state_machine.noise_handshake_complete()
        .map_err(|e| anyhow::anyhow!("Failed to transition to SetupInProgress: {}", e))?;
    info!("🔐 Noise handshake complete - state: {}", state_machine.current_state());

    // Phase 3c: Build and send SetupConnection message
    let setup_connection = build_mint_setup_connection("127.0.0.1", 3333, "mint-01")?;
    info!("📋 Built SetupConnection: version {}-{}",
        setup_connection.min_version,
        setup_connection.max_version
    );

    // Convert SetupConnection to binary frame and send through encrypted channel
    let mut setup_bytes = Vec::new();
    use binary_sv2::Encodable;
    setup_connection.to_bytes(&mut setup_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to encode SetupConnection: {:?}", e))?;

    // Create Sv2Frame from encoded bytes and send through encrypted channel
    let sv2_frame = StandardSv2Frame::from_bytes_unchecked(setup_bytes.into());
    let frame = StandardEitherFrame::Sv2(sv2_frame);

    sender.send(frame)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send SetupConnection: {}", e))?;

    info!("📤 Sent SetupConnection through encrypted channel");

    // Phase 3d: Receive and validate SetupConnection response from pool
    // The pool should respond with SetupConnectionSuccess
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        receiver.recv()
    ).await {
        Ok(Ok(response_frame)) => {
            info!("✅ Received SetupConnection response from pool");

            // Extract and validate the Sv2 frame
            // In Phase 3, we validate that we received a valid Sv2 response frame
            // In Phase 4, we can implement full SetupConnectionSuccess parsing
            match response_frame {
                StandardEitherFrame::Sv2(mut frame) => {
                    // Extract message type and payload
                    let header = frame.get_header()
                        .ok_or_else(|| anyhow::anyhow!("SetupConnectionSuccess response missing header"))?;

                    let message_type = header.msg_type();
                    let payload = frame.payload().to_vec();

                    info!("✅ Pool responded with Sv2 frame (type: 0x{:02x}, {} bytes)",
                        message_type, payload.len());

                    // Phase 4: Full SetupConnectionSuccess parsing
                    // Validate message type is SetupConnectionSuccess (0x1)
                    if message_type != MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS {
                        return Err(anyhow::anyhow!(
                            "Expected SetupConnectionSuccess (0x{:02x}), got 0x{:02x}",
                            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
                            message_type
                        ));
                    }

                    // Parse the message from (message_type, payload) tuple
                    let mut payload_mut = payload.clone();
                    let message: AnyMessage = (message_type, payload_mut.as_mut_slice())
                        .try_into()
                        .map_err(|e| anyhow::anyhow!("Failed to parse SetupConnectionSuccess: {:?}", e))?;

                    // Convert to static lifetime for use in async context
                    let message_static = match message {
                        AnyMessage::Common(m) => match m {
                            CommonMessages::SetupConnectionSuccess(success_msg) => {
                                success_msg.into_static()
                            }
                            _ => {
                                return Err(anyhow::anyhow!(
                                    "Expected SetupConnectionSuccess, got different CommonMessage"
                                ));
                            }
                        }
                        _ => {
                            return Err(anyhow::anyhow!(
                                "Expected CommonMessages variant, got different message type"
                            ));
                        }
                    };

                    // Validate protocol version
                    if message_static.used_version != 2 {
                        return Err(anyhow::anyhow!(
                            "Pool negotiated unsupported version: {} (expected 2)",
                            message_static.used_version
                        ));
                    }

                    info!("🔗 SetupConnectionSuccess validated");
                    info!("   - Negotiated version: {}", message_static.used_version);
                    info!("   - Pool feature flags: 0x{:08x}", message_static.flags);
                }
                StandardEitherFrame::HandShake(_) => {
                    return Err(anyhow::anyhow!(
                        "Unexpected HandShake frame in response, expected Sv2 frame"
                    ));
                }
            }
        }
        Ok(Err(e)) => {
            tracing::warn!("Channel recv error waiting for SetupConnection response: {}", e);
            return Err(anyhow::anyhow!("Channel error receiving response: {}", e));
        }
        Err(_) => {
            return Err(anyhow::anyhow!("Timeout (10s) waiting for SetupConnection response from pool"));
        }
    }

    // Transition state: SetupConnection accepted
    state_machine.setup_connection_accepted()
        .map_err(|e| anyhow::anyhow!("Failed to transition to Ready: {}", e))?;
    info!("📋 SetupConnection negotiated - state: {}", state_machine.current_state());

    // Create in-memory message handling channels
    // These bridge between the Noise-encrypted recv/sender and message processing
    let (msg_sender, msg_receiver) = async_channel::bounded(100);

    info!("✅ Mint SV2 connection ready (state: {})", state_machine.current_state());

    Ok((msg_receiver, msg_sender))
}