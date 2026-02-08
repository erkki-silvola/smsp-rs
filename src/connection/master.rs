use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::{Message, Parameters, Status};
use crate::session::{FlowControl, FragmentManager, SequenceTracker};
use crate::transport::Transport;
use bytes::Bytes;
use std::time::Duration;

/// Master/Host connection - initiates requests
pub struct MasterConnection<T: Transport> {
    transport: T,
    state: ConnectionState,
    parameters: Option<Parameters>,
    sequence: SequenceTracker,
    fragment_manager: Option<FragmentManager>,
    flow_control: Option<FlowControl>,
}

impl<T: Transport> MasterConnection<T> {
    /// Create a new master connection with a transport
    pub fn new(transport: T) -> Self {
        MasterConnection {
            transport,
            state: ConnectionState::Uninitialized,
            parameters: None,
            sequence: SequenceTracker::new(),
            fragment_manager: None,
            flow_control: None,
        }
    }

    /// Initialize the connection by negotiating parameters
    /// Returns the negotiated parameters
    pub fn initialize(&mut self) -> Result<Parameters> {
        if self.state != ConnectionState::Uninitialized {
            return Err(Error::InvalidState(format!(
                "Cannot initialize from state {:?}",
                self.state
            )));
        }

        self.state = ConnectionState::Negotiating;

        // Send parameter request
        self.transport.send(Parameters::INIT_REQUEST.as_bytes())?;

        // Wait for response with timeout (5 seconds as per spec)
        let mut buf = [0u8; 256];
        let n = self
            .transport
            .receive_timeout(&mut buf, Duration::from_secs(5))?;

        // Parse parameters
        let response = std::str::from_utf8(&buf[..n]).map_err(|_| {
            Error::InvalidParameters("Response is not valid UTF-8".to_string())
        })?;

        let parameters = Parameters::from_response(response)?;

        // Initialize fragment manager and flow control
        self.fragment_manager = Some(FragmentManager::new(parameters.fragment_max_size as usize));
        self.flow_control = Some(FlowControl::new(
            parameters.fragment_count,
            parameters.timeout,
        ));

        self.parameters = Some(parameters.clone());
        self.state = ConnectionState::Ready;
        self.sequence.reset();

        Ok(parameters)
    }

    /// Send a request and receive response
    pub fn request(&mut self, data: &[u8]) -> Result<Bytes> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send request in state {:?}",
                self.state
            )));
        }

        let params = self
            .parameters
            .as_ref()
            .ok_or_else(|| Error::InvalidState("Parameters not initialized".to_string()))?;

        let fragment_manager = self.fragment_manager.as_ref().ok_or_else(|| {
            Error::InvalidState("Fragment manager not initialized".to_string())
        })?;

        let flow_control = self.flow_control.as_mut().ok_or_else(|| {
            Error::InvalidState("Flow control not initialized".to_string())
        })?;

        // Split data into fragments
        let fragments = fragment_manager.split(data);

        // Send all fragments
        for (i, fragment) in fragments.iter().enumerate() {
            let is_last = i == fragments.len() - 1;
            let status = if is_last { Status::Last } else { Status::More };

            let seq = self.sequence.next_seq();
            let msg = Message::new(
                crate::protocol::Header::new(
                    status,
                    params.length_present,
                    seq,
                    if params.length_present {
                        Some(fragment.len() as u16)
                    } else {
                        None
                    },
                )?,
                fragment.clone(),
            )?;

            // Send fragment
            self.transport.send(&msg.to_bytes())?;

            // Increment pending count
            flow_control.increment_pending();

            // Wait for ACK if needed (but not after last fragment with fragment_count=1)
            if flow_control.should_wait_for_ack() && !(is_last && params.fragment_count == 1) {
                flow_control.wait_for_ack(&mut self.transport, params.length_present)?;
            }
        }

        // Now receive the response
        self.receive_response()
    }

    /// Receive a response from the slave
    fn receive_response(&mut self) -> Result<Bytes> {
        let params = self
            .parameters
            .as_ref()
            .ok_or_else(|| Error::InvalidState("Parameters not initialized".to_string()))?;

        let fragment_manager = self.fragment_manager.as_mut().ok_or_else(|| {
            Error::InvalidState("Fragment manager not initialized".to_string())
        })?;

        let flow_control = self.flow_control.as_mut().ok_or_else(|| {
            Error::InvalidState("Flow control not initialized".to_string())
        })?;

        // Clear previous assembly
        fragment_manager.clear();

        // Receive response sequence tracker
        let mut response_seq = SequenceTracker::new();

        loop {
            // Receive message
            let mut buf = [0u8; 4096];
            let n = self
                .transport
                .receive_timeout(&mut buf, params.timeout)?;

            let (msg, _) = Message::from_bytes(&buf[..n])?;

            // Check for cancel
            if msg.is_cancel() {
                let reason = String::from_utf8_lossy(&msg.payload);
                fragment_manager.clear();
                return Err(Error::Cancelled(reason.to_string()));
            }

            // Validate sequence
            if !response_seq.validate_and_advance(msg.header.seq) {
                // Send cancel and return error
                flow_control.send_cancel(
                    &mut self.transport,
                    msg.header.seq,
                    params.length_present,
                    "Sequence error",
                )?;
                fragment_manager.clear();
                return Err(Error::Sequence {
                    expected: response_seq.current(),
                    actual: msg.header.seq,
                });
            }

            // Check if this was the last fragment before moving payload
            let is_last = msg.is_last();

            // Add fragment
            fragment_manager.add_fragment(msg.payload)?;

            // Send ACK
            flow_control.send_ack(&mut self.transport, msg.header.seq, params.length_present)?;

            // Check if this was the last fragment
            if is_last {
                break;
            }
        }

        Ok(fragment_manager.finalize())
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

    /// Get negotiated parameters
    pub fn parameters(&self) -> Option<&Parameters> {
        self.parameters.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockTransport;

    #[test]
    fn test_master_initialization() {
        let mut transport = MockTransport::new();
        
        // Prepare the slave response in advance
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

        // Try to send request before initialization
        let result = master.request(b"test");
        assert!(matches!(result, Err(Error::InvalidState(_))));
    }
}
