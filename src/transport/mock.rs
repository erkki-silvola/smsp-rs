use crate::error::{Error, Result};
use crate::transport::Transport;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Mock transport for testing
///
/// Allows simulating bidirectional communication without actual I/O
#[derive(Clone)]
pub struct MockTransport {
    /// Data to be read
    rx_buffer: Arc<Mutex<VecDeque<u8>>>,
    /// Data that was written (shared with peer's rx_buffer for pair())
    tx_buffer: Arc<Mutex<VecDeque<u8>>>,
    /// Whether the transport is connected
    connected: Arc<Mutex<bool>>,
}

impl MockTransport {
    /// Create a new mock transport
    pub fn new() -> Self {
        MockTransport {
            rx_buffer: Arc::new(Mutex::new(VecDeque::new())),
            tx_buffer: Arc::new(Mutex::new(VecDeque::new())),
            connected: Arc::new(Mutex::new(true)),
        }
    }

    /// Create a pair of connected mock transports
    /// Data sent on one will be received by the other
    pub fn pair() -> (Self, Self) {
        let a_to_b = Arc::new(Mutex::new(VecDeque::new()));
        let b_to_a = Arc::new(Mutex::new(VecDeque::new()));
        let connected = Arc::new(Mutex::new(true));

        // Wire up so data sent on A goes to B's rx and vice versa
        let a = MockTransport {
            rx_buffer: b_to_a.clone(),
            tx_buffer: a_to_b.clone(),
            connected: connected.clone(),
        };

        let b = MockTransport {
            rx_buffer: a_to_b,
            tx_buffer: b_to_a,
            connected,
        };

        (a, b)
    }

    /// Add data to the receive buffer
    pub fn add_rx_data(&mut self, data: &[u8]) {
        let mut rx = self.rx_buffer.lock().unwrap();
        rx.extend(data.iter());
    }

    /// Get all data that was transmitted
    pub fn get_tx_data(&self) -> Vec<u8> {
        let tx = self.tx_buffer.lock().unwrap();
        tx.iter().copied().collect()
    }

    /// Clear transmitted data
    pub fn clear_tx_data(&mut self) {
        let mut tx = self.tx_buffer.lock().unwrap();
        tx.clear();
    }

    /// Get number of bytes available to receive
    pub fn rx_available(&self) -> usize {
        let rx = self.rx_buffer.lock().unwrap();
        rx.len()
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for MockTransport {
    fn send(&mut self, data: &[u8]) -> Result<()> {
        if !self.is_connected() {
            return Err(Error::InvalidState("Transport not connected".to_string()));
        }

        let mut tx = self.tx_buffer.lock().unwrap();
        tx.extend(data.iter());
        Ok(())
    }

    fn receive(&mut self, buf: &mut [u8]) -> Result<usize> {
        if !self.is_connected() {
            return Err(Error::InvalidState("Transport not connected".to_string()));
        }

        let mut rx = self.rx_buffer.lock().unwrap();

        if rx.is_empty() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "No data available",
            )));
        }

        let n = buf.len().min(rx.len());
        for item in buf.iter_mut().take(n) {
            *item = rx.pop_front().unwrap();
        }

        Ok(n)
    }

    fn receive_timeout(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize> {
        if !self.is_connected() {
            return Err(Error::InvalidState("Transport not connected".to_string()));
        }

        let mut rx = self.rx_buffer.lock().unwrap();

        if rx.is_empty() {
            return Err(Error::Timeout);
        }

        let n = buf.len().min(rx.len());
        for item in buf.iter_mut().take(n) {
            *item = rx.pop_front().unwrap();
        }

        Ok(n)
    }

    fn close(&mut self) -> Result<()> {
        let mut connected = self.connected.lock().unwrap();
        *connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        let connected = self.connected.lock().unwrap();
        *connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_transport() {
        let mut transport = MockTransport::new();
        transport.add_rx_data(b"hello");

        let mut buf = [0u8; 10];
        let n = transport.receive(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello");

        transport.send(b"world").unwrap();
        assert_eq!(transport.get_tx_data(), b"world");
    }

    #[test]
    fn test_mock_pair() {
        let (mut a, mut b) = MockTransport::pair();

        a.send(b"from A").unwrap();
        let mut buf = [0u8; 10];
        let n = b.receive(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"from A");

        b.send(b"from B").unwrap();
        let n = a.receive(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"from B");
    }

    #[test]
    fn test_timeout() {
        let mut transport = MockTransport::new();
        let mut buf = [0u8; 10];
        let result = transport.receive_timeout(&mut buf, Duration::from_millis(100));
        assert!(matches!(result, Err(Error::Timeout)));
    }
}
