//! Stats integration for Translator
//!
//! Implements `StatsSnapshotProvider` trait for Translator to send snapshot updates
//! to the stats service for web dashboard consumption.

use super::TranslatorSv2;
use stats::stats_adapter::{MinerInfo, PoolConnection, TranslatorStatus, StatsSnapshotProvider};
use stats_sv2::types::{DownstreamSnapshot, ServiceSnapshot, ServiceType, unix_timestamp};
use std::time::SystemTime;

// Unix timestamp helper (kept for potential future use)
fn _unix_timestamp_helper() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl StatsSnapshotProvider for TranslatorSv2 {
    type Snapshot = TranslatorStatus;

    fn get_snapshot(&self) -> TranslatorStatus {
        // Get wallet balance if wallet is available
        let ehash_balance = if let Some(ref wallet) = self.wallet {
            // Try to get balance synchronously without blocking
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    match wallet.total_balance().await {
                        Ok(amount) => u64::from(amount),
                        Err(_) => 0,
                    }
                })
            })
        } else {
            0
        };

        // Get upstream pool connection info from config
        // In 1.5.0, there can be multiple upstreams, so we'll use the first one for now
        let upstream_pool = self.config.upstreams.first().map(|upstream| PoolConnection {
            address: format!("{}:{}", upstream.address, upstream.port),
        });

        // Get miner stats from tracker
        let downstream_miners: Vec<MinerInfo> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let all_miners = self.miner_tracker.get_all_miners().await;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                all_miners.into_iter().map(|miner| {
                    let elapsed_secs = miner.connected_time.elapsed().as_secs();
                    let connected_timestamp = now.saturating_sub(elapsed_secs);
                    let address = if self.config.redact_ip {
                        "REDACTED".to_string()
                    } else {
                        miner.address.to_string()
                    };
                    MinerInfo {
                        name: miner.name,
                        id: miner.id,
                        address,
                        hashrate: miner.estimated_hashrate,
                        shares_submitted: miner.shares_submitted,
                        connected_at: connected_timestamp,
                    }
                }).collect()
            })
        });

        // Get blockchain network from environment variable
        let blockchain_network = std::env::var("BITCOIND_NETWORK")
            .unwrap_or_else(|_| "unknown".to_string())
            .to_lowercase();

        TranslatorStatus {
            ehash_balance,
            upstream_pool,
            downstream_miners,
            blockchain_network,
            timestamp: unix_timestamp(),
        }
    }
}

impl TranslatorSv2 {
    /// Get a ServiceSnapshot for time-series metrics collection.
    pub fn get_metrics_snapshot(&self) -> ServiceSnapshot {
        let downstreams = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.miner_tracker.get_all_miners().await
                    .into_iter()
                    .map(|miner| DownstreamSnapshot {
                        downstream_id: miner.id,
                        name: miner.name,
                        address: if self.config.redact_ip {
                            "REDACTED".to_string()
                        } else {
                            miner.address.to_string()
                        },
                        shares_lifetime: miner.shares_submitted,
                        shares_in_window: miner.shares_in_window,
                        sum_difficulty_in_window: miner.sum_difficulty_in_window,
                        timestamp: unix_timestamp(),
                    })
                    .collect()
            })
        });

        ServiceSnapshot {
            service_type: ServiceType::Translator,
            downstreams,
            timestamp: unix_timestamp(),
        }
    }
}
