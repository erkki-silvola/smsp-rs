use std::io;
use thiserror::Error;

/// Result type alias for SMSP operations
pub type Result<T> = std::result::Result<T, Error>;

/// SMSP protocol errors
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error during transport operations
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Protocol violation or invalid data
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Timeout occurred during operation
    #[error("Operation timed out")]
    Timeout,

    /// Invalid header format
    #[error("Invalid header: {0}")]
    InvalidHeader(String),

    /// Invalid parameters
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    /// Connection not established or in wrong state
    #[error("Invalid connection state: {0}")]
    InvalidState(String),

    /// Fragment error
    #[error("Fragment error: {0}")]
    Fragment(String),

    /// Sequence number mismatch
    #[error("Sequence error: expected {expected}, got {actual}")]
    Sequence { expected: u8, actual: u8 },

    /// Remote side cancelled the operation
    #[error("Operation cancelled by remote: {0}")]
    Cancelled(String),

    /// Buffer too small
    #[error("Buffer too small: need {needed}, have {available}")]
    BufferTooSmall { needed: usize, available: usize },

    /// Message too large for transport
    #[error("Message too large: {size} bytes exceeds maximum {max}")]
    MessageTooLarge { size: usize, max: usize },
}
