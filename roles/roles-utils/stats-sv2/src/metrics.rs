//! Metrics calculation utilities.

/// Calculate hashrate from share difficulties.
///
/// # Arguments
///
/// * `sum_difficulty` - Sum of all share difficulties in the window
/// * `window_seconds` - Duration of the window in seconds
///
/// # Returns
///
/// Hashrate in hashes per second (H/s)
pub fn derive_hashrate(sum_difficulty: f64, window_seconds: u64) -> f64 {
    if window_seconds == 0 {
        0.0
    } else {
        sum_difficulty / window_seconds as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_hashrate_basic() {
        // 100 difficulty over 10 seconds = 10 H/s
        let hashrate = derive_hashrate(100.0, 10);
        assert_eq!(hashrate, 10.0);
    }

    #[test]
    fn test_derive_hashrate_zero_window() {
        let hashrate = derive_hashrate(100.0, 0);
        assert_eq!(hashrate, 0.0);
    }

    #[test]
    fn test_derive_hashrate_fractional() {
        // 25.5 difficulty over 5 seconds = 5.1 H/s
        let hashrate = derive_hashrate(25.5, 5);
        assert!((hashrate - 5.1).abs() < 0.0001);
    }

    #[test]
    fn test_derive_hashrate_large_values() {
        // Typical case: large difficulty sums
        // 1M difficulty / 10 = 100k H/s
        let hashrate = derive_hashrate(1_000_000.0, 10);
        assert_eq!(hashrate, 100_000.0);
    }
}
