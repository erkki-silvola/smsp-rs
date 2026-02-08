use crate::error::{Error, Result};
use crate::protocol::header::{Header, Status};
use bytes::{Bytes, BytesMut, BufMut};

/// Complete SMSP message with header and payload
#[derive(Debug, Clone)]
pub struct Message {
    /// Message header
    pub header: Header,
    /// Message payload
    pub payload: Bytes,
}

impl Message {
    /// Create a new message
    pub fn new(header: Header, payload: Bytes) -> Result<Self> {
        // Validate payload length matches header
        if let Some(length) = header.length {
            if payload.len() != length as usize {
                return Err(Error::Protocol(format!(
                    "Payload length {} does not match header length {}",
                    payload.len(),
                    length
                )));
            }
        }

        Ok(Message { header, payload })
    }

    /// Create a message with MORE status
    pub fn more(seq: u8, length_present: bool, payload: Bytes) -> Result<Self> {
        let length = if length_present {
            Some(payload.len() as u16)
        } else {
            None
        };
        let header = Header::new(Status::More, length_present, seq, length)?;
        Ok(Message { header, payload })
    }

    /// Create a message with LAST status
    pub fn last(seq: u8, length_present: bool, payload: Bytes) -> Result<Self> {
        let length = if length_present {
            Some(payload.len() as u16)
        } else {
            None
        };
        let header = Header::new(Status::Last, length_present, seq, length)?;
        Ok(Message { header, payload })
    }

    /// Create a CANCEL/ERROR message
    pub fn cancel(seq: u8, length_present: bool, reason: Bytes) -> Result<Self> {
        let length = if length_present {
            Some(reason.len() as u16)
        } else {
            None
        };
        let header = Header::new(Status::Cancel, length_present, seq, length)?;
        Ok(Message {
            header,
            payload: reason,
        })
    }

    /// Create an ACK message
    pub fn ack(seq: u8, length_present: bool) -> Result<Self> {
        let header = Header::ack(seq, length_present)?;
        Ok(Message {
            header,
            payload: Bytes::new(),
        })
    }

    /// Serialize message to bytes
    pub fn to_bytes(&self) -> Bytes {
        let header_bytes = self.header.to_bytes();
        let mut buf = BytesMut::with_capacity(header_bytes.len() + self.payload.len());
        buf.put_slice(&header_bytes);
        buf.put_slice(&self.payload);
        buf.freeze()
    }

    /// Deserialize message from bytes
    /// Returns the message and the number of bytes consumed
    pub fn from_bytes(bytes: &[u8]) -> Result<(Self, usize)> {
        if bytes.is_empty() {
            return Err(Error::Protocol("Empty buffer".to_string()));
        }

        // Parse header
        let header = Header::from_bytes(bytes)?;
        let header_size = header.size();

        // Get payload
        let payload_len = header.length.unwrap_or(0) as usize;
        let total_len = header_size + payload_len;

        if bytes.len() < total_len {
            return Err(Error::BufferTooSmall {
                needed: total_len,
                available: bytes.len(),
            });
        }

        let payload = Bytes::copy_from_slice(&bytes[header_size..total_len]);
        let message = Message::new(header, payload)?;

        Ok((message, total_len))
    }

    /// Get total size of the message in bytes
    pub fn size(&self) -> usize {
        self.header.size() + self.payload.len()
    }

    /// Check if this is an ACK message
    pub fn is_ack(&self) -> bool {
        self.header.is_ack()
    }

    /// Check if this is a CANCEL message
    pub fn is_cancel(&self) -> bool {
        self.header.status == Status::Cancel
    }

    /// Check if this is the last fragment
    pub fn is_last(&self) -> bool {
        self.header.status == Status::Last
    }

    /// Check if more fragments are coming
    pub fn is_more(&self) -> bool {
        self.header.status == Status::More
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let payload = Bytes::from_static(b"Hello, World!");
        let msg = Message::more(5, true, payload.clone()).unwrap();

        assert_eq!(msg.header.seq, 5);
        assert_eq!(msg.header.status, Status::More);
        assert_eq!(msg.payload, payload);
    }

    #[test]
    fn test_ack_message() {
        let msg = Message::ack(0, true).unwrap();
        assert!(msg.is_ack());
        assert_eq!(msg.payload.len(), 0);
    }

    #[test]
    fn test_cancel_message() {
        let reason = Bytes::from_static(b"error");
        let msg = Message::cancel(0, true, reason.clone()).unwrap();
        assert!(msg.is_cancel());
        assert_eq!(msg.payload, reason);
    }

    #[test]
    fn test_serialization() {
        let payload = Bytes::from_static(b"test");
        let msg = Message::last(10, true, payload).unwrap();
        
        let bytes = msg.to_bytes();
        let (decoded, consumed) = Message::from_bytes(&bytes).unwrap();

        assert_eq!(consumed, bytes.len());
        assert_eq!(decoded.header, msg.header);
        assert_eq!(decoded.payload, msg.payload);
    }

    #[test]
    fn test_multiple_messages_in_buffer() {
        let msg1 = Message::more(0, true, Bytes::from_static(b"first")).unwrap();
        let msg2 = Message::last(1, true, Bytes::from_static(b"second")).unwrap();

        let bytes1 = msg1.to_bytes();
        let bytes2 = msg2.to_bytes();

        let mut combined = BytesMut::new();
        combined.put_slice(&bytes1);
        combined.put_slice(&bytes2);

        let (decoded1, consumed1) = Message::from_bytes(&combined).unwrap();
        let (decoded2, consumed2) = Message::from_bytes(&combined[consumed1..]).unwrap();

        assert_eq!(decoded1.payload, msg1.payload);
        assert_eq!(decoded2.payload, msg2.payload);
        assert_eq!(consumed1 + consumed2, combined.len());
    }
}
