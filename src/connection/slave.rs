use crate::connection::io;
use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::{Message, Parameters};
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::Bytes;

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
        self.state.require_initializing()?;
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
        self.state.require_can_transfer()?;
        let result = io::receive_fragments(
            &mut self.transport,
            self.parameters.length_present,
            self.parameters.fragment_count,
            self.parameters.timeout,
        );
        if let Err(Error::Sequence { actual, .. }) = result {
            let _ = self.send_cancel(actual, "Sequence error");
        }
        result
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
        self.state.require_can_transfer()?;
        self.sequence.reset();
        io::send_fragments(
            &mut self.transport,
            data,
            self.parameters.length_present,
            self.parameters.fragment_count,
            self.parameters.fragment_max_size as usize,
            self.parameters.timeout,
            &mut self.sequence,
        )
    }

    /// Send a CANCEL/error message to the master.
    ///
    /// # Errors
    ///
    /// Returns an error if not in a transferable state.
    pub fn send_error(&mut self, reason: &str) -> Result<()> {
        self.state.require_can_transfer()?;
        self.send_cancel(self.sequence.current(), reason)
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
