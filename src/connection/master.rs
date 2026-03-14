use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::{Header, Message, Parameters};
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::{Bytes, BytesMut};
use std::time::Duration;

/// Master (connection initiator) side of the SMSP protocol.
///
/// The master initiates the connection, negotiates parameters with the
/// slave, then performs request/response cycles. Large messages are
/// automatically fragmented according to the negotiated parameters.
///
/// # Example
///
/// ```rust,no_run
/// use smsp_rs::{MasterConnection, TcpTransport};
///
/// # fn main() -> smsp_rs::Result<()> {
/// let transport = TcpTransport::connect("192.168.1.100:8080")?;
/// let mut master = MasterConnection::new(transport);
/// let params = master.initialize()?;
/// let response = master.request(b"Hello, Device!")?;
/// master.close()?;
/// # Ok(())
/// # }
/// ```
pub struct MasterConnection<T: Transport> {
    transport: T,
    state: ConnectionState,
    parameters: Option<Parameters>,
    sequence: SequenceTracker,
}

impl<T: Transport> MasterConnection<T> {
    /// Create a new master connection wrapping the given transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            state: ConnectionState::Uninitialized,
            parameters: None,
            sequence: SequenceTracker::new(),
        }
    }

    /// Initialize the connection by negotiating parameters with the slave.
    ///
    /// Sends `"parameters?\0"` and expects a parameter response within 5 seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if not in `Uninitialized` state, on timeout,
    /// or if the slave's parameters are invalid.
    pub fn initialize(&mut self) -> Result<Parameters> {
        if self.state != ConnectionState::Uninitialized {
            return Err(Error::InvalidState(format!(
                "Cannot initialize from state {:?}",
                self.state
            )));
        }
        self.state = ConnectionState::Negotiating;
        self.transport.send(Parameters::INIT_REQUEST.as_bytes())?;

        let mut buf = [0u8; 256];
        let n = self
            .transport
            .receive_timeout(&mut buf, Duration::from_secs(5))?;

        let response = std::str::from_utf8(&buf[..n])
            .map_err(|_| Error::InvalidParameters("Response is not valid UTF-8".into()))?;

        let parameters = Parameters::from_response(response)?;
        if parameters.protocol_version != 1 {
            return Err(Error::InvalidParameters(format!(
                "Unsupported protocol version: {}",
                parameters.protocol_version
            )));
        }

        self.parameters = Some(parameters.clone());
        self.state = ConnectionState::Ready;
        self.sequence.reset();
        Ok(parameters)
    }

    /// Send a request and receive the response.
    ///
    /// The request data is automatically fragmented according to negotiated
    /// parameters. Returns the complete reassembled response.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection is not in `Ready` state, on
    /// timeout, or if the remote side cancels.
    pub fn request(&mut self, data: &[u8]) -> Result<Bytes> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send request in state {:?}",
                self.state
            )));
        }
        let params = self.params()?;
        let length_present = params.length_present;
        let fragment_count = params.fragment_count;
        let max_size = params.fragment_max_size as usize;

        self.sequence.reset();
        self.send_fragments(data, length_present, fragment_count, max_size)?;
        self.receive_fragments(length_present, fragment_count)
    }

    fn send_fragments(
        &mut self,
        data: &[u8],
        length_present: bool,
        fragment_count: u8,
        max_size: usize,
    ) -> Result<()> {
        let timeout = self.params()?.timeout;
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

    fn receive_fragments(&mut self, length_present: bool, fragment_count: u8) -> Result<Bytes> {
        let timeout = self.params()?.timeout;
        let mut response_seq = SequenceTracker::new();
        let mut response = BytesMut::new();
        let mut received_since_ack: u8 = 0;

        loop {
            let msg = self.read_message(length_present, timeout)?;

            if msg.is_cancel() {
                return Err(Error::Cancelled(
                    String::from_utf8_lossy(&msg.payload).into(),
                ));
            }

            if !response_seq.validate_and_advance(msg.header.seq) {
                return Err(Error::Sequence {
                    expected: response_seq.current(),
                    actual: msg.header.seq,
                });
            }

            let is_last = msg.is_last();
            response.extend_from_slice(&msg.payload);

            if is_last {
                return Ok(response.freeze());
            }

            received_since_ack += 1;
            if received_since_ack >= fragment_count {
                let ack = Message::ack(0, length_present)?;
                self.transport.send(&ack.to_bytes())?;
                received_since_ack = 0;
            }
        }
    }

    fn wait_ack(&mut self, length_present: bool, timeout: Duration) -> Result<()> {
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

    fn read_message(&mut self, length_present: bool, timeout: Duration) -> Result<Message> {
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

    fn read_exact(&mut self, buf: &mut [u8], timeout: Duration) -> Result<()> {
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

    /// Get the negotiated parameters, if available.
    pub fn parameters(&self) -> Option<&Parameters> {
        self.parameters.as_ref()
    }

    fn params(&self) -> Result<Parameters> {
        self.parameters
            .clone()
            .ok_or_else(|| Error::InvalidState("Parameters not initialized".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockTransport;

    #[test]
    fn test_master_initialization() {
        let mut transport = MockTransport::new();
        let params = Parameters::default_slave();
        transport.add_rx_data(params.to_response().as_bytes());
        let mut master = MasterConnection::new(transport);
        let negotiated = master.initialize().unwrap();
        assert_eq!(negotiated, params);
        assert_eq!(master.state(), ConnectionState::Ready);
    }

    #[test]
    fn test_master_invalid_state() {
        let transport = MockTransport::new();
        let mut master = MasterConnection::new(transport);
        assert!(matches!(
            master.request(b"test"),
            Err(Error::InvalidState(_))
        ));
    }
}
