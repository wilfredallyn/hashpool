//! Time-series hashrate metrics collection for hashpool.
//!
//! This crate provides shared types and storage abstractions for collecting
//! and querying hashrate data from translator and pool services.

pub mod metrics;
pub mod storage;
pub mod types;
pub mod windowing;

pub use metrics::derive_hashrate;
pub use storage::StatsStorage;
pub use types::{DownstreamSnapshot, ServiceSnapshot, ServiceType};
pub use windowing::{WindowedMetricsCollector, unix_timestamp};

#[cfg(test)]
mod tests {
    use crate::metrics::derive_hashrate;

    #[test]
    fn test_hashrate_calculation() {
        // Test basic hashrate calculation: 1000 difficulty / 10 seconds = 100 h/s
        let hashrate = derive_hashrate(1000.0, 10);
        assert_eq!(hashrate, 100.0);

        // Test zero difficulty
        let hashrate = derive_hashrate(0.0, 10);
        assert_eq!(hashrate, 0.0);

        // Test large difficulty (typical mining)
        let hashrate = derive_hashrate(1_000_000_000_000.0, 10);
        assert_eq!(hashrate, 100_000_000_000.0); // 100 GH/s
    }

}
