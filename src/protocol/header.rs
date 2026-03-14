use crate::error::{Error, Result};

/// Protocol message status (2 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Last fragment or ACK (as receiver).
    Last = 0b00,
    /// More fragments to follow.
    More = 0b01,
    /// Cancel / error.
    Cancel = 0b10,
    /// Reserved for future use.
    Reserved = 0b11,
}

impl Status {
    /// Create status from 2-bit value.
    pub fn from_bits(bits: u8) -> Result<Self> {
        match bits & 0b11 {
            0b00 => Ok(Self::Last),
            0b01 => Ok(Self::More),
            0b10 => Ok(Self::Cancel),
            0b11 => Ok(Self::Reserved),
            _ => unreachable!(),
        }
    }

    /// Convert status to 2-bit value.
    pub fn to_bits(self) -> u8 {
        self as u8
    }
}

/// Protocol header.
///
/// Bit layout (Byte 0):
/// - Bits 0-1: status (2 bits)
/// - Bit 2: length_present (1 bit)
/// - Bit 3: reserved (1 bit)
/// - Bits 4-7: sequence number (4 bits)
///
/// Bytes 1-2 (optional): payload length as big-endian u16
/// (only present when `length_present` is true).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub status: Status,
    pub length_present: bool,
    pub seq: u8,
    pub length: Option<u16>,
}

impl Header {
    /// Minimum header size (without length field).
    pub const MIN_SIZE: usize = 1;
    /// Maximum header size (with length field).
    pub const MAX_SIZE: usize = 3;

    /// Create a new header with validation.
    pub fn new(status: Status, length_present: bool, seq: u8, length: Option<u16>) -> Result<Self> {
        if seq > 15 {
            return Err(Error::InvalidHeader(format!(
                "Sequence number {seq} exceeds maximum 15"
            )));
        }
        if length_present && length.is_none() {
            return Err(Error::InvalidHeader(
                "length_present is true but length is None".into(),
            ));
        }
        if !length_present && length.is_some() {
            return Err(Error::InvalidHeader(
                "length_present is false but length is Some".into(),
            ));
        }
        Ok(Self {
            status,
            length_present,
            seq,
            length,
        })
    }

    /// Serialize header to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::MAX_SIZE);
        let first =
            self.status.to_bits() | ((self.length_present as u8) << 2) | ((self.seq & 0x0F) << 4);
        bytes.push(first);
        if let Some(len) = self.length {
            bytes.extend_from_slice(&len.to_be_bytes());
        }
        bytes
    }

    /// Deserialize header from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::InvalidHeader("Empty buffer".into()));
        }
        let first = bytes[0];
        let status = Status::from_bits(first & 0b11)?;
        let length_present = (first & 0b00000100) != 0;
        let seq = (first >> 4) & 0x0F;
        let length = if length_present {
            if bytes.len() < 3 {
                return Err(Error::InvalidHeader(
                    "Buffer too small for length field".into(),
                ));
            }
            Some(u16::from_be_bytes([bytes[1], bytes[2]]))
        } else {
            None
        };
        Self::new(status, length_present, seq, length)
    }

    /// Get the size of this header in bytes.
    pub fn size(&self) -> usize {
        if self.length_present {
            Self::MAX_SIZE
        } else {
            Self::MIN_SIZE
        }
    }

    /// Create an ACK header.
    pub fn ack(seq: u8, length_present: bool) -> Result<Self> {
        let length = if length_present { Some(0) } else { None };
        Self::new(Status::Last, length_present, seq, length)
    }

    /// Check if this is an ACK header (LAST status with zero-length payload).
    pub fn is_ack(&self) -> bool {
        self.status == Status::Last && self.length.unwrap_or(0) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_roundtrip() {
        for bits in 0..=3u8 {
            let s = Status::from_bits(bits).unwrap();
            assert_eq!(s.to_bits(), bits);
        }
    }

    #[test]
    fn test_header_serialization_with_length() {
        let header = Header::new(Status::More, true, 5, Some(256)).unwrap();
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 3);
        assert_eq!(bytes[0] & 0b11, 0b01);
        assert_eq!(bytes[0] & 0b100, 0b100);
        assert_eq!((bytes[0] >> 4) & 0x0F, 5);
        assert_eq!(u16::from_be_bytes([bytes[1], bytes[2]]), 256);
    }

    #[test]
    fn test_header_serialization_without_length() {
        let header = Header::new(Status::More, false, 3, None).unwrap();
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0] & 0b11, 0b01);
        assert_eq!(bytes[0] & 0b100, 0);
        assert_eq!((bytes[0] >> 4) & 0x0F, 3);
    }

    #[test]
    fn test_header_roundtrip() {
        let original = Header::new(Status::Last, true, 15, Some(1024)).unwrap();
        let decoded = Header::from_bytes(&original.to_bytes()).unwrap();
        assert_eq!(original, decoded);

        let original = Header::new(Status::Cancel, false, 7, None).unwrap();
        let decoded = Header::from_bytes(&original.to_bytes()).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_ack_header() {
        let header = Header::ack(0, true).unwrap();
        assert!(header.is_ack());
        assert_eq!(header.status, Status::Last);
        assert_eq!(header.length, Some(0));

        let header = Header::ack(0, false).unwrap();
        assert!(header.is_ack());
        assert_eq!(header.length, None);
    }

    #[test]
    fn test_seq_out_of_range() {
        assert!(Header::new(Status::More, true, 16, Some(0)).is_err());
    }

    #[test]
    fn test_length_present_mismatch() {
        assert!(Header::new(Status::More, true, 0, None).is_err());
        assert!(Header::new(Status::More, false, 0, Some(100)).is_err());
    }
}
