use redis::{Client, aio::ConnectionManager};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

use crate::error::RedisError;

pub struct RedisClient {
    manager: ConnectionManager,
}

impl RedisClient {
    pub async fn new(redis_url: &str) -> Result<Self, RedisError> {
        let client = Client::open(redis_url)
            .map_err(|e| RedisError::ConnectionError(e.to_string()))?;
        
        let manager = ConnectionManager::new(client).await
            .map_err(|e| RedisError::ConnectionError(e.to_string()))?;

        Ok(Self { manager })
    }

    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        expiry: Option<Duration>,
    ) -> Result<(), RedisError> {
        let serialized = serde_json::to_string(value)?;
        
        let mut cmd = redis::cmd("SET");
        cmd.arg(key).arg(&serialized);
        
        if let Some(expiry) = expiry {
            cmd.arg("EX").arg(expiry.as_secs());
        }

        cmd.query_async::<_, ()>(&mut self.manager.clone())
            .await
            .map_err(RedisError::from)
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, RedisError> {
        let value: Option<String> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut self.manager.clone())
            .await
            .map_err(RedisError::from)?;

        match value {
            Some(v) => {
                let deserialized = serde_json::from_str(&v)?;
                Ok(Some(deserialized))
            }
            None => Ok(None),
        }
    }

    pub async fn delete(&self, key: &str) -> Result<(), RedisError> {
        redis::cmd("DEL")
            .arg(key)
            .query_async::<_, ()>(&mut self.manager.clone())
            .await
            .map_err(RedisError::from)
    }

    pub async fn exists(&self, key: &str) -> Result<bool, RedisError> {
        let exists: bool = redis::cmd("EXISTS")
            .arg(key)
            .query_async(&mut self.manager.clone())
            .await
            .map_err(RedisError::from)?;

        Ok(exists)
    }

    pub async fn set_if_not_exists<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        expiry: Option<Duration>,
    ) -> Result<bool, RedisError> {
        let serialized = serde_json::to_string(value)?;
        
        let mut cmd = redis::cmd("SETNX");
        cmd.arg(key).arg(&serialized);
        
        let success: bool = cmd.query_async(&mut self.manager.clone())
            .await
            .map_err(RedisError::from)?;

        if success && expiry.is_some() {
            redis::cmd("EXPIRE")
                .arg(key)
                .arg(expiry.unwrap().as_secs())
                .query_async::<_, ()>(&mut self.manager.clone())
                .await
                .map_err(RedisError::from)?;
        }

        Ok(success)
    }
} 