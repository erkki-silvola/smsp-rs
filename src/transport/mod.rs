use crate::error::Result;
use std::time::Duration;

#[cfg(feature = "tcp")]
pub mod tcp;

pub mod mock;

/// Abstract transport layer for SMSP protocol
///
/// Implementations provide the underlying communication mechanism
/// (TCP, serial, etc.) with blocking I/O operations.
pub trait Transport: Send {
    /// Send data over the transport
    fn send(&mut self, data: &[u8]) -> Result<()>;

    /// Receive data into buffer, returns number of bytes read
    /// Blocks until at least one byte is available
    fn receive(&mut self, buf: &mut [u8]) -> Result<usize>;

    /// Receive data with timeout, returns number of bytes read
    /// Returns Err(Error::Timeout) if timeout expires
    fn receive_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;

    /// Close the transport connection
    fn close(&mut self) -> Result<()>;

    /// Check if transport is connected
    fn is_connected(&self) -> bool;
}
