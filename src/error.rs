use std::io;
use thiserror::Error;

/// Result type alias for SMSP operations.
pub type Result<T> = std::result::Result<T, Error>;

/// SMSP protocol errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("Operation timed out")]
    Timeout,
    #[error("Invalid header: {0}")]
    InvalidHeader(String),
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),
    #[error("Invalid connection state: {0}")]
    InvalidState(String),
    #[error("Sequence error: expected {expected}, got {actual}")]
    Sequence { expected: u8, actual: u8 },
    #[error("Operation cancelled by remote: {0}")]
    Cancelled(String),
    #[error("Buffer too small: need {needed}, have {available}")]
    BufferTooSmall { needed: usize, available: usize },
    #[error("Message too large: {size} bytes exceeds maximum {max}")]
    MessageTooLarge { size: usize, max: usize },
}
