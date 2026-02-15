use crate::error::{Error, Result};

/// Protocol status bits
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Last fragment or ACK as receiver
    Last = 0b00,
    /// More fragments to come
    More = 0b01,
    /// Cancel/Error
    Cancel = 0b10,
    /// Reserved for future use
    Reserved = 0b11,
}

impl Status {
    /// Create status from 2-bit value
    pub fn from_bits(bits: u8) -> Result<Self> {
        match bits & 0b11 {
            0b00 => Ok(Status::Last),
            0b01 => Ok(Status::More),
            0b10 => Ok(Status::Cancel),
            0b11 => Ok(Status::Reserved),
            _ => unreachable!(),
        }
    }

    /// Convert status to 2-bit value
    pub fn to_bits(self) -> u8 {
        self as u8
    }
}

/// Protocol header
///
/// Bit layout (Byte 0):
/// - Bits 0-1: status (2 bits)
/// - Bit 2: length_present (1 bit)
/// - Bit 3: reserved (1 bit)
/// - Bits 4-7: sequence number (4 bits)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Message status
    pub status: Status,
    /// Whether length field is present
    pub length_present: bool,
    /// Sequence number (0-15)
    pub seq: u8,
    /// Payload length (only if length_present is true)
    pub length: Option<u16>,
}

impl Header {
    /// Minimum header size (without length field)
    pub const MIN_SIZE: usize = 1;

    /// Maximum header size (with length field)
    pub const MAX_SIZE: usize = 3;

    /// Create a new header
    pub fn new(status: Status, length_present: bool, seq: u8, length: Option<u16>) -> Result<Self> {
        if seq > 15 {
            return Err(Error::InvalidHeader(format!(
                "Sequence number {} exceeds maximum 15",
                seq
            )));
        }

        if length_present && length.is_none() {
            return Err(Error::InvalidHeader(
                "length_present is true but length is None".to_string(),
            ));
        }

        if !length_present && length.is_some() {
            return Err(Error::InvalidHeader(
                "length_present is false but length is Some".to_string(),
            ));
        }

        Ok(Header {
            status,
            length_present,
            seq,
            length,
        })
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::MAX_SIZE);

        // First byte: status (bits 0-1) | length_present (bit 2) | reserved (bit 3) | seq (bits 4-7)
        let first_byte =
            self.status.to_bits() | ((self.length_present as u8) << 2) | ((self.seq & 0x0F) << 4);

        bytes.push(first_byte);

        // Length field (big-endian u16)
        if let Some(length) = self.length {
            bytes.extend_from_slice(&length.to_be_bytes());
        }

        bytes
    }

    /// Deserialize header from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::InvalidHeader("Empty buffer".to_string()));
        }

        let first_byte = bytes[0];

        // Extract fields from first byte
        let status = Status::from_bits(first_byte & 0b11)?;
        let length_present = (first_byte & 0b00000100) != 0;
        let seq = (first_byte >> 4) & 0x0F;

        // Parse length if present
        let length = if length_present {
            if bytes.len() < 3 {
                return Err(Error::InvalidHeader(
                    "Buffer too small for length field".to_string(),
                ));
            }
            Some(u16::from_be_bytes([bytes[1], bytes[2]]))
        } else {
            None
        };

        Header::new(status, length_present, seq, length)
    }

    /// Get the size of this header in bytes
    pub fn size(&self) -> usize {
        if self.length_present {
            Self::MAX_SIZE
        } else {
            Self::MIN_SIZE
        }
    }

    /// Create an ACK header
    pub fn ack(seq: u8, length_present: bool) -> Result<Self> {
        let length = if length_present { Some(0) } else { None };
        Header::new(Status::Last, length_present, seq, length)
    }

    /// Check if this is an ACK header
    pub fn is_ack(&self) -> bool {
        self.status == Status::Last && self.length.unwrap_or(0) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_conversion() {
        assert_eq!(Status::from_bits(0b00).unwrap(), Status::Last);
        assert_eq!(Status::from_bits(0b01).unwrap(), Status::More);
        assert_eq!(Status::from_bits(0b10).unwrap(), Status::Cancel);
        assert_eq!(Status::from_bits(0b11).unwrap(), Status::Reserved);
    }

    #[test]
    fn test_header_serialization() {
        let header = Header::new(Status::More, true, 5, Some(256)).unwrap();
        let bytes = header.to_bytes();

        assert_eq!(bytes.len(), 3);
        assert_eq!(bytes[0] & 0b00000011, 0b00000001); // Status::More in bits 0-1
        assert_eq!(bytes[0] & 0b00000100, 0b00000100); // length_present in bit 2
        assert_eq!((bytes[0] >> 4) & 0x0F, 5); // seq in bits 4-7
        assert_eq!(u16::from_be_bytes([bytes[1], bytes[2]]), 256); // length
    }

    #[test]
    fn test_header_deserialization() {
        // Byte 0: status=MORE(01) in bits 0-1, length_present=1 in bit 2, seq=5 in bits 4-7
        // 0b01010101 = 0x55: status=01, length_present=1, reserved=0, seq=0101
        let bytes = vec![0b01010101, 0x01, 0x00];
        let header = Header::from_bytes(&bytes).unwrap();

        assert_eq!(header.status, Status::More);
        assert_eq!(header.length_present, true);
        assert_eq!(header.seq, 5);
        assert_eq!(header.length, Some(256));
    }

    #[test]
    fn test_ack_header() {
        let header = Header::ack(0, true).unwrap();
        assert!(header.is_ack());
        assert_eq!(header.status, Status::Last);
        assert_eq!(header.length, Some(0));
    }

    #[test]
    fn test_roundtrip() {
        let original = Header::new(Status::Last, true, 15, Some(1024)).unwrap();
        let bytes = original.to_bytes();
        let decoded = Header::from_bytes(&bytes).unwrap();

        assert_eq!(original, decoded);
    }
}
