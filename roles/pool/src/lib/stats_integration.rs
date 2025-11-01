//! Stats integration for Pool
//!
//! Implements `StatsSnapshotProvider` trait for Pool to send snapshot updates
//! to the stats service for web dashboard consumption.

use super::mining_pool::Pool;
use stats::stats_adapter::{
    PoolStatus, ProxyConnection, ServiceConnection, ServiceType, StatsSnapshotProvider,
};
use stats_sv2::types::{DownstreamSnapshot, ServiceSnapshot, ServiceType as MetricsServiceType, unix_timestamp};
use std::time::SystemTime;

// Unix timestamp helper (kept for potential future use)
fn _unix_timestamp_helper() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl StatsSnapshotProvider for Pool {
    type Snapshot = PoolStatus;

    fn get_snapshot(&self) -> PoolStatus {
        // Get service connections (pool, mint, jd-server if connected)
        let mut services = Vec::new();

        // Add the pool itself as first service
        // In this version, we use a default address as Pool doesn't store its listen address
        services.push(ServiceConnection {
            service_type: ServiceType::Pool,
            address: "0.0.0.0:34254".to_string(),
        });

        // Add mint connections if they exist
        if let Some(ref _mint_connection) = self.mint_connection {
            // Mint connection exists, add it to services
            services.push(ServiceConnection {
                service_type: ServiceType::Mint,
                address: "127.0.0.1:34260".to_string(),
            });
        }

        // Add JD-Server if configured
        if let Some(ref jds_address) = self.jd_server_address {
            services.push(ServiceConnection {
                service_type: ServiceType::JobDeclarator,
                address: jds_address.clone(),
            });
        }

        // Get stats snapshot from registry for all downstreams
        let stats_snapshot = self.stats_registry.snapshot();

        // Collect all downstream proxy connections
        let mut downstream_proxies = Vec::new();

        for (id, downstream) in &self.downstreams {
            // Try to get downstream info including address and custom work flag
            if let Ok((address, requires_custom_work)) = downstream.safe_lock(|d| {
                (d.address.to_string(), d.requires_custom_work)
            }) {
                // Lookup stats from registry
                let (shares, quotes, ehash, last_share) =
                    stats_snapshot.get(id).copied().unwrap_or((0, 0, 0, None));

                // Track both Translators and JDCs
                // JDC: requires_custom_work = true (Job Declaration Client)
                // Translator: requires_custom_work = false (Mining Protocol proxy)
                downstream_proxies.push(ProxyConnection {
                    id: *id,
                    address,
                    channels: Vec::new(), // Would need to track channel mapping
                    shares_submitted: shares,
                    quotes_created: quotes,
                    ehash_mined: ehash,
                    last_share_at: last_share,
                    work_selection: requires_custom_work, // JDC has work_selection = true
                });
            }
        }

        PoolStatus {
            services,
            downstream_proxies,
            listen_address: "0.0.0.0:34254".to_string(),
            timestamp: unix_timestamp(),
        }
    }
}

impl Pool {
    /// Get a ServiceSnapshot for time-series metrics collection.
    pub fn get_metrics_snapshot(&self) -> ServiceSnapshot {
        let mut downstreams = Vec::new();

        for (id, downstream) in &self.downstreams {
            if let Ok((address, _requires_custom_work)) = downstream.safe_lock(|d| {
                (d.address.to_string(), d.requires_custom_work)
            }) {
                if let Some(stats) = self.stats_registry.get_stats(*id) {
                    downstreams.push(DownstreamSnapshot {
                        downstream_id: *id,
                        name: format!("translator_{}", id),
                        address,
                        shares_lifetime: stats.shares_submitted.load(std::sync::atomic::Ordering::Relaxed),
                        shares_in_window: stats.shares_in_current_window(),
                        sum_difficulty_in_window: stats.get_sum_difficulty(),
                        timestamp: unix_timestamp(),
                    });
                }
            }
        }

        ServiceSnapshot {
            service_type: MetricsServiceType::Pool,
            downstreams,
            timestamp: unix_timestamp(),
        }
    }
}
