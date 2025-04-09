use thiserror::Error;

#[derive(Error, Debug)]
pub enum RedisError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid data format: {0}")]
    InvalidDataFormat(String),
} 