use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::{Message, Parameters, Status};
use crate::session::{FlowControl, FragmentManager, SequenceTracker};
use crate::transport::Transport;
use bytes::Bytes;

/// Slave/Device connection - responds to requests
pub struct SlaveConnection<T: Transport> {
    transport: T,
    state: ConnectionState,
    parameters: Parameters,
    sequence: SequenceTracker,
    fragment_manager: FragmentManager,
    flow_control: FlowControl,
}

impl<T: Transport> SlaveConnection<T> {
    /// Create a new slave connection with a transport
    pub fn new(transport: T, parameters: Parameters) -> Self {
        let fragment_manager = FragmentManager::new(parameters.fragment_max_size as usize);
        let flow_control = FlowControl::new(parameters.fragment_count, parameters.timeout);

        SlaveConnection {
            transport,
            state: ConnectionState::Uninitialized,
            parameters,
            sequence: SequenceTracker::new(),
            fragment_manager,
            flow_control,
        }
    }

    /// Create a slave with default parameters
    pub fn with_defaults(transport: T) -> Self {
        Self::new(transport, Parameters::default_slave())
    }

    /// Wait for initialization request and respond with parameters
    pub fn wait_for_init(&mut self) -> Result<()> {
        if self.state != ConnectionState::Uninitialized {
            return Err(Error::InvalidState(format!(
                "Cannot initialize from state {:?}",
                self.state
            )));
        }

        self.state = ConnectionState::Negotiating;

        // Wait for parameter request
        let mut buf = [0u8; 256];
        let n = self.transport.receive(&mut buf)?;

        let request = std::str::from_utf8(&buf[..n])
            .map_err(|_| Error::InvalidParameters("Request is not valid UTF-8".to_string()))?;

        if request != Parameters::INIT_REQUEST {
            return Err(Error::Protocol(format!(
                "Expected '{}', got '{}'",
                Parameters::INIT_REQUEST,
                request
            )));
        }

        // Send parameters response
        let response = self.parameters.to_response();
        self.transport.send(response.as_bytes())?;

        self.state = ConnectionState::Ready;
        self.sequence.reset();

        Ok(())
    }

    /// Receive a request from the master
    pub fn receive_request(&mut self) -> Result<Bytes> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot receive request in state {:?}",
                self.state
            )));
        }

        // Clear previous assembly
        self.fragment_manager.clear();

        // Reset sequence for new request
        let mut request_seq = SequenceTracker::new();

        loop {
            // Receive message
            let mut buf = [0u8; 4096];
            let n = self
                .transport
                .receive_timeout(&mut buf, self.parameters.timeout)?;

            let (msg, _) = Message::from_bytes(&buf[..n])?;

            // Check for cancel
            if msg.is_cancel() {
                let reason = String::from_utf8_lossy(&msg.payload);
                self.fragment_manager.clear();
                return Err(Error::Cancelled(reason.to_string()));
            }

            // Validate sequence
            if !request_seq.validate_and_advance(msg.header.seq) {
                // Send cancel and return error
                self.flow_control.send_cancel(
                    &mut self.transport,
                    msg.header.seq,
                    self.parameters.length_present,
                    "Sequence error",
                )?;
                self.fragment_manager.clear();
                return Err(Error::Sequence {
                    expected: request_seq.current(),
                    actual: msg.header.seq,
                });
            }

            // Check if this was the last fragment before moving payload
            let is_last = msg.is_last();

            // Add fragment
            self.fragment_manager.add_fragment(msg.payload)?;

            // Send ACK if we've received fragment_count fragments or if it's the last
            if is_last
                || self.flow_control.pending_count() + 1 >= self.parameters.fragment_count
            {
                self.flow_control.send_ack(
                    &mut self.transport,
                    msg.header.seq,
                    self.parameters.length_present,
                )?;
                self.flow_control.reset_pending();
            } else {
                self.flow_control.increment_pending();
            }

            // Check if this was the last fragment
            if is_last {
                break;
            }
        }

        Ok(self.fragment_manager.finalize())
    }

    /// Send a response to the master
    pub fn send_response(&mut self, data: &[u8]) -> Result<()> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send response in state {:?}",
                self.state
            )));
        }

        // Split data into fragments
        let fragments = self.fragment_manager.split(data);

        // Reset sequence for response
        self.sequence.reset();

        // Send all fragments
        for (i, fragment) in fragments.iter().enumerate() {
            let is_last = i == fragments.len() - 1;
            let status = if is_last { Status::Last } else { Status::More };

            let seq = self.sequence.next_seq();
            let msg = Message::new(
                crate::protocol::Header::new(
                    status,
                    self.parameters.length_present,
                    seq,
                    if self.parameters.length_present {
                        Some(fragment.len() as u16)
                    } else {
                        None
                    },
                )?,
                fragment.clone(),
            )?;

            // Send fragment
            self.transport.send(&msg.to_bytes())?;

            // Wait for ACK
            self.flow_control
                .wait_for_ack(&mut self.transport, self.parameters.length_present)?;
        }

        Ok(())
    }

    /// Send an error response
    pub fn send_error(&mut self, reason: &str) -> Result<()> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send error in state {:?}",
                self.state
            )));
        }

        self.flow_control.send_cancel(
            &mut self.transport,
            self.sequence.current(),
            self.parameters.length_present,
            reason,
        )
    }

    /// Close the connection
    pub fn close(mut self) -> Result<()> {
        self.state = ConnectionState::Closed;
        self.transport.close()
    }

    /// Get current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Get parameters
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

        // Send init request
        master_transport.send(Parameters::INIT_REQUEST.as_bytes()).unwrap();

        let mut slave = SlaveConnection::with_defaults(slave_transport);
        slave.wait_for_init().unwrap();

        assert_eq!(slave.state(), ConnectionState::Ready);
    }

    #[test]
    fn test_slave_invalid_init() {
        let mut transport = MockTransport::new();
        transport.add_rx_data(b"invalid request");

        let mut slave = SlaveConnection::with_defaults(transport);
        let result = slave.wait_for_init();

        assert!(matches!(result, Err(Error::Protocol(_))));
    }
}
