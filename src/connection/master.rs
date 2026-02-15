use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::{Header, Message, Parameters, Status};
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::{Bytes, BytesMut};
use std::time::Duration;

pub struct MasterConnection<T: Transport> {
    transport: T,
    state: ConnectionState,
    parameters: Option<Parameters>,
    sequence: SequenceTracker,
}

impl<T: Transport> MasterConnection<T> {
    pub fn new(transport: T) -> Self {
        MasterConnection {
            transport,
            state: ConnectionState::Uninitialized,
            parameters: None,
            sequence: SequenceTracker::new(),
        }
    }

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
            .map_err(|_| Error::InvalidParameters("Response is not valid UTF-8".to_string()))?;

        let parameters = Parameters::from_response(response)?;

        if !parameters.length_present {
            return Err(Error::InvalidParameters(
                "Master only implements version where lenght needs to be present".to_string(),
            ));
        }

        self.parameters = Some(parameters.clone());
        self.state = ConnectionState::Ready;
        self.sequence.reset();

        Ok(parameters)
    }

    pub fn request(&mut self, data: &[u8]) -> Result<Bytes> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send request in state {:?}",
                self.state
            )));
        }

        let max_size = self
            .parameters
            .as_ref()
            .ok_or_else(|| Error::InvalidState("Parameters not initialized".to_string()))?
            .fragment_max_size as usize;

        let mut offset = 0;

        while offset < data.len() {
            let end = (offset + max_size).min(data.len());
            let fragment = &data[offset..end];
            let is_last = end >= data.len();

            let status = if is_last { Status::Last } else { Status::More };
            let seq = self.sequence.next_seq();

            let msg = Message::new(
                Header::new(status, true, seq, Some(fragment.len() as u16))?,
                Bytes::copy_from_slice(fragment),
            )?;

            self.transport.send(&msg.to_bytes())?;

            if !is_last {
                self.read_ack()?;
            }

            offset = end;
        }

        self.receive_response()
    }

    fn receive_response(&mut self) -> Result<Bytes> {
        let mut response_seq = SequenceTracker::new();
        let mut response = BytesMut::new();

        loop {
            let msg = self.read_message()?;

            if msg.is_cancel() {
                let reason = String::from_utf8_lossy(&msg.payload);
                return Err(Error::Cancelled(reason.to_string()));
            }

            if !response_seq.validate_and_advance(msg.header.seq) {
                return Err(Error::Sequence {
                    expected: response_seq.current(),
                    actual: msg.header.seq,
                });
            }

            let msg_seq = msg.header.seq;
            let is_last = msg.is_last();
            response.extend_from_slice(&msg.payload);

            if is_last {
                return Ok(response.freeze());
            }

            self.send_ack(msg_seq)?;
        }
    }

    fn read_message(&mut self) -> Result<Message> {
        let timeout = self
            .parameters
            .as_ref()
            .ok_or_else(|| Error::InvalidState("Parameters not initialized".to_string()))?
            .timeout;

        let mut buf = [0u8; 1];
        let n = self.transport.receive_timeout(&mut buf, timeout)?;

        assert_eq!(n, 1);

        let header = Header::from_bytes(&buf)?;

        assert_eq!(header.length_present, true);

        let mut len_buf = [0u8; 2];
        let n = self.transport.receive_timeout(&mut len_buf, timeout)?;

        assert_eq!(n, 2);

        let length = u16::from_be_bytes(len_buf);

        let mut payload = vec![0u8; length as usize];
        if length > 0 {
            let n = self.transport.receive_timeout(&mut payload, timeout)?;

            if n != length as usize {
                return Err(Error::Protocol(format!(
                    "Expected {} bytes, got {}",
                    length, n
                )));
            }
        }

        let full_header = Header::new(header.status, true, header.seq, Some(length))?;
        Message::new(full_header, Bytes::from(payload))
    }

    fn read_ack(&mut self) -> Result<()> {
        let msg = self.read_message()?;

        if !msg.is_ack() {
            if msg.is_cancel() {
                let reason = String::from_utf8_lossy(&msg.payload);
                return Err(Error::Cancelled(reason.to_string()));
            }
            return Err(Error::Protocol(format!(
                "Expected ACK, got status {:?}",
                msg.header.status
            )));
        }

        Ok(())
    }

    fn send_ack(&mut self, seq: u8) -> Result<()> {
        let ack = Message::ack(seq, true)?;
        self.transport.send(&ack.to_bytes())?;
        Ok(())
    }

    pub fn close(mut self) -> Result<()> {
        self.state = ConnectionState::Closed;
        self.transport.close()
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

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
