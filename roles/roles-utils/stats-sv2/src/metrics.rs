//! Metrics calculation utilities.

/// Conversion factor from Bitcoin difficulty units to hashes.
/// Each share at difficulty 1 represents 2^32 hashes of work (Bitcoin standard).
const HASHES_PER_DIFFICULTY: f64 = 4_294_967_296.0;

/// Calculate hashrate from share difficulties.
///
/// # Arguments
///
/// * `sum_difficulty` - Sum of all share difficulties in the window (Bitcoin difficulty units)
/// * `window_seconds` - Duration of the window in seconds
///
/// # Returns
///
/// Hashrate in hashes per second (H/s)
///
/// # Formula
///
/// Hashrate = (sum_difficulty * 2^32) / window_seconds
///
/// Bitcoin difficulty is defined as a ratio (max_target / current_target).
/// Each share at difficulty D represents 2^32 * D hashes of computational work.
pub fn derive_hashrate(sum_difficulty: f64, window_seconds: u64) -> f64 {
    if window_seconds == 0 {
        0.0
    } else {
        (sum_difficulty * HASHES_PER_DIFFICULTY) / window_seconds as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_hashrate_basic() {
        // 100 difficulty over 10 seconds
        // = (100 * 2^32) / 10 = 42,949,672,960 H/s = 42.9 GH/s
        let hashrate = derive_hashrate(100.0, 10);
        assert_eq!(hashrate, 42_949_672_960.0);
    }

    #[test]
    fn test_derive_hashrate_zero_window() {
        let hashrate = derive_hashrate(100.0, 0);
        assert_eq!(hashrate, 0.0);
    }

    #[test]
    fn test_derive_hashrate_fractional() {
        // 25.5 difficulty over 5 seconds
        // = (25.5 * 2^32) / 5 = 2,195,312,681 H/s
        let hashrate = derive_hashrate(25.5, 5);
        let expected = (25.5 * HASHES_PER_DIFFICULTY) / 5.0;
        assert!((hashrate - expected).abs() < 0.0001);
    }

    #[test]
    fn test_derive_hashrate_large_values() {
        // Typical case: large difficulty sums
        // 1M difficulty over 10 seconds
        // = (1000000 * 2^32) / 10 = 429,496,729,600,000 H/s = 429.5 TH/s
        let hashrate = derive_hashrate(1_000_000.0, 10);
        assert_eq!(hashrate, 429_496_729_600_000.0);
    }

    #[test]
    fn test_derive_hashrate_realistic_miner() {
        // Example: 1.2 TH/s BitAxe miner
        // Need difficulty sum that produces ~1.2 TH/s over 10 seconds:
        // 1.2e12 = (difficulty * 2^32) / 10
        // difficulty ≈ 2796.2
        let difficulty_sum = 2796.2;
        let window = 10;
        let hashrate = derive_hashrate(difficulty_sum, window);
        let expected_th_s = 1.2e12;
        // Allow ~1% tolerance
        assert!((hashrate - expected_th_s).abs() / expected_th_s < 0.01);
    }
}
