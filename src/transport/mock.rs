use crate::error::{Error, Result};
use crate::transport::{Transport, TransportCapabilities};
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
/// Each [`Transport::send`] delivers one discrete frame to the peer, matching
/// no-length transport message boundaries.
#[derive(Clone)]
pub struct MockTransport {
    rx_frames: Arc<Mutex<VecDeque<Vec<u8>>>>,
    tx_peer: Arc<Mutex<VecDeque<Vec<u8>>>>,
    tx_log: Arc<Mutex<VecDeque<u8>>>,
    connected: Arc<Mutex<bool>>,
    capabilities: TransportCapabilities,
}

impl MockTransport {
    /// Create a standalone mock transport.
    pub fn new() -> Self {
        Self {
            rx_frames: Arc::new(Mutex::new(VecDeque::new())),
            tx_peer: Arc::new(Mutex::new(VecDeque::new())),
            tx_log: Arc::new(Mutex::new(VecDeque::new())),
            connected: Arc::new(Mutex::new(true)),
            capabilities: TransportCapabilities {
                length_present: true,
                fragment_max_size: 256,
            },
        }
    }

    /// Set transport capabilities for testing no-length mode or custom fragment sizes.
    pub fn with_capabilities(mut self, capabilities: TransportCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Create a pair of connected mock transports.
    ///
    /// Data sent on one will be received by the other.
    pub fn pair() -> (Self, Self) {
        let a_to_b = Arc::new(Mutex::new(VecDeque::new()));
        let b_to_a = Arc::new(Mutex::new(VecDeque::new()));
        let default_caps = TransportCapabilities {
            length_present: true,
            fragment_max_size: 256,
        };
        let a = Self {
            rx_frames: b_to_a.clone(),
            tx_peer: a_to_b.clone(),
            tx_log: Arc::new(Mutex::new(VecDeque::new())),
            connected: Arc::new(Mutex::new(true)),
            capabilities: default_caps,
        };
        let b = Self {
            rx_frames: a_to_b,
            tx_peer: b_to_a,
            tx_log: Arc::new(Mutex::new(VecDeque::new())),
            connected: Arc::new(Mutex::new(true)),
            capabilities: default_caps,
        };
        (a, b)
    }

    /// Add data to the receive buffer as one frame.
    pub fn add_rx_data(&mut self, data: &[u8]) {
        self.rx_frames.lock().unwrap().push_back(data.to_vec());
    }

    /// Get all data that was transmitted.
    pub fn get_tx_data(&self) -> Vec<u8> {
        self.tx_log.lock().unwrap().iter().copied().collect()
    }

    /// Clear transmitted data.
    pub fn clear_tx_data(&mut self) {
        self.tx_log.lock().unwrap().clear();
    }

    /// Get number of bytes available to receive.
    pub fn rx_available(&self) -> usize {
        self.rx_frames.lock().unwrap().iter().map(Vec::len).sum()
    }

    fn rx_frame_remaining(&self) -> usize {
        self.rx_frames.lock().unwrap().front().map_or(0, Vec::len)
    }

    fn try_read(&self, buf: &mut [u8]) -> Option<usize> {
        let mut frames = self.rx_frames.lock().unwrap();
        loop {
            let front = frames.front_mut()?;
            if front.is_empty() {
                frames.pop_front();
                continue;
            }
            let n = buf.len().min(front.len());
            buf[..n].copy_from_slice(&front[..n]);
            front.drain(..n);
            if front.is_empty() {
                frames.pop_front();
            }
            return Some(n);
        }
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
        self.tx_log.lock().unwrap().extend(data.iter());
        self.tx_peer.lock().unwrap().push_back(data.to_vec());
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

    async fn receive_no_length_payload(
        &mut self,
        buf: &mut [u8],
        timeout: Duration,
    ) -> Result<usize> {
        if self.rx_frame_remaining() == 0 {
            return Ok(0);
        }
        self.receive_timeout(buf, timeout).await
    }

    async fn close(&mut self) -> Result<()> {
        *self.connected.lock().unwrap() = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    fn capabilities(&self) -> TransportCapabilities {
        self.capabilities
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

    #[tokio::test]
    async fn test_no_length_empty_payload_after_header() {
        let mut transport = MockTransport::new();
        transport.add_rx_data(&[0x00]);
        let mut hdr = [0u8; 1];
        transport.receive(&mut hdr).await.unwrap();
        let mut buf = [0u8; 64];
        let n = transport
            .receive_no_length_payload(&mut buf, Duration::from_millis(50))
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
