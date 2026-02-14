use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::{Header, Message, Parameters, Status};
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::{Bytes, BytesMut};

pub struct SlaveConnection<T: Transport> {
    transport: T,
    state: ConnectionState,
    parameters: Parameters,
    sequence: SequenceTracker,
}

impl<T: Transport> SlaveConnection<T> {
    pub fn new(transport: T, parameters: Parameters) -> Self {
        SlaveConnection {
            transport,
            state: ConnectionState::Uninitialized,
            parameters,
            sequence: SequenceTracker::new(),
        }
    }

    pub fn with_defaults(transport: T) -> Self {
        Self::new(transport, Parameters::default_slave())
    }

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
            .map_err(|_| Error::InvalidParameters("Request is not valid UTF-8".to_string()))?;

        if request != Parameters::INIT_REQUEST {
            return Err(Error::Protocol(format!(
                "Expected '{}', got '{}'",
                Parameters::INIT_REQUEST,
                request
            )));
        }

        let response = self.parameters.to_response();
        self.transport.send(response.as_bytes())?;

        self.state = ConnectionState::Ready;
        self.sequence.reset();

        Ok(())
    }

    pub fn receive_request(&mut self) -> Result<Bytes> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot receive request in state {:?}",
                self.state
            )));
        }

        let mut request_seq = SequenceTracker::new();
        let mut request_data = BytesMut::new();

        loop {
            let msg = self.read_message()?;

            if msg.is_cancel() {
                let reason = String::from_utf8_lossy(&msg.payload);
                return Err(Error::Cancelled(reason.to_string()));
            }

            if !request_seq.validate_and_advance(msg.header.seq) {
                self.send_cancel(msg.header.seq, "Sequence error")?;
                return Err(Error::Sequence {
                    expected: request_seq.current(),
                    actual: msg.header.seq,
                });
            }

            let msg_seq = msg.header.seq;
            let is_last = msg.is_last();
            request_data.extend_from_slice(&msg.payload);

            if is_last {
                return Ok(request_data.freeze());
            }

            self.send_ack(msg_seq)?;
        }
    }

    pub fn send_response(&mut self, data: &[u8]) -> Result<()> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send response in state {:?}",
                self.state
            )));
        }

        self.sequence.reset();

        let max_size = self.parameters.fragment_max_size as usize;
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

        Ok(())
    }

    pub fn send_error(&mut self, reason: &str) -> Result<()> {
        if !self.state.can_transfer() {
            return Err(Error::InvalidState(format!(
                "Cannot send error in state {:?}",
                self.state
            )));
        }

        self.send_cancel(self.sequence.current(), reason)
    }

    fn read_message(&mut self) -> Result<Message> {
        let mut buf = [0u8; 1];
        let n = self
            .transport
            .receive_timeout(&mut buf, self.parameters.timeout)?;

        if n != 1 {
            return Err(Error::Protocol("Failed to read header byte".to_string()));
        }

        let header = Header::from_bytes(&buf)?;

        if !header.length_present {
            return Err(Error::Protocol(
                "Protocol version without length not supported".to_string(),
            ));
        }

        let mut len_buf = [0u8; 2];
        let n = self
            .transport
            .receive_timeout(&mut len_buf, self.parameters.timeout)?;

        if n != 2 {
            return Err(Error::Protocol("Failed to read length field".to_string()));
        }

        let length = u16::from_be_bytes(len_buf);

        let mut payload = vec![0u8; length as usize];
        if length > 0 {
            let n = self
                .transport
                .receive_timeout(&mut payload, self.parameters.timeout)?;

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

    fn send_cancel(&mut self, seq: u8, reason: &str) -> Result<()> {
        let cancel = Message::cancel(seq, true, Bytes::from(reason.to_string()))?;
        self.transport.send(&cancel.to_bytes())?;
        Ok(())
    }

    pub fn close(mut self) -> Result<()> {
        self.state = ConnectionState::Closed;
        self.transport.close()
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

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
