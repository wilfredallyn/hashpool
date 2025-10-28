//! Setup connection message building for mint-pool SV2 communication
//!
//! Handles construction and serialization of SetupConnection messages
//! required during the SV2 handshake process

use anyhow::Result;
use binary_sv2::Str0255;
use common_messages_sv2::{Protocol, SetupConnection};
use tracing::info;

/// Build a SetupConnection message for the mint role
///
/// This constructs the message that the mint sends to the pool after
/// completing the Noise handshake, establishing the connection protocol version,
/// flags, and device information.
///
/// # Arguments
/// * `endpoint_host` - Hostname or IP address of the mint endpoint
/// * `endpoint_port` - Port number of the mint endpoint
/// * `device_id` - Optional device identifier (empty string if not used)
///
/// # Returns
/// A SetupConnection message configured for mining protocol
pub fn build_mint_setup_connection<'a>(
    endpoint_host: &'a str,
    endpoint_port: u16,
    device_id: &'a str,
) -> Result<SetupConnection<'a>> {
    // Convert strings to Str0255 format required by SRI 1.5.0
    // Using try_from instead of from_string (from_string doesn't exist in SRI 1.5.0)
    let endpoint_host = Str0255::try_from(endpoint_host.to_string())
        .map_err(|e| anyhow::anyhow!("Invalid endpoint_host: {:?}", e))?;

    let vendor = Str0255::try_from("HashPool Mint".to_string())
        .map_err(|e| anyhow::anyhow!("Invalid vendor: {:?}", e))?;

    let hardware_version = Str0255::try_from("v1.0".to_string())
        .map_err(|e| anyhow::anyhow!("Invalid hardware_version: {:?}", e))?;

    let firmware = Str0255::try_from("mint-sv2-phase-3".to_string())
        .map_err(|e| anyhow::anyhow!("Invalid firmware: {:?}", e))?;

    let device_id = Str0255::try_from(device_id.to_string())
        .map_err(|e| anyhow::anyhow!("Invalid device_id: {:?}", e))?;

    // Capture host bytes before moving endpoint_host
    let host_bytes = std::str::from_utf8(endpoint_host.inner_as_ref())
        .unwrap_or("<invalid_utf8>")
        .to_string();

    // Create SetupConnection with all required fields
    // Note: Using mining protocol as the mint acts as a downstream to the pool
    let setup_connection = SetupConnection {
        protocol: Protocol::MiningProtocol,
        min_version: 2,
        max_version: 2,
        flags: 0, // No optional features enabled for now
        endpoint_host,
        endpoint_port,
        vendor,
        hardware_version,
        firmware,
        device_id,
    };

    info!(
        "✅ Built SetupConnection: protocol={:?}, host={}, port={}",
        setup_connection.protocol, host_bytes, endpoint_port
    );

    Ok(setup_connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_setup_connection() {
        let result = build_mint_setup_connection("127.0.0.1", 8080, "mint-01");
        assert!(result.is_ok());

        let sc = result.unwrap();
        assert_eq!(sc.min_version, 2);
        assert_eq!(sc.max_version, 2);
        assert_eq!(sc.endpoint_port, 8080);
        assert_eq!(sc.flags, 0);
    }

    #[test]
    fn test_build_setup_connection_empty_device_id() {
        let result = build_mint_setup_connection("localhost", 9000, "");
        assert!(result.is_ok());

        let sc = result.unwrap();
        assert_eq!(sc.endpoint_port, 9000);
    }

    #[test]
    fn test_setup_connection_protocol_is_mining() {
        let result = build_mint_setup_connection("192.168.1.100", 8000, "device-1");
        assert!(result.is_ok());

        let sc = result.unwrap();
        assert_eq!(sc.protocol, Protocol::MiningProtocol);
    }
}
