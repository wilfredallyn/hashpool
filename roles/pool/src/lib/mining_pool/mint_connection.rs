//! Mint service connection management
//!
//! Handles Noise-encrypted TCP connections with the Cashu mint service,
//! including handshake protocol and message framing.
//!
//! Phase 2 Implementation:
//! - Listens for incoming connections from the mint service
//! - Performs Noise protocol handshake as responder
//! - Sets up encrypted communication channel
//! - Prepares for Phase 3 message exchange

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use stratum_common::{
    network_helpers_sv2::noise_connection::Connection,
    roles_logic_sv2::codec_sv2::{HandshakeRole, Responder},
    roles_logic_sv2::parsers_sv2::AnyMessage,
};
use key_utils::{Secp256k1PublicKey, Secp256k1SecretKey};

/// Message type alias for mint protocol communication
/// Uses AnyMessage to support all SV2 message types during handshake
pub type MintMessage = AnyMessage<'static>;

/// Manages the connection to the mint service
pub struct MintConnection {
    /// Remote address of the mint service
    address: SocketAddr,
    /// Authority secret key for Noise protocol
    authority_secret_key: Secp256k1SecretKey,
    /// Authority public key for Noise protocol
    authority_public_key: Secp256k1PublicKey,
    /// Certificate validity duration
    cert_validity_duration: Duration,
    /// TCP connection receiver
    receiver: Option<Box<dyn std::any::Any + Send + Sync>>,
    /// TCP connection sender
    sender: Option<Box<dyn std::any::Any + Send + Sync>>,
    /// Connection state
    is_connected: Arc<RwLock<bool>>,
}

impl MintConnection {
    /// Create a new mint connection instance with default authentication keys
    pub fn new(address: SocketAddr) -> Self {
        // Use default keys for basic connectivity
        // Phase 3: These should be loaded from pool config
        use secp256k1::rand::thread_rng;

        let secret_key = secp256k1::SecretKey::new(&mut thread_rng());
        let authority_secret_key = Secp256k1SecretKey(secret_key);
        let authority_public_key = Secp256k1PublicKey::from(authority_secret_key);

        Self {
            address,
            authority_secret_key,
            authority_public_key,
            cert_validity_duration: Duration::from_secs(3600), // 1 hour default
            receiver: None,
            sender: None,
            is_connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Create a new mint connection instance with specified authentication keys
    pub fn with_keys(
        address: SocketAddr,
        authority_secret_key: Secp256k1SecretKey,
        authority_public_key: Secp256k1PublicKey,
        cert_validity_duration: Duration,
    ) -> Self {
        Self {
            address,
            authority_secret_key,
            authority_public_key,
            cert_validity_duration,
            receiver: None,
            sender: None,
            is_connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Get mint service address
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }

    /// Establish connection with mint service
    ///
    /// Phase 2: Listens for an incoming connection from the mint
    /// and performs the SV2 Noise handshake as the responder.
    pub async fn establish_connection(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("🔗 Attempting to establish mint connection on {}", self.address);

        // Create TCP listener as the pool (responder)
        let listener = TcpListener::bind(self.address).await?;
        info!("📡 Listening for mint service on {}", self.address);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("✓ Accepted connection from mint service: {}", peer_addr);

                    match self.perform_handshake(stream).await {
                        Ok((receiver, sender)) => {
                            self.receiver = Some(Box::new(receiver));
                            self.sender = Some(Box::new(sender));
                            *self.is_connected.write().await = true;

                            info!("🔐 Noise handshake successful with mint at {}", peer_addr);
                            return Ok(());
                        }
                        Err(e) => {
                            error!("❌ Handshake failed with {}: {}", peer_addr, e);
                            warn!("Waiting for next connection attempt...");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    error!("❌ Error accepting connection: {}", e);
                    return Err(Box::new(e));
                }
            }
        }
    }

    /// Perform SV2 Noise handshake with mint service
    ///
    /// This establishes the Noise-encrypted channel and performs
    /// the SV2 SetupConnection/SetupConnectionSuccess exchange.
    async fn perform_handshake(
        &self,
        stream: TcpStream,
    ) -> Result<(Box<dyn std::any::Any + Send + Sync>, Box<dyn std::any::Any + Send + Sync>), String> {
        debug!("Starting Noise handshake as responder...");

        // Create Noise responder for the handshake using authority keys
        let responder = Responder::from_authority_kp(
            &self.authority_public_key.clone().into_bytes(),
            &self.authority_secret_key.clone().into_bytes(),
            self.cert_validity_duration,
        )
        .map_err(|e| format!("Failed to create responder: {:?}", e))?;

        // Perform Noise connection handshake with mint service
        // This establishes the encrypted channel using Noise protocol
        // Uses AnyMessage to support the SV2 SetupConnection messages
        let (receiver, sender) = Connection::new::<MintMessage>(
            stream,
            HandshakeRole::Responder(responder),
        )
        .await
        .map_err(|e| format!("Connection failed: {:?}", e))?;

        debug!("Noise handshake completed");
        info!("✅ SV2 Noise handshake successful");

        // Phase 3: Receiver and sender will be used for message exchange
        // The mint will send MintQuoteRequest messages via the sender
        // The pool will receive them via the receiver

        Ok((
            Box::new(receiver) as Box<dyn std::any::Any + Send + Sync>,
            Box::new(sender) as Box<dyn std::any::Any + Send + Sync>,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_mint_connection_creation() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 34260);
        let conn = MintConnection::new(addr);

        assert_eq!(conn.address(), addr);
    }

    #[tokio::test]
    async fn test_mint_connection_not_connected_initially() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 34260);
        let conn = MintConnection::new(addr);

        assert!(!conn.is_connected().await);
    }
}
