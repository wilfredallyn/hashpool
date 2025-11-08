//! Adaptive bucketing strategy for time-series data aggregation.
//!
//! This module provides intelligent bucketing calculations to maintain consistent
//! data point density across different time ranges. The goal is to keep approximately
//! 60 data points per graph regardless of the time window, balancing detail visibility
//! with rendering performance.
//!
//! # Strategy
//!
//! Given a time range, calculate the optimal bucket size that produces ~60 data points:
//! - Bucket sizes are rounded to "nice" numbers (60s, 5m, 15m, 30m, 1h, 2h, 3h)
//! - This ensures human-readable buckets and efficient SQL grouping
//!
//! # Examples
//!
//! ```ignore
//! use stats_sv2::bucketing::AdaptiveBucketing;
//!
//! // 1-hour range should use 60-second buckets (60 points)
//! assert_eq!(AdaptiveBucketing::calculate_bucket_size(0, 3600, 60), 60);
//!
//! // 24-hour range should use 30-minute buckets (48 points)
//! assert_eq!(AdaptiveBucketing::calculate_bucket_size(0, 86400, 60), 1800);
//!
//! // 7-day range should use 3-hour buckets (56 points)
//! assert_eq!(AdaptiveBucketing::calculate_bucket_size(0, 604800, 60), 10800);
//! ```

/// Nice bucket sizes in seconds, in ascending order.
/// These represent human-readable time units that work well for bucketing.
const NICE_BUCKET_SIZES: &[u64] = &[60, 300, 900, 1800, 3600, 7200, 10800, 21600];

/// Calculate the optimal bucket size for a time range to maintain target point density.
///
/// # Arguments
///
/// * `from_timestamp` - Start of the time range (Unix seconds)
/// * `to_timestamp` - End of the time range (Unix seconds)
/// * `target_points` - Desired number of data points (typically 60)
///
/// # Returns
///
/// Bucket size in seconds. Always returns a value from `NICE_BUCKET_SIZES`.
/// If the calculated bucket size exceeds all nice sizes, returns the largest.
///
/// # Algorithm
///
/// 1. Calculate ideal bucket size: `time_range / target_points`
/// 2. Find the smallest "nice" bucket size that is >= ideal size
/// 3. Return that nice size
///
/// # Example
///
/// For a 24-hour range with 60 target points:
/// - Ideal = 86400 / 60 = 1440 seconds
/// - Smallest nice size >= 1440 is 1800 seconds (30 minutes)
/// - Returns 1800
pub fn calculate_bucket_size(from_timestamp: u64, to_timestamp: u64, target_points: u64) -> u64 {
    if target_points == 0 {
        return NICE_BUCKET_SIZES[0];
    }

    let time_range = to_timestamp.saturating_sub(from_timestamp);
    if time_range == 0 {
        return NICE_BUCKET_SIZES[0];
    }

    let ideal_bucket_size = time_range / target_points;

    NICE_BUCKET_SIZES
        .iter()
        .find(|&&size| size >= ideal_bucket_size)
        .copied()
        .unwrap_or(*NICE_BUCKET_SIZES.last().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_1hour_range() {
        // 1 hour = 3600 seconds / 60 points = 60 second buckets
        assert_eq!(calculate_bucket_size(0, 3600, 60), 60);
    }

    #[test]
    fn test_24hour_range() {
        // 24 hours = 86400 seconds / 60 points = 1440 seconds (ideal)
        // Rounds up to 1800 seconds (30 minutes)
        assert_eq!(calculate_bucket_size(0, 86400, 60), 1800);
    }

    #[test]
    fn test_7day_range() {
        // 7 days = 604800 seconds / 60 points = 10080 seconds (ideal)
        // Rounds up to 10800 seconds (3 hours)
        assert_eq!(calculate_bucket_size(0, 604800, 60), 10800);
    }

    #[test]
    fn test_30day_range() {
        // 30 days = 2592000 seconds / 60 points = 43200 seconds (ideal)
        // Rounds up to next available, which is beyond our list
        assert_eq!(calculate_bucket_size(0, 2592000, 60), 21600);
    }

    #[test]
    fn test_small_range() {
        // 10 seconds / 60 points = 0.166 seconds (ideal)
        // Rounds up to 60 seconds (minimum)
        assert_eq!(calculate_bucket_size(0, 10, 60), 60);
    }

    #[test]
    fn test_zero_range() {
        // Edge case: zero range should return minimum bucket size
        assert_eq!(calculate_bucket_size(100, 100, 60), 60);
    }

    #[test]
    fn test_zero_target_points() {
        // Edge case: zero target points should return minimum bucket size
        assert_eq!(calculate_bucket_size(0, 3600, 0), 60);
    }

    #[test]
    fn test_arbitrary_range_1() {
        // 12 hours = 43200 seconds / 60 points = 720 seconds (ideal)
        // Rounds up to 900 seconds (15 minutes)
        assert_eq!(calculate_bucket_size(0, 43200, 60), 900);
    }

    #[test]
    fn test_arbitrary_range_2() {
        // 3 hours = 10800 seconds / 60 points = 180 seconds (ideal)
        // Rounds up to 300 seconds (5 minutes)
        assert_eq!(calculate_bucket_size(0, 10800, 60), 300);
    }

    #[test]
    fn test_non_zero_start_timestamp() {
        // Should use time_range, not absolute timestamps
        // from=1000, to=4600 => range=3600 (1 hour) => 60 second buckets
        assert_eq!(calculate_bucket_size(1000, 4600, 60), 60);
    }

    #[test]
    fn test_resulting_point_counts() {
        // Verify our bucketing actually produces ~60 points per graph
        let test_cases = vec![
            (3600, 60, 60),      // 1h: 3600 / 60 = 60 points ✓
            (86400, 1800, 48),   // 24h: 86400 / 1800 = 48 points ✓
            (604800, 10800, 56), // 7d: 604800 / 10800 = 56 points ✓
        ];

        for (time_range, expected_bucket, expected_points) in test_cases {
            let bucket = calculate_bucket_size(0, time_range, 60);
            assert_eq!(bucket, expected_bucket);
            let points = time_range / bucket;
            assert_eq!(points, expected_points);
        }
    }

    #[test]
    fn test_edge_case_large_target_points() {
        // Should never produce more than the max nice bucket size
        let bucket = calculate_bucket_size(0, 3600, 1000);
        assert!(bucket <= 21600);
    }
}
