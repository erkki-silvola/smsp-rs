use crate::error::Result;
use std::time::Duration;

#[cfg(feature = "serial")]
pub mod serial;

#[cfg(feature = "tcp")]
pub mod tcp;

pub mod mock;

/// Transport capability metadata used during parameter negotiation and fragmentation.
///
/// When `length_present` is `true`, the transport requires the length field in SMSP
/// headers (TCP, serial). When `false`, the transport operates in no-length mode
/// (e.g. future BLE) and `fragment_max_size` defines the maximum payload per fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportCapabilities {
    /// `true` if this transport requires the length field in message headers.
    pub length_present: bool,
    /// Maximum payload bytes per fragment when `length_present` is `false`.
    pub fragment_max_size: u16,
}

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

    /// Receive payload for a no-length message after the header byte has been read.
    ///
    /// Default implementation reads up to `buf.len()` bytes via [`Self::receive_timeout`].
    /// Transports with discrete frames (e.g. [`mock::MockTransport`]) may override this
    /// to return zero bytes when the current frame is exhausted.
    async fn receive_no_length_payload(
        &mut self,
        buf: &mut [u8],
        timeout: Duration,
    ) -> Result<usize> {
        self.receive_timeout(buf, timeout).await
    }

    /// Return transport capabilities for negotiation and fragmentation.
    fn capabilities(&self) -> TransportCapabilities;
}
