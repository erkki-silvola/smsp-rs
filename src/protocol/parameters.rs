use crate::error::{Error, Result};
use crate::transport::TransportCapabilities;
use std::time::Duration;

/// Maximum SMSP payload bytes per fragment before an ACK (length-present and no-length modes).
pub const MAX_FRAGMENT_PAYLOAD: u16 = 256;

/// Connection parameters negotiated during initialization.
///
/// Format: `"<version>,<length_present>,<timeout_ms>,<fragment_count>,<fragment_max_size>\n"`
///
/// # Constraints
///
/// - `fragment_count` is always 1-255.
/// - When `length_present=true`, `fragment_count` MUST be 1.
/// - When `length_present=false`, `fragment_max_size` MUST be 0
///   (payload length is inferred from the transport layer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameters {
    pub protocol_version: u8,
    pub length_present: bool,
    pub timeout: Duration,
    pub fragment_count: u8,
    pub fragment_max_size: u16,
}

impl Parameters {
    /// The initialization request sent by master.
    pub const INIT_REQUEST: &'static str = "parameters?\0";

    /// Default parameters for slave devices (length_present=true, fragment_count=1).
    pub fn default_slave() -> Self {
        Self {
            protocol_version: 1,
            length_present: true,
            timeout: Duration::from_millis(20000),
            fragment_count: 1,
            fragment_max_size: 256,
        }
    }

    /// Parse parameters from the initialization response.
    ///
    /// # Errors
    ///
    /// Returns an error if the format is invalid or constraints are violated.
    pub fn from_response(response: &str) -> Result<Self> {
        let response = response.trim();
        let parts: Vec<&str> = response.split(',').collect();
        if parts.len() != 5 {
            return Err(Error::InvalidParameters(format!(
                "Expected 5 comma-separated values, got {}",
                parts.len()
            )));
        }

        let protocol_version = parts[0].parse::<u8>().map_err(|_| {
            Error::InvalidParameters(format!("Invalid protocol_version: {}", parts[0]))
        })?;

        let length_present = match parts[1] {
            "0" => false,
            "1" => true,
            _ => {
                return Err(Error::InvalidParameters(format!(
                    "Invalid length_present: {} (expected 0 or 1)",
                    parts[1]
                )))
            }
        };

        let timeout_ms = parts[2]
            .parse::<u64>()
            .map_err(|_| Error::InvalidParameters(format!("Invalid timeout: {}", parts[2])))?;

        let fragment_count = parts[3].parse::<u8>().map_err(|_| {
            Error::InvalidParameters(format!("Invalid fragment_count: {}", parts[3]))
        })?;

        let fragment_max_size = parts[4].parse::<u16>().map_err(|_| {
            Error::InvalidParameters(format!("Invalid fragment_max_size: {}", parts[4]))
        })?;

        if fragment_count == 0 {
            return Err(Error::InvalidParameters(
                "fragment_count must be at least 1".into(),
            ));
        }

        if length_present && fragment_count != 1 {
            return Err(Error::InvalidParameters(
                "When length_present is true, fragment_count must be 1".into(),
            ));
        }

        if !length_present && fragment_max_size != 0 {
            return Err(Error::InvalidParameters(
                "When length_present is false, fragment_max_size must be 0".into(),
            ));
        }

        Ok(Self {
            protocol_version,
            length_present,
            timeout: Duration::from_millis(timeout_ms),
            fragment_count,
            fragment_max_size,
        })
    }

    /// Payload limit per fragment before flow-control ACK, capped at [`MAX_FRAGMENT_PAYLOAD`].
    pub fn fragment_payload_limit(&self, caps: TransportCapabilities) -> usize {
        let raw = if self.length_present {
            self.fragment_max_size
        } else {
            caps.fragment_max_size
        };
        usize::from(raw.min(MAX_FRAGMENT_PAYLOAD)).max(1)
    }

    /// Format parameters as response string.
    pub fn to_response(&self) -> String {
        format!(
            "{},{},{},{},{}\n",
            self.protocol_version,
            u8::from(self.length_present),
            self.timeout.as_millis(),
            self.fragment_count,
            self.fragment_max_size
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportCapabilities;

    #[test]
    fn test_parse_length_present() {
        let params = Parameters::from_response("1,1,20000,1,256\n").unwrap();
        assert_eq!(params.protocol_version, 1);
        assert!(params.length_present);
        assert_eq!(params.timeout, Duration::from_millis(20000));
        assert_eq!(params.fragment_count, 1);
        assert_eq!(params.fragment_max_size, 256);
    }

    #[test]
    fn test_parse_no_length() {
        let params = Parameters::from_response("1,0,20000,2,0\n").unwrap();
        assert!(!params.length_present);
        assert_eq!(params.fragment_count, 2);
        assert_eq!(params.fragment_max_size, 0);
    }

    #[test]
    fn fragment_payload_limit_capped_at_max() {
        let mut params = Parameters::default_slave();
        params.fragment_max_size = 512;
        let caps = TransportCapabilities {
            length_present: true,
            fragment_max_size: 512,
        };
        assert_eq!(params.fragment_payload_limit(caps), 256);
    }

    #[test]
    fn test_roundtrip() {
        let original = Parameters::default_slave();
        let parsed = Parameters::from_response(&original.to_response()).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_rejects_fragment_count_zero() {
        assert!(Parameters::from_response("1,1,20000,0,256\n").is_err());
    }

    #[test]
    fn test_rejects_length_present_with_fragment_count_gt1() {
        assert!(Parameters::from_response("1,1,20000,2,256\n").is_err());
    }

    #[test]
    fn test_rejects_no_length_with_nonzero_max_size() {
        assert!(Parameters::from_response("1,0,20000,2,256\n").is_err());
    }

    #[test]
    fn test_rejects_wrong_field_count() {
        assert!(Parameters::from_response("1,1,20000\n").is_err());
    }

    #[test]
    fn test_rejects_invalid_length_present() {
        assert!(Parameters::from_response("1,2,20000,1,256\n").is_err());
    }
}
