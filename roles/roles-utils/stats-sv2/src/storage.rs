//! SQLite storage backend for time-series metrics.

use crate::bucketing::calculate_bucket_size;
use crate::types::{DownstreamSnapshot, HashratePoint};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite, Row};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// Storage abstraction for metrics data.
#[async_trait::async_trait]
pub trait StatsStorage: Send + Sync {
    /// Store a downstream snapshot.
    async fn store_downstream(&self, downstream: &DownstreamSnapshot) -> Result<()>;

    /// Query hashrate for a specific downstream in a time range.
    async fn query_hashrate(
        &self,
        downstream_id: u32,
        from_timestamp: u64,
        to_timestamp: u64,
    ) -> Result<Vec<HashratePoint>>;

    /// Query aggregate hashrate across all downstreams.
    async fn query_aggregate_hashrate(
        &self,
        from_timestamp: u64,
        to_timestamp: u64,
    ) -> Result<Vec<HashratePoint>>;
}

/// SQLite-backed storage implementation.
pub struct SqliteStorage {
    pool: Pool<Sqlite>,
}

impl SqliteStorage {
    /// Create a new SQLite storage instance.
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_path = db_path.as_ref();

        // Create parent directories if they don't exist
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection_options = SqliteConnectOptions::from_str(
            &format!("sqlite://{}", db_path.display())
        )?
        .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connection_options)
            .await?;

        let storage = Self { pool };
        storage.init_schema().await?;

        Ok(storage)
    }

    /// Initialize the database schema.
    async fn init_schema(&self) -> Result<()> {
        // Create downstreams table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS downstreams (
                id INTEGER PRIMARY KEY,
                downstream_id INTEGER NOT NULL UNIQUE,
                name TEXT NOT NULL,
                address TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create hashrate_samples table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS hashrate_samples (
                timestamp INTEGER NOT NULL,
                downstream_id INTEGER NOT NULL,
                shares_in_window INTEGER NOT NULL,
                sum_difficulty REAL NOT NULL,
                shares_lifetime INTEGER NOT NULL,
                window_seconds INTEGER NOT NULL,

                PRIMARY KEY (timestamp, downstream_id),
                FOREIGN KEY (downstream_id) REFERENCES downstreams(downstream_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for efficient queries
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_timestamp_downstream
            ON hashrate_samples(timestamp, downstream_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_downstream_timestamp
            ON hashrate_samples(downstream_id, timestamp)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Store or update a downstream's metadata.
    async fn upsert_downstream(&self, downstream: &DownstreamSnapshot) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO downstreams (downstream_id, name, address)
            VALUES (?, ?, ?)
            ON CONFLICT(downstream_id) DO UPDATE SET
                name = excluded.name,
                address = excluded.address
            "#,
        )
        .bind(downstream.downstream_id as i32)
        .bind(&downstream.name)
        .bind(&downstream.address)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Convert raw query rows to HashratePoint with bucketed aggregation.
    fn aggregate_rows_to_hashrate_points(rows: Vec<sqlx::sqlite::SqliteRow>) -> Vec<HashratePoint> {
        rows.iter()
            .enumerate()
            .map(|(idx, row)| {
                let bucket_timestamp = row.get::<i64, _>("bucket_timestamp") as u64;
                let total_difficulty = row.get::<f64, _>("total_difficulty");
                let sample_count = row.get::<i64, _>("sample_count") as u64;
                let bucket_duration_seconds = row.get::<i64, _>("bucket_duration_seconds") as u64;
                let window_seconds = row.get::<i64, _>("window_seconds") as u64;

                // Calculate average hashrate across samples
                // Average the difficulty first, then derive hashrate
                let avg_difficulty = total_difficulty / sample_count as f64;

                // Use the actual bucket duration (time span from first to last sample in bucket).
                // If bucket_duration is 0 (single sample), fall back to the sample's window_seconds.
                // This preserves the measurement window for single-sample buckets while using
                // actual duration for multi-sample aggregations.
                let effective_duration = if bucket_duration_seconds > 0 {
                    bucket_duration_seconds
                } else {
                    window_seconds
                };

                let hashrate = crate::metrics::derive_hashrate(avg_difficulty, effective_duration);

                // Log first few and last few buckets for debugging
                if idx < 3 || idx >= rows.len().saturating_sub(3) {
                    tracing::debug!(
                        "Bucket[{}] timestamp={}, total_diff={:.2}, samples={}, bucket_duration={}, window_secs={}, avg_diff={:.2}, effective_duration={}, hashrate={:.2}H/s",
                        idx,
                        bucket_timestamp,
                        total_difficulty,
                        sample_count,
                        bucket_duration_seconds,
                        window_seconds,
                        avg_difficulty,
                        effective_duration,
                        hashrate
                    );
                }

                HashratePoint {
                    timestamp: bucket_timestamp,
                    hashrate_hs: hashrate,
                }
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl StatsStorage for SqliteStorage {
    async fn store_downstream(&self, downstream: &DownstreamSnapshot) -> Result<()> {
        // Update downstream metadata
        self.upsert_downstream(downstream).await?;

        // Store the hashrate sample (convert u64 to i64 for SQLite)
        tracing::debug!(
            "Storing downstream snapshot: downstream_id={}, timestamp={}, shares_in_window={}, sum_difficulty={}, window_seconds={}",
            downstream.downstream_id,
            downstream.timestamp,
            downstream.shares_in_window,
            downstream.sum_difficulty_in_window,
            downstream.window_seconds
        );

        sqlx::query(
            r#"
            INSERT INTO hashrate_samples
            (timestamp, downstream_id, shares_in_window, sum_difficulty, shares_lifetime, window_seconds)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(downstream.timestamp as i64)
        .bind(downstream.downstream_id as i32)
        .bind(downstream.shares_in_window as i64)
        .bind(downstream.sum_difficulty_in_window)
        .bind(downstream.shares_lifetime as i64)
        .bind(downstream.window_seconds as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn query_hashrate(
        &self,
        downstream_id: u32,
        from_timestamp: u64,
        to_timestamp: u64,
    ) -> Result<Vec<HashratePoint>> {
        // Calculate adaptive bucket size to maintain ~60 data points per graph
        let bucket_seconds = calculate_bucket_size(from_timestamp, to_timestamp, 60);

        // Aggregate data into calculated buckets to smooth the graph
        // Key fix: Use the LATEST snapshot per bucket (MAX timestamp) instead of summing
        // This prevents overcounting since each snapshot's sum_difficulty is already
        // aggregated over a 60-second measurement window.
        let rows = sqlx::query(
            r#"
            WITH bucketed AS (
                SELECT
                    timestamp,
                    downstream_id,
                    sum_difficulty,
                    window_seconds,
                    (timestamp / ?) * ? AS bucket_timestamp
                FROM hashrate_samples
                WHERE downstream_id = ? AND timestamp >= ? AND timestamp <= ?
            ), ranked AS (
                SELECT
                    bucket_timestamp,
                    timestamp,
                    sum_difficulty,
                    window_seconds,
                    ROW_NUMBER() OVER (
                        PARTITION BY bucket_timestamp
                        ORDER BY timestamp DESC
                    ) AS rn,
                    MAX(timestamp) OVER (PARTITION BY bucket_timestamp) -
                        MIN(timestamp) OVER (PARTITION BY bucket_timestamp) AS bucket_duration_seconds
                FROM bucketed
            )
            SELECT
                bucket_timestamp,
                CAST(sum_difficulty AS REAL) AS total_difficulty,
                1 AS sample_count,
                bucket_duration_seconds,
                window_seconds
            FROM ranked
            WHERE rn = 1
            ORDER BY bucket_timestamp ASC
            "#,
        )
        .bind(bucket_seconds as i64)
        .bind(bucket_seconds as i64)
        .bind(downstream_id as i32)
        .bind(from_timestamp as i64)
        .bind(to_timestamp as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(Self::aggregate_rows_to_hashrate_points(rows))
    }

    async fn query_aggregate_hashrate(
        &self,
        from_timestamp: u64,
        to_timestamp: u64,
    ) -> Result<Vec<HashratePoint>> {
        // Calculate adaptive bucket size to maintain ~60 data points per graph
        let bucket_seconds = calculate_bucket_size(from_timestamp, to_timestamp, 60);

        tracing::info!(
            "Query aggregate hashrate: from={}, to={}, bucket_seconds={}, time_range_seconds={}",
            from_timestamp,
            to_timestamp,
            bucket_seconds,
            to_timestamp.saturating_sub(from_timestamp)
        );

        // Aggregate data into calculated buckets to smooth the graph
        // Key fix: Use the LATEST snapshot per bucket (MAX timestamp) instead of summing
        // This prevents overcounting since each snapshot's sum_difficulty is already
        // aggregated over a 60-second measurement window.
        let rows = sqlx::query(
            r#"
            WITH bucketed AS (
                SELECT
                    timestamp,
                    downstream_id,
                    sum_difficulty,
                    window_seconds,
                    (timestamp / ?) * ? AS bucket_timestamp
                FROM hashrate_samples
                WHERE timestamp >= ? AND timestamp <= ?
            ), ranked AS (
                SELECT
                    bucket_timestamp,
                    downstream_id,
                    sum_difficulty,
                    window_seconds,
                    ROW_NUMBER() OVER (
                        PARTITION BY downstream_id, bucket_timestamp
                        ORDER BY timestamp DESC
                    ) AS rn
                FROM bucketed
            )
            SELECT
                bucket_timestamp,
                SUM(CAST(sum_difficulty AS REAL)) AS total_difficulty,
                1 AS sample_count,
                0 AS bucket_duration_seconds,
                MAX(window_seconds) AS window_seconds
            FROM ranked
            WHERE rn = 1
            GROUP BY bucket_timestamp
            ORDER BY bucket_timestamp ASC
            "#,
        )
        .bind(bucket_seconds as i64)
        .bind(bucket_seconds as i64)
        .bind(from_timestamp as i64)
        .bind(to_timestamp as i64)
        .fetch_all(&self.pool)
        .await?;

        tracing::info!(
            "Query returned {} rows from database",
            rows.len()
        );

        let points = Self::aggregate_rows_to_hashrate_points(rows);

        tracing::info!(
            "Aggregated to {} hashrate points; min={:.2}H/s, max={:.2}H/s",
            points.len(),
            points.iter().map(|p| p.hashrate_hs).fold(f64::INFINITY, f64::min),
            points.iter().map(|p| p.hashrate_hs).fold(0.0, f64::max)
        );

        Ok(points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SqliteStorage::new(&db_path).await.unwrap();

        // Verify tables exist
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='downstreams'"
        )
        .fetch_one(&storage.pool)
        .await
        .unwrap();

        assert_eq!(result.0, 1);
    }

    #[tokio::test]
    async fn test_store_and_query_downstream() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SqliteStorage::new(&db_path).await.unwrap();

        // Use timestamp 6000
        // Query range: 0 to 7000 (7000s) / 60 points = 116.67s bucket → rounds to 300s (5m)
        let downstream = DownstreamSnapshot {
            downstream_id: 1,
            name: "test_miner".to_string(),
            address: "192.168.1.1:4444".to_string(),
            shares_lifetime: 100,
            shares_in_window: 10,
            sum_difficulty_in_window: 100.0,
            window_seconds: 10,
            timestamp: 6000,
        };

        storage.store_downstream(&downstream).await.unwrap();

        let results = storage.query_hashrate(1, 0, 7000).await.unwrap();
        assert_eq!(results.len(), 1);
        // 6000 / 300 * 300 = 6000 (bucket boundary at 5-minute intervals)
        assert_eq!(results[0].timestamp, 6000);
        // (100 * 2^32) / 10 seconds = 42,949,672,960 H/s
        assert_eq!(results[0].hashrate_hs, 42_949_672_960.0);
    }

    #[tokio::test]
    async fn test_multiple_samples_same_downstream() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SqliteStorage::new(&db_path).await.unwrap();

        // Store samples at 10-second intervals within a bucket
        // Query range: 6000 to 6060 (60s) / 60 points = 1s bucket → rounds to 60s
        for i in 0..6 {
            let downstream = DownstreamSnapshot {
                downstream_id: 1,
                name: "miner_1".to_string(),
                address: "192.168.1.1:4444".to_string(),
                shares_lifetime: (i + 1) * 10,
                shares_in_window: 10,
                sum_difficulty_in_window: 1000.0,
                window_seconds: 10,
                timestamp: 6000 + (i as u64 * 10),
            };
            storage.store_downstream(&downstream).await.unwrap();
        }

        // Query samples - should be aggregated into 60-second bucket
        let results = storage.query_hashrate(1, 6000, 6060).await.unwrap();
        // All 6 samples fall into the same bucket
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp, 6000);
        // Average of 6 samples of 1000 difficulty each = 1000
        // Bucket duration: MAX(timestamp) - MIN(timestamp) = 6050 - 6000 = 50 seconds
        // (1000 * 2^32) / 50 seconds = 85,899,345,920 H/s
        assert_eq!(results[0].hashrate_hs, 85_899_345_920.0);
    }

    #[tokio::test]
    async fn test_multiple_downstreams() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SqliteStorage::new(&db_path).await.unwrap();

        // Store samples from multiple downstreams at the same timestamp
        let timestamp = 6000;

        let down1 = DownstreamSnapshot {
            downstream_id: 1,
            name: "miner_1".to_string(),
            address: "192.168.1.1:4444".to_string(),
            shares_lifetime: 100,
            shares_in_window: 10,
            sum_difficulty_in_window: 1000.0,
            window_seconds: 10,
            timestamp,
        };

        let down2 = DownstreamSnapshot {
            downstream_id: 2,
            name: "miner_2".to_string(),
            address: "192.168.1.2:4444".to_string(),
            shares_lifetime: 50,
            shares_in_window: 5,
            sum_difficulty_in_window: 500.0,
            window_seconds: 10,
            timestamp,
        };

        storage.store_downstream(&down1).await.unwrap();
        storage.store_downstream(&down2).await.unwrap();

        // Query each downstream separately
        let results1 = storage.query_hashrate(1, 6000, 7000).await.unwrap();
        let results2 = storage.query_hashrate(2, 6000, 7000).await.unwrap();

        assert_eq!(results1.len(), 1);
        // (1000 * 2^32) / 10 seconds = 429,496,729,600 H/s
        assert_eq!(results1[0].hashrate_hs, 429_496_729_600.0);

        assert_eq!(results2.len(), 1);
        // (500 * 2^32) / 10 seconds = 214,748,364,800 H/s
        assert_eq!(results2[0].hashrate_hs, 214_748_364_800.0);
    }

    #[tokio::test]
    async fn test_aggregate_hashrate() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SqliteStorage::new(&db_path).await.unwrap();

        let timestamp = 6000;

        // Store samples from two miners at the same timestamp
        let down1 = DownstreamSnapshot {
            downstream_id: 1,
            name: "miner_1".to_string(),
            address: "192.168.1.1:4444".to_string(),
            shares_lifetime: 100,
            shares_in_window: 10,
            sum_difficulty_in_window: 1000.0,
            window_seconds: 10,
            timestamp,
        };

        let down2 = DownstreamSnapshot {
            downstream_id: 2,
            name: "miner_2".to_string(),
            address: "192.168.1.2:4444".to_string(),
            shares_lifetime: 50,
            shares_in_window: 5,
            sum_difficulty_in_window: 1000.0,
            window_seconds: 10,
            timestamp,
        };

        storage.store_downstream(&down1).await.unwrap();
        storage.store_downstream(&down2).await.unwrap();

        let results = storage.query_aggregate_hashrate(6000, 7000).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp, 6000);
        // Aggregate query should sum downstream contributions.
        // Two downstreams * 1000 difficulty each = 2000 difficulty in the bucket.
        // (2000 * 2^32) / 10 seconds = 858,993,459,200 H/s.
        assert_eq!(results[0].hashrate_hs, 858_993_459_200.0);
    }

    #[tokio::test]
    async fn test_empty_query() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SqliteStorage::new(&db_path).await.unwrap();

        // Query without storing anything
        let results = storage.query_hashrate(1, 1000, 2000).await.unwrap();
        assert_eq!(results.len(), 0);

        let results = storage.query_aggregate_hashrate(1000, 2000).await.unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_timestamp_range_filtering() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SqliteStorage::new(&db_path).await.unwrap();

        // Store samples at different timestamps spanning multiple buckets
        // Query range: 6000 to 6250 (250s) / 60 points = 4.16s bucket → rounds to 60s
        // So we expect samples to fall into 60-second boundaries
        for ts in [6000u64, 6010, 6120, 6130].iter() {
            let downstream = DownstreamSnapshot {
                downstream_id: 1,
                name: "miner_1".to_string(),
                address: "192.168.1.1:4444".to_string(),
                shares_lifetime: 100,
                shares_in_window: 10,
                sum_difficulty_in_window: 100.0,
                window_seconds: 10,
                timestamp: *ts,
            };
            storage.store_downstream(&downstream).await.unwrap();
        }

        // Query with specific range
        let results = storage.query_hashrate(1, 6000, 6250).await.unwrap();

        // Should get two 60-second buckets with adaptive bucketing
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].timestamp, 6000); // First bucket (samples at 6000, 6010)
        assert_eq!(results[1].timestamp, 6120); // Second bucket (samples at 6120, 6130)
    }
}
