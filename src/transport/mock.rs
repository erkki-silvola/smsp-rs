use crate::error::{Error, Result};
use crate::transport::Transport;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

/// Mock transport for testing.
///
/// Supports both standalone mode (`new()`) and paired bidirectional
/// mode (`pair()`) where data sent on one end is received by the other.
#[derive(Clone)]
pub struct MockTransport {
    rx_buffer: Arc<Mutex<VecDeque<u8>>>,
    tx_buffer: Arc<Mutex<VecDeque<u8>>>,
    connected: Arc<Mutex<bool>>,
}

impl MockTransport {
    /// Create a standalone mock transport.
    pub fn new() -> Self {
        Self {
            rx_buffer: Arc::new(Mutex::new(VecDeque::new())),
            tx_buffer: Arc::new(Mutex::new(VecDeque::new())),
            connected: Arc::new(Mutex::new(true)),
        }
    }

    /// Create a pair of connected mock transports.
    ///
    /// Data sent on one will be received by the other.
    pub fn pair() -> (Self, Self) {
        let a_to_b = Arc::new(Mutex::new(VecDeque::new()));
        let b_to_a = Arc::new(Mutex::new(VecDeque::new()));
        let a = Self {
            rx_buffer: b_to_a.clone(),
            tx_buffer: a_to_b.clone(),
            connected: Arc::new(Mutex::new(true)),
        };
        let b = Self {
            rx_buffer: a_to_b,
            tx_buffer: b_to_a,
            connected: Arc::new(Mutex::new(true)),
        };
        (a, b)
    }

    /// Add data to the receive buffer.
    pub fn add_rx_data(&mut self, data: &[u8]) {
        self.rx_buffer.lock().unwrap().extend(data.iter());
    }

    /// Get all data that was transmitted.
    pub fn get_tx_data(&self) -> Vec<u8> {
        self.tx_buffer.lock().unwrap().iter().copied().collect()
    }

    /// Clear transmitted data.
    pub fn clear_tx_data(&mut self) {
        self.tx_buffer.lock().unwrap().clear();
    }

    /// Get number of bytes available to receive.
    pub fn rx_available(&self) -> usize {
        self.rx_buffer.lock().unwrap().len()
    }

    fn try_read(&self, buf: &mut [u8]) -> Option<usize> {
        let mut rx = self.rx_buffer.lock().unwrap();
        if rx.is_empty() {
            return None;
        }
        let n = buf.len().min(rx.len());
        for item in buf.iter_mut().take(n) {
            *item = rx.pop_front().unwrap();
        }
        Some(n)
    }
}

struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 {
            self.0 = false;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

async fn yield_now() {
    YieldNow(true).await;
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for MockTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        if !self.is_connected() {
            return Err(Error::InvalidState("Transport not connected".into()));
        }
        self.tx_buffer.lock().unwrap().extend(data.iter());
        Ok(())
    }

    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.receive_timeout(buf, Duration::from_secs(5)).await
    }

    async fn receive_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let deadline = Instant::now() + timeout;
        loop {
            if !self.is_connected() {
                return Err(Error::InvalidState("Transport not connected".into()));
            }
            if let Some(n) = self.try_read(buf) {
                return Ok(n);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            yield_now().await;
        }
    }

    async fn close(&mut self) -> Result<()> {
        *self.connected.lock().unwrap() = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_transport() {
        let mut transport = MockTransport::new();
        transport.add_rx_data(b"hello");
        let mut buf = [0u8; 10];
        let n = transport.receive(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello");
        transport.send(b"world").await.unwrap();
        assert_eq!(transport.get_tx_data(), b"world");
    }

    #[tokio::test]
    async fn test_mock_pair() {
        let (mut a, mut b) = MockTransport::pair();
        a.send(b"from A").await.unwrap();
        let mut buf = [0u8; 10];
        let n = b.receive(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"from A");
        b.send(b"from B").await.unwrap();
        let n = a.receive(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"from B");
    }

    #[tokio::test]
    async fn test_timeout_on_empty() {
        let mut transport = MockTransport::new();
        let mut buf = [0u8; 10];
        assert!(matches!(
            transport
                .receive_timeout(&mut buf, Duration::from_millis(50))
                .await,
            Err(Error::Timeout)
        ));
    }
}
