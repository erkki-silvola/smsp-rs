use crate::error::{Error, Result};
use crate::transport::Transport;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// TCP transport implementation.
///
/// Wraps a `TcpStream` with Nagle's algorithm disabled for low-latency
/// IoT communication.
pub struct TcpTransport {
    stream: Option<TcpStream>,
}

impl TcpTransport {
    /// Create a new TCP transport by connecting to an address.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream: Some(stream),
        })
    }

    /// Create a TCP transport from an existing `TcpStream`.
    ///
    /// # Errors
    ///
    /// Returns an error if `set_nodelay` fails.
    pub fn from_stream(stream: TcpStream) -> Result<Self> {
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
    fn send(&mut self, data: &[u8]) -> Result<()> {
        let stream = self.stream_mut()?;
        stream.write_all(data)?;
        stream.flush()?;
        Ok(())
    }

    fn receive(&mut self, buf: &mut [u8]) -> Result<usize> {
        let stream = self.stream_mut()?;
        let n = stream.read(buf)?;
        if n == 0 {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Connection closed",
            )));
        }
        Ok(n)
    }

    fn receive_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let stream = self.stream_mut()?;
        stream.set_read_timeout(Some(timeout))?;
        let result = stream.read(buf);
        stream.set_read_timeout(None)?;
        match result {
            Ok(0) => Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Connection closed",
            ))),
            Ok(n) => Ok(n),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Err(Error::Timeout)
            }
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn close(&mut self) -> Result<()> {
        if let Some(stream) = self.stream.take() {
            stream.shutdown(std::net::Shutdown::Both)?;
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
}

impl Drop for TcpTransport {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn test_tcp_send_receive() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        stream.write_all(&buf[..n]).unwrap();
                    }
                }
            }
        });

        thread::sleep(Duration::from_millis(50));

        let mut transport = TcpTransport::connect(addr).unwrap();
        assert!(transport.is_connected());

        let data = b"Hello, SMSP!";
        transport.send(data).unwrap();

        let mut buf = [0u8; 1024];
        let n = transport.receive(&mut buf).unwrap();
        assert_eq!(&buf[..n], data);

        transport.close().unwrap();
        assert!(!transport.is_connected());
    }
}
