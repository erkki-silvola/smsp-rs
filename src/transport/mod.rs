use crate::error::Result;
use std::time::Duration;

#[cfg(feature = "serial")]
pub mod serial;

#[cfg(feature = "tcp")]
pub mod tcp;

pub mod mock;

/// Abstract async transport layer for SMSP protocol.
///
/// Implementations provide the underlying communication mechanism
/// (TCP, serial, etc.) with non-blocking async I/O.
#[allow(async_fn_in_trait)]
pub trait Transport: Send {
    /// Send data over the transport.
    async fn send(&mut self, data: &[u8]) -> Result<()>;

    /// Receive data into buffer, returns number of bytes read.
    ///
    /// Waits until at least one byte is available.
    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize>;

    /// Receive data with timeout, returns number of bytes read.
    ///
    /// # Errors
    ///
    /// Returns `Error::Timeout` if timeout expires before data arrives.
    async fn receive_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;

    /// Close the transport connection.
    async fn close(&mut self) -> Result<()>;

    /// Check if transport is connected.
    fn is_connected(&self) -> bool;
}
