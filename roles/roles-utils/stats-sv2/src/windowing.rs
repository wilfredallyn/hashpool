//! Shared windowed metrics collection for time-series hashrate calculations.
//!
//! Provides a unified `WindowedMetricsCollector` used by both Translator and Pool
//! to track shares with timestamps and calculate windowed difficulty sums.
//! This ensures both services use the same window calculation logic.

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current Unix timestamp in seconds.
pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Shared metrics collector that tracks shares within a rolling time window.
///
/// Stores shares with Unix timestamps and calculates windowed sums.
/// Automatically removes shares outside the window to bound memory usage.
///
/// # Example
/// ```ignore
/// let mut collector = WindowedMetricsCollector::new(10); // 10-second window
/// collector.record_share(100.0); // difficulty 100
/// let sum = collector.sum_difficulty_in_window(); // sum of shares in last 10s
/// ```
#[derive(Debug, Clone)]
pub struct WindowedMetricsCollector {
    // Shares stored as (unix_timestamp_secs, difficulty)
    shares: Vec<(u64, f64)>,
    window_seconds: u64,
}

impl WindowedMetricsCollector {
    /// Create a new collector with the specified window size in seconds.
    pub fn new(window_seconds: u64) -> Self {
        Self {
            shares: Vec::new(),
            window_seconds,
        }
    }

    /// Record a share with its difficulty. Uses current Unix timestamp.
    pub fn record_share(&mut self, difficulty: f64) {
        let now = unix_timestamp();
        self.shares.push((now, difficulty));

        // Cleanup shares outside the window to prevent unbounded growth
        // Keep shares newer than: now - window_seconds
        let cutoff = now.saturating_sub(self.window_seconds);
        self.shares.retain(|(ts, _)| *ts > cutoff);
    }

    /// Get the sum of difficulties for shares in the current window.
    /// Only includes shares from the last `window_seconds` seconds.
    pub fn sum_difficulty_in_window(&self) -> f64 {
        let now = unix_timestamp();
        let cutoff = now.saturating_sub(self.window_seconds);

        self.shares
            .iter()
            .filter(|(ts, _)| *ts > cutoff)
            .map(|(_, difficulty)| difficulty)
            .sum()
    }

    /// Get the count of shares in the current window.
    pub fn shares_in_window(&self) -> u64 {
        let now = unix_timestamp();
        let cutoff = now.saturating_sub(self.window_seconds);

        self.shares
            .iter()
            .filter(|(ts, _)| *ts > cutoff)
            .count() as u64
    }

    /// Get all recent shares (used mainly for testing/debugging).
    pub fn recent_shares(&self) -> &[(u64, f64)] {
        &self.shares
    }

    /// Clear all recorded shares (used for testing).
    pub fn clear(&mut self) {
        self.shares.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_record_and_query_basic() {
        let mut collector = WindowedMetricsCollector::new(10);
        collector.record_share(100.0);

        assert_eq!(collector.shares_in_window(), 1);
        assert_eq!(collector.sum_difficulty_in_window(), 100.0);
    }

    #[test]
    fn test_multiple_shares_same_window() {
        let mut collector = WindowedMetricsCollector::new(10);
        collector.record_share(100.0);
        collector.record_share(50.0);
        collector.record_share(25.0);

        assert_eq!(collector.shares_in_window(), 3);
        assert_eq!(collector.sum_difficulty_in_window(), 175.0);
    }

    #[test]
    fn test_window_filtering() {
        // This test uses small time windows and sleeps to test filtering
        // Note: In real code, we'd use timestamped shares directly
        let mut collector = WindowedMetricsCollector::new(1); // 1-second window
        collector.record_share(100.0);

        // Immediately query - share is in window
        assert_eq!(collector.shares_in_window(), 1);

        // Sleep 1.1 seconds to exceed window
        thread::sleep(Duration::from_millis(1100));
        collector.record_share(50.0); // Add new share to trigger cleanup

        // Old share should be filtered out
        assert_eq!(collector.shares_in_window(), 1);
        assert_eq!(collector.sum_difficulty_in_window(), 50.0);
    }

    #[test]
    fn test_zero_window() {
        let mut collector = WindowedMetricsCollector::new(0);
        collector.record_share(100.0);

        // With zero window, should filter everything (current_ts > ts is never true)
        // Actually with saturating_sub(0), cutoff == now, so filter is ts > now
        // which is always false for shares just recorded
        assert_eq!(collector.shares_in_window(), 0);
    }

    #[test]
    fn test_multiple_windows() {
        let mut c1 = WindowedMetricsCollector::new(10);
        let mut c2 = WindowedMetricsCollector::new(10);

        c1.record_share(100.0);
        c2.record_share(50.0);

        assert_eq!(c1.sum_difficulty_in_window(), 100.0);
        assert_eq!(c2.sum_difficulty_in_window(), 50.0);
    }

    #[test]
    fn test_empty_collector() {
        let collector = WindowedMetricsCollector::new(10);

        assert_eq!(collector.shares_in_window(), 0);
        assert_eq!(collector.sum_difficulty_in_window(), 0.0);
    }

    #[test]
    fn test_large_difficulties() {
        let mut collector = WindowedMetricsCollector::new(10);
        collector.record_share(1_000_000_000.0);
        collector.record_share(500_000_000.0);

        assert_eq!(collector.shares_in_window(), 2);
        assert_eq!(collector.sum_difficulty_in_window(), 1_500_000_000.0);
    }

    #[test]
    fn test_clear() {
        let mut collector = WindowedMetricsCollector::new(10);
        collector.record_share(100.0);
        collector.record_share(50.0);

        assert_eq!(collector.shares_in_window(), 2);

        collector.clear();
        assert_eq!(collector.shares_in_window(), 0);
        assert_eq!(collector.sum_difficulty_in_window(), 0.0);
    }
}
