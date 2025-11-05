//! Snapshot types for time-series metrics collection.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// The type of service sending metrics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceType {
    Translator,
    Pool,
}

/// Snapshot of a single downstream (miner or translator connection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownstreamSnapshot {
    /// Unique identifier for this downstream
    pub downstream_id: u32,

    /// Human-readable name (e.g., "miner_0" or "translator_1")
    pub name: String,

    /// Network address (IP:port)
    pub address: String,

    /// Lifetime total shares accepted for this downstream
    pub shares_lifetime: u64,

    /// Shares accepted in the current measurement window
    pub shares_in_window: u64,

    /// Sum of difficulties for shares in current window
    pub sum_difficulty_in_window: f64,

    /// Size of the measurement window in seconds
    pub window_seconds: u64,

    /// Unix timestamp when this snapshot was captured
    pub timestamp: u64,
}

/// Complete snapshot of a service's metrics state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSnapshot {
    /// The service type (Translator or Pool)
    pub service_type: ServiceType,

    /// Snapshots for all connected downstreams
    pub downstreams: Vec<DownstreamSnapshot>,

    /// Unix timestamp when this snapshot was captured
    pub timestamp: u64,
}

/// A single point in a hashrate time-series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashratePoint {
    /// Unix timestamp
    pub timestamp: u64,

    /// Hashrate in hashes per second
    pub hashrate_hs: f64,
}

/// Get current Unix timestamp in seconds.
pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = DownstreamSnapshot {
            downstream_id: 1,
            name: "miner_1".to_string(),
            address: "192.168.1.100:4444".to_string(),
            shares_lifetime: 100,
            shares_in_window: 5,
            sum_difficulty_in_window: 100.5,
            window_seconds: 60,
            timestamp: unix_timestamp(),
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let deserialized: DownstreamSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.downstream_id, 1);
        assert_eq!(deserialized.shares_lifetime, 100);
    }
}
