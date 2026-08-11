use crate::error::{Error, Result};
use crate::transport::Transport;
use futures::future::{select, Either};
use futures_timer::Delay;
use serialport_stream::{new, AsyncReadExt, AsyncWriteExt, SerialPortStream};
use std::time::Duration;

/// Serial port transport implementation.
///
/// Wraps a [`SerialPortStream`] for async read/write over a serial device.
pub struct SerialTransport {
    stream: Option<SerialPortStream>,
}

impl SerialTransport {
    /// Open a serial port at the given path and baud rate.
    ///
    /// # Errors
    ///
    /// Returns an error if the port cannot be opened.
    pub fn open(path: &str, baud_rate: u32) -> Result<Self> {
        let stream = new(path, baud_rate).open().map_err(Error::Io)?;
        Ok(Self {
            stream: Some(stream),
        })
    }

    fn stream_mut(&mut self) -> Result<&mut SerialPortStream> {
        self.stream
            .as_mut()
            .ok_or_else(|| Error::InvalidState("Transport not connected".into()))
    }
}

impl Transport for SerialTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let stream = self.stream_mut()?;
        stream.write_all(data).await.map_err(Error::Io)?;
        stream.flush().await.map_err(Error::Io)?;
        Ok(())
    }

    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize> {
        let stream = self.stream_mut()?;
        let n = stream.read(buf).await.map_err(Error::Io)?;
        if n == 0 {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Connection closed",
            )));
        }
        Ok(n)
    }

    async fn receive_timeout(&mut self, buf: &mut [u8], duration: Duration) -> Result<usize> {
        let stream = self.stream_mut()?;
        let read = stream.read(buf);
        let timer = Delay::new(duration);
        futures::pin_mut!(read, timer);
        match select(read, timer).await {
            Either::Left((result, _)) => match result {
                Ok(0) => Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Connection closed",
                ))),
                Ok(n) => Ok(n),
                Err(e) => Err(Error::Io(e)),
            },
            Either::Right((_, _)) => Err(Error::Timeout),
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.stream.take();
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
}
