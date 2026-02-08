use crate::error::{Error, Result};
use crate::protocol::Message;
use crate::transport::Transport;
use std::time::Duration;

/// Flow control manager for handling ACKs and timeouts
#[derive(Debug)]
pub struct FlowControl {
    /// Number of fragments to send before waiting for ACK
    fragment_count: u8,
    /// Current count of unacknowledged fragments
    pending_count: u8,
    /// Timeout for waiting for ACK
    timeout: Duration,
}

impl FlowControl {
    /// Create a new flow control manager
    pub fn new(fragment_count: u8, timeout: Duration) -> Self {
        FlowControl {
            fragment_count,
            pending_count: 0,
            timeout,
        }
    }

    /// Check if we should wait for ACK before sending more
    pub fn should_wait_for_ack(&self) -> bool {
        self.pending_count >= self.fragment_count
    }

    /// Increment pending fragment count
    pub fn increment_pending(&mut self) {
        self.pending_count += 1;
    }

    /// Reset pending count after receiving ACK
    pub fn reset_pending(&mut self) {
        self.pending_count = 0;
    }

    /// Get current pending count
    pub fn pending_count(&self) -> u8 {
        self.pending_count
    }

    /// Get timeout duration
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Wait for ACK message from transport
    pub fn wait_for_ack<T: Transport>(
        &mut self,
        transport: &mut T,
        length_present: bool,
    ) -> Result<Message> {
        let mut buf = [0u8; 1024];
        let n = transport.receive_timeout(&mut buf, self.timeout)?;

        let (msg, _) = Message::from_bytes(&buf[..n])?;

        // Validate it's an ACK
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

        // Validate length_present matches
        if msg.header.length_present != length_present {
            return Err(Error::Protocol(format!(
                "ACK length_present mismatch: expected {}, got {}",
                length_present, msg.header.length_present
            )));
        }

        self.reset_pending();
        Ok(msg)
    }

    /// Send ACK message
    pub fn send_ack<T: Transport>(
        &self,
        transport: &mut T,
        seq: u8,
        length_present: bool,
    ) -> Result<()> {
        let ack = Message::ack(seq, length_present)?;
        let bytes = ack.to_bytes();
        transport.send(&bytes)?;
        Ok(())
    }

    /// Send cancel/error message
    pub fn send_cancel<T: Transport>(
        &self,
        transport: &mut T,
        seq: u8,
        length_present: bool,
        reason: &str,
    ) -> Result<()> {
        let cancel = Message::cancel(
            seq,
            length_present,
            bytes::Bytes::from(reason.to_string()),
        )?;
        let bytes = cancel.to_bytes();
        transport.send(&bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockTransport;

    #[test]
    fn test_flow_control() {
        let mut flow = FlowControl::new(2, Duration::from_secs(5));
        
        assert!(!flow.should_wait_for_ack());
        assert_eq!(flow.pending_count(), 0);
        
        flow.increment_pending();
        assert!(!flow.should_wait_for_ack());
        assert_eq!(flow.pending_count(), 1);
        
        flow.increment_pending();
        assert!(flow.should_wait_for_ack());
        assert_eq!(flow.pending_count(), 2);
        
        flow.reset_pending();
        assert!(!flow.should_wait_for_ack());
        assert_eq!(flow.pending_count(), 0);
    }

    #[test]
    fn test_ack_roundtrip() {
        let flow = FlowControl::new(1, Duration::from_secs(5));
        let mut transport = MockTransport::new();
        
        // Send ACK
        flow.send_ack(&mut transport, 5, true).unwrap();
        
        // Verify transmitted data
        let tx_data = transport.get_tx_data();
        let (msg, _) = Message::from_bytes(&tx_data).unwrap();
        
        assert!(msg.is_ack());
        assert_eq!(msg.header.seq, 5);
        assert_eq!(msg.header.length_present, true);
    }

    #[test]
    fn test_cancel_message() {
        let flow = FlowControl::new(1, Duration::from_secs(5));
        let mut transport = MockTransport::new();
        
        // Send cancel
        flow.send_cancel(&mut transport, 3, true, "Test error").unwrap();
        
        // Verify transmitted data
        let tx_data = transport.get_tx_data();
        let (msg, _) = Message::from_bytes(&tx_data).unwrap();
        
        assert!(msg.is_cancel());
        assert_eq!(msg.header.seq, 3);
        assert_eq!(msg.payload, bytes::Bytes::from("Test error"));
    }

    #[test]
    fn test_wait_for_ack_timeout() {
        let mut flow = FlowControl::new(1, Duration::from_millis(100));
        let mut transport = MockTransport::new();
        
        let result = flow.wait_for_ack(&mut transport, true);
        assert!(matches!(result, Err(Error::Timeout)));
    }

    #[test]
    fn test_wait_for_ack_success() {
        let mut flow = FlowControl::new(1, Duration::from_secs(5));
        let mut transport = MockTransport::new();
        
        // Prepare ACK in receive buffer
        let ack = Message::ack(0, true).unwrap();
        transport.add_rx_data(&ack.to_bytes());
        
        let result = flow.wait_for_ack(&mut transport, true);
        assert!(result.is_ok());
        assert_eq!(flow.pending_count(), 0);
    }

    #[test]
    fn test_wait_for_ack_receives_cancel() {
        let mut flow = FlowControl::new(1, Duration::from_secs(5));
        let mut transport = MockTransport::new();
        
        // Prepare CANCEL in receive buffer
        let cancel = Message::cancel(0, true, bytes::Bytes::from("error")).unwrap();
        transport.add_rx_data(&cancel.to_bytes());
        
        let result = flow.wait_for_ack(&mut transport, true);
        assert!(matches!(result, Err(Error::Cancelled(_))));
    }
}
