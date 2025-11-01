//! Pool statistics tracking for hashpool.
//!
//! This crate provides external stats collection that integrates with the
//! quote-dispatcher callback mechanism, keeping stats logic separate from
//! core pool code.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::RwLock;
use quote_dispatcher::QuoteEventCallback;

/// Get current Unix timestamp in seconds.
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Per-downstream stats tracked externally from SRI code.
pub struct DownstreamStats {
    pub shares_submitted: AtomicU64,
    pub quotes_created: AtomicU64,
    pub ehash_mined: AtomicU64,
    pub last_share_at: AtomicU64,
    pub sum_difficulty: AtomicU64, // Fixed-point: multiply by 2^32 to store f64 as u64
    pub shares_in_window: AtomicU64,
}

impl DownstreamStats {
    pub fn new() -> Self {
        Self {
            shares_submitted: AtomicU64::new(0),
            quotes_created: AtomicU64::new(0),
            ehash_mined: AtomicU64::new(0),
            last_share_at: AtomicU64::new(0),
            sum_difficulty: AtomicU64::new(0),
            shares_in_window: AtomicU64::new(0),
        }
    }

    /// Track a standard share (no quote).
    pub fn record_share(&self) {
        let now = unix_timestamp();
        self.shares_submitted.fetch_add(1, Ordering::Relaxed);
        self.last_share_at.store(now, Ordering::Relaxed);
    }

    /// Record a share with its difficulty for time-series metrics.
    pub fn record_share_with_difficulty(&self, difficulty: f64) {
        let now = unix_timestamp();
        self.shares_submitted.fetch_add(1, Ordering::Relaxed);
        self.shares_in_window.fetch_add(1, Ordering::Relaxed);

        // Convert f64 difficulty to fixed-point u64 (multiply by 2^32)
        let difficulty_fixed = (difficulty * (2u64.pow(32) as f64)) as u64;
        self.sum_difficulty.fetch_add(difficulty_fixed, Ordering::Relaxed);

        self.last_share_at.store(now, Ordering::Relaxed);
    }

    /// Get the sum of difficulties as f64.
    pub fn get_sum_difficulty(&self) -> f64 {
        let fixed = self.sum_difficulty.load(Ordering::Relaxed);
        fixed as f64 / (2u64.pow(32) as f64)
    }

    /// Get the number of shares in current window.
    pub fn shares_in_current_window(&self) -> u64 {
        self.shares_in_window.load(Ordering::Relaxed)
    }
}

impl Default for DownstreamStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Global stats registry for all downstreams.
pub struct PoolStatsRegistry {
    stats: RwLock<HashMap<u32, Arc<DownstreamStats>>>,
}

impl PoolStatsRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            stats: RwLock::new(HashMap::new()),
        })
    }

    pub fn register_downstream(&self, downstream_id: u32) -> Arc<DownstreamStats> {
        let stats = Arc::new(DownstreamStats::new());
        self.stats.write().insert(downstream_id, stats.clone());
        stats
    }

    pub fn unregister_downstream(&self, downstream_id: u32) {
        self.stats.write().remove(&downstream_id);
    }

    pub fn get_stats(&self, downstream_id: u32) -> Option<Arc<DownstreamStats>> {
        self.stats.read().get(&downstream_id).cloned()
    }

    pub fn snapshot(&self) -> HashMap<u32, (u64, u64, u64, Option<u64>)> {
        self.stats
            .read()
            .iter()
            .map(|(id, stats)| {
                let shares = stats.shares_submitted.load(Ordering::Relaxed);
                let quotes = stats.quotes_created.load(Ordering::Relaxed);
                let ehash = stats.ehash_mined.load(Ordering::Relaxed);
                let last_share = stats.last_share_at.load(Ordering::Relaxed);
                let last_share_opt = if last_share > 0 {
                    Some(last_share)
                } else {
                    None
                };
                (*id, (shares, quotes, ehash, last_share_opt))
            })
            .collect()
    }
}

impl Default for PoolStatsRegistry {
    fn default() -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
        }
    }
}

/// Callback that updates stats when quotes are created.
pub struct StatsCallback {
    stats: Arc<DownstreamStats>,
}

impl StatsCallback {
    pub fn new(stats: Arc<DownstreamStats>) -> Self {
        Self { stats }
    }
}

impl QuoteEventCallback for StatsCallback {
    fn on_quote_created(&self, _channel_id: u32, amount: u64) {
        let now = unix_timestamp();
        self.stats.shares_submitted.fetch_add(1, Ordering::Relaxed);
        self.stats.quotes_created.fetch_add(1, Ordering::Relaxed);
        self.stats.ehash_mined.fetch_add(amount, Ordering::Relaxed);
        self.stats.last_share_at.store(now, Ordering::Relaxed);
    }
}
