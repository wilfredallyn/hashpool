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
            .map(|row| {
                let bucket_timestamp = row.get::<i64, _>("bucket_timestamp") as u64;
                let total_difficulty = row.get::<f64, _>("total_difficulty");
                let sample_count = row.get::<i64, _>("sample_count") as u64;
                let window_seconds = row.get::<i64, _>("window_seconds") as u64;

                // Calculate average hashrate across samples
                // Average the difficulty first, then derive hashrate
                let avg_difficulty = total_difficulty / sample_count as f64;

                HashratePoint {
                    timestamp: bucket_timestamp,
                    hashrate_hs: crate::metrics::derive_hashrate(avg_difficulty, window_seconds),
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
        let rows = sqlx::query(
            r#"
            SELECT
                (timestamp / ?) * ? as bucket_timestamp,
                SUM(CAST(sum_difficulty AS REAL)) as total_difficulty,
                COUNT(*) as sample_count,
                MAX(window_seconds) as window_seconds
            FROM hashrate_samples
            WHERE downstream_id = ? AND timestamp >= ? AND timestamp <= ?
            GROUP BY bucket_timestamp
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

        // Aggregate data into calculated buckets to smooth the graph
        let rows = sqlx::query(
            r#"
            SELECT
                (timestamp / ?) * ? as bucket_timestamp,
                SUM(CAST(sum_difficulty AS REAL)) as total_difficulty,
                COUNT(*) as sample_count,
                MAX(window_seconds) as window_seconds
            FROM hashrate_samples
            WHERE timestamp >= ? AND timestamp <= ?
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

        Ok(Self::aggregate_rows_to_hashrate_points(rows))
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

        // Query samples - should be aggregated into 60-second bucket (as before)
        let results = storage.query_hashrate(1, 6000, 6060).await.unwrap();
        // All 6 samples fall into the same bucket
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp, 6000);
        // Average of 6 samples of 1000 difficulty each = 1000
        // (1000 * 2^32) / 10 seconds = 429,496,729,600 H/s
        assert_eq!(results[0].hashrate_hs, 429_496_729_600.0);
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
        // 2 samples with difficulty 1000 each in the same bucket
        // SUM = 2000, COUNT = 2, avg = 1000
        // (1000 * 2^32) / 10 seconds = 429,496,729,600 H/s
        assert_eq!(results[0].hashrate_hs, 429_496_729_600.0);
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
