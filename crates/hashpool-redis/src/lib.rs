pub mod client;
pub mod error;

pub use client::RedisClient;
pub use error::RedisError;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        field1: String,
        field2: i32,
    }

    #[tokio::test]
    async fn test_redis_operations() {
        let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
        
        // Try to connect to Redis, skip test if not available
        let client = match RedisClient::new(&redis_url).await {
            Ok(client) => client,
            Err(e) => {
                println!("Skipping Redis tests: {}", e);
                return;
            }
        };

        // Test data
        let test_data = TestData {
            field1: "test".to_string(),
            field2: 42,
        };

        // Test set and get
        client.set("test_key", &test_data, Some(Duration::from_secs(60))).await.unwrap();
        let retrieved: Option<TestData> = client.get("test_key").await.unwrap();
        assert_eq!(retrieved, Some(test_data));

        // Test exists
        assert!(client.exists("test_key").await.unwrap());
        assert!(!client.exists("nonexistent_key").await.unwrap());

        // Test delete
        client.delete("test_key").await.unwrap();
        assert!(!client.exists("test_key").await.unwrap());
    }
} 