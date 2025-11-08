//! Time-series hashrate metrics collection for hashpool.
//!
//! This crate provides shared types and storage abstractions for collecting
//! and querying hashrate data from translator and pool services.

pub mod bucketing;
pub mod metrics;
pub mod storage;
pub mod types;
pub mod windowing;

pub use bucketing::calculate_bucket_size;
pub use metrics::derive_hashrate;
pub use storage::StatsStorage;
pub use types::{DownstreamSnapshot, ServiceSnapshot, ServiceType};
pub use windowing::{WindowedMetricsCollector, unix_timestamp};

#[cfg(test)]
mod tests {
    use crate::metrics::derive_hashrate;

    #[test]
    fn test_hashrate_calculation() {
        // Test basic hashrate calculation: (1000 * 2^32) / 10 seconds
        let hashrate = derive_hashrate(1000.0, 10);
        assert_eq!(hashrate, 429_496_729_600.0);

        // Test zero difficulty
        let hashrate = derive_hashrate(0.0, 10);
        assert_eq!(hashrate, 0.0);

        // Test large difficulty (typical mining)
        // (1_000_000_000_000 * 2^32) / 10
        let hashrate = derive_hashrate(1_000_000_000_000.0, 10);
        let expected = (1_000_000_000_000.0 * 4_294_967_296.0) / 10.0;
        assert_eq!(hashrate, expected);
    }

}
