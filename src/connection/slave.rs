use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::{Header, Message, Parameters};
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::{Bytes, BytesMut};

/// Slave (connection receiver) side of the SMSP protocol.
///
/// The slave waits for the master to connect, responds with parameters,
/// then receives requests and sends responses. Large messages are
/// automatically fragmented according to the configured parameters.
///
/// # Example
///
/// ```rust,no_run
/// use smsp_rs::{SlaveConnection, TcpTransport};
/// use std::net::TcpListener;
///
/// # fn main() -> smsp_rs::Result<()> {
/// let listener = TcpListener::bind("0.0.0.0:8080")?;
/// for stream in listener.incoming() {
///     let transport = TcpTransport::from_stream(stream?)?;
///     let mut slave = SlaveConnection::with_defaults(transport);
///     slave.wait_for_init()?;
///     let request = slave.receive_request()?;
///     slave.send_response(b"response")?;
///     slave.close()?;
/// }
/// # Ok(())
/// # }
/// ```
pub struct SlaveConnection<T: Transport> {
    transport: T,
    state: ConnectionState,
    parameters: Parameters,
    sequence: SequenceTracker,
}

impl<T: Transport> SlaveConnection<T> {
    /// Create a new slave connection with custom parameters.
    pub fn new(transport: T, parameters: Parameters) -> Self {
        Self {
            transport,
            state: ConnectionState::Uninitialized,
            parameters,
            sequence: SequenceTracker::new(),
        }
    }

    /// Create a new slave connection with default parameters.
    pub fn with_defaults(transport: T) -> Self {
        Self::new(transport, Parameters::default_slave())
    }

    /// Wait for the master's initialization request and respond with parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if not in `Uninitialized` state or if the master
    /// sends an invalid initialization request.
    pub fn wait_for_init(&mut self) -> Result<()> {
        if self.state != ConnectionState::Uninitialized {
            return Err(Error::InvalidState(format!(
                "Cannot initialize from state {:?}",
                self.state
            )));
        }
        self.state = ConnectionState::Negotiating;

        let mut buf = [0u8; 256];
        let n = self.transport.receive(&mut buf)?;
        let request = std::str::from_utf8(&buf[..n])
            .map_err(|_| Error::InvalidParameters("Request is not valid UTF-8".into()))?;

        if request != Parameters::INIT_REQUEST {
            return Err(Error::Protocol(format!(
                "Expected '{}', got '{}'",
                Parameters::INIT_REQUEST,
                request
            )));
        }

        self.transport
            .send(self.parameters.to_response().as_bytes())?;
        self.state = ConnectionState::Ready;
        self.sequence.reset();
        Ok(())
    }

    /// Receive a complete request from the master.
    ///
    /// Automatically reassembles fragmented messages and sends ACKs
    /// according to the configured `fragment_count`.
    ///
    /// # Errors
    ///
    /// Returns an error if not in `Ready` state, on timeout, or if
    /// the master cancels.
    pub fn receive_request(&mut self) -> Result<Bytes> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot receive request in state {:?}",
                self.state
            )));
        }
        let length_present = self.parameters.length_present;
        let fragment_count = self.parameters.fragment_count;
        let timeout = self.parameters.timeout;

        let mut request_seq = SequenceTracker::new();
        let mut request_data = BytesMut::new();
        let mut received_since_ack: u8 = 0;

        loop {
            let msg = self.read_message(length_present, timeout)?;

            if msg.is_cancel() {
                return Err(Error::Cancelled(
                    String::from_utf8_lossy(&msg.payload).into(),
                ));
            }

            if !request_seq.validate_and_advance(msg.header.seq) {
                self.send_cancel(msg.header.seq, "Sequence error")?;
                return Err(Error::Sequence {
                    expected: request_seq.current(),
                    actual: msg.header.seq,
                });
            }

            let is_last = msg.is_last();
            request_data.extend_from_slice(&msg.payload);

            if is_last {
                return Ok(request_data.freeze());
            }

            received_since_ack += 1;
            if received_since_ack >= fragment_count {
                let ack = Message::ack(0, length_present)?;
                self.transport.send(&ack.to_bytes())?;
                received_since_ack = 0;
            }
        }
    }

    /// Send a response to the master.
    ///
    /// The response data is automatically fragmented according to the
    /// configured parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if not in `Ready` state, on timeout, or if
    /// the master cancels.
    pub fn send_response(&mut self, data: &[u8]) -> Result<()> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send response in state {:?}",
                self.state
            )));
        }

        self.sequence.reset();
        let length_present = self.parameters.length_present;
        let fragment_count = self.parameters.fragment_count;
        let max_size = self.parameters.fragment_max_size as usize;
        let timeout = self.parameters.timeout;

        let chunks: Vec<&[u8]> = if length_present {
            data.chunks(max_size).collect()
        } else {
            vec![data]
        };
        let total = chunks.len();
        let mut sent_since_ack: u8 = 0;

        for (i, chunk) in chunks.iter().enumerate() {
            let is_last = i + 1 == total;
            let seq = self.sequence.next_seq();
            let msg = if is_last {
                Message::last(seq, length_present, Bytes::copy_from_slice(chunk))?
            } else {
                Message::more(seq, length_present, Bytes::copy_from_slice(chunk))?
            };
            self.transport.send(&msg.to_bytes())?;

            if is_last {
                break;
            }

            sent_since_ack += 1;
            if sent_since_ack >= fragment_count {
                self.wait_ack(length_present, timeout)?;
                sent_since_ack = 0;
            }
        }
        Ok(())
    }

    /// Send a CANCEL/error message to the master.
    ///
    /// # Errors
    ///
    /// Returns an error if not in a transferable state.
    pub fn send_error(&mut self, reason: &str) -> Result<()> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send error in state {:?}",
                self.state
            )));
        }
        self.send_cancel(self.sequence.current(), reason)
    }

    fn wait_ack(&mut self, length_present: bool, timeout: std::time::Duration) -> Result<()> {
        let msg = self.read_message(length_present, timeout)?;
        if msg.is_cancel() {
            return Err(Error::Cancelled(
                String::from_utf8_lossy(&msg.payload).into(),
            ));
        }
        if !msg.is_ack() {
            return Err(Error::Protocol(format!(
                "Expected ACK, got status {:?}",
                msg.header.status
            )));
        }
        Ok(())
    }

    fn read_message(
        &mut self,
        length_present: bool,
        timeout: std::time::Duration,
    ) -> Result<Message> {
        let mut hdr_byte = [0u8; 1];
        self.read_exact(&mut hdr_byte, timeout)?;
        let header = Header::from_bytes(&hdr_byte)?;

        if length_present {
            let mut len_buf = [0u8; 2];
            self.read_exact(&mut len_buf, timeout)?;
            let length = u16::from_be_bytes(len_buf);
            let mut payload = vec![0u8; length as usize];
            if length > 0 {
                self.read_exact(&mut payload, timeout)?;
            }
            let full_header = Header::new(header.status, true, header.seq, Some(length))?;
            Message::new(full_header, Bytes::from(payload))
        } else {
            let mut buf = [0u8; 65535];
            let n = self.transport.receive_timeout(&mut buf, timeout)?;
            let payload = Bytes::copy_from_slice(&buf[..n]);
            let full_header = Header::new(header.status, false, header.seq, None)?;
            Message::new(full_header, payload)
        }
    }

    fn read_exact(&mut self, buf: &mut [u8], timeout: std::time::Duration) -> Result<()> {
        let mut offset = 0;
        while offset < buf.len() {
            let n = self
                .transport
                .receive_timeout(&mut buf[offset..], timeout)?;
            if n == 0 {
                return Err(Error::Protocol("Unexpected EOF during read".into()));
            }
            offset += n;
        }
        Ok(())
    }

    fn send_cancel(&mut self, seq: u8, reason: &str) -> Result<()> {
        let cancel = Message::cancel(
            seq,
            self.parameters.length_present,
            Bytes::from(reason.to_string()),
        )?;
        self.transport.send(&cancel.to_bytes())?;
        Ok(())
    }

    /// Close the connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the transport cannot be closed.
    pub fn close(mut self) -> Result<()> {
        self.state = ConnectionState::Closed;
        self.transport.close()
    }

    /// Get the current connection state.
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Get the configured parameters.
    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockTransport;

    #[test]
    fn test_slave_initialization() {
        let (mut master_transport, slave_transport) = MockTransport::pair();
        master_transport
            .send(Parameters::INIT_REQUEST.as_bytes())
            .unwrap();
        let mut slave = SlaveConnection::with_defaults(slave_transport);
        slave.wait_for_init().unwrap();
        assert_eq!(slave.state(), ConnectionState::Ready);
    }

    #[test]
    fn test_slave_invalid_init() {
        let mut transport = MockTransport::new();
        transport.add_rx_data(b"invalid request");
        let mut slave = SlaveConnection::with_defaults(transport);
        assert!(matches!(slave.wait_for_init(), Err(Error::Protocol(_))));
    }
}
