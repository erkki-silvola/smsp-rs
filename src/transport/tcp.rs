use crate::error::{Error, Result};
use crate::transport::{Transport, TransportCapabilities};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// TCP transport implementation.
///
/// Wraps a `TcpStream` with Nagle's algorithm disabled for low-latency
/// IoT communication.
pub struct TcpTransport {
    stream: Option<TcpStream>,
}

impl TcpTransport {
    /// Connect to a TCP address and create a transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(addr: impl tokio::net::ToSocketAddrs) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream: Some(stream),
        })
    }

    /// Create a TCP transport from an existing connected `TcpStream`.
    ///
    /// # Errors
    ///
    /// Returns an error if `set_nodelay` fails.
    pub async fn from_stream(stream: TcpStream) -> Result<Self> {
        stream.set_nodelay(true)?;
        Ok(Self {
            stream: Some(stream),
        })
    }

    fn stream_mut(&mut self) -> Result<&mut TcpStream> {
        self.stream
            .as_mut()
            .ok_or_else(|| Error::InvalidState("Transport not connected".into()))
    }
}

impl Transport for TcpTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let stream = self.stream_mut()?;
        stream.write_all(data).await?;
        stream.flush().await?;
        Ok(())
    }

    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize> {
        let stream = self.stream_mut()?;
        let n = stream.read(buf).await?;
        if n == 0 {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Connection closed",
            )));
        }
        Ok(n)
    }

    async fn receive_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let stream = self.stream_mut()?;
        match tokio::time::timeout(timeout, stream.read(buf)).await {
            Ok(Ok(0)) => Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Connection closed",
            ))),
            Ok(Ok(n)) => Ok(n),
            Ok(Err(e)) => Err(Error::Io(e)),
            Err(_) => Err(Error::Timeout),
        }
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.shutdown().await;
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            length_present: true,
            fragment_max_size: 0,
        }
    }
}

impl Drop for TcpTransport {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream
                .into_std()
                .map(|s| s.shutdown(std::net::Shutdown::Both));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_tcp_send_receive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        stream.write_all(&buf[..n]).await.unwrap();
                    }
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut transport = TcpTransport::connect(addr).await.unwrap();
        assert!(transport.is_connected());
        let caps = transport.capabilities();
        assert!(caps.length_present);
        assert_eq!(caps.fragment_max_size, 0);

        let data = b"Hello, SMSP!";
        transport.send(data).await.unwrap();

        let mut buf = [0u8; 1024];
        let n = transport.receive(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], data);

        transport.close().await.unwrap();
        assert!(!transport.is_connected());
    }
}
