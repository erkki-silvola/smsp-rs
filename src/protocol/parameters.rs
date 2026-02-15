use crate::error::{Error, Result};
use std::time::Duration;

/// Connection parameters negotiated during initialization
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameters {
    /// Protocol version
    pub protocol_version: u8,
    /// Whether length field is present in headers
    pub length_present: bool,
    /// Timeout for operations in milliseconds
    pub timeout: Duration,
    /// Number of fragments that can be sent before waiting for ACK
    pub fragment_count: u8,
    /// Maximum size of a single fragment
    pub fragment_max_size: u16,
}

impl Parameters {
    /// Default parameters for slave devices
    pub fn default_slave() -> Self {
        Parameters {
            protocol_version: 1,
            length_present: true,
            timeout: Duration::from_millis(20000),
            fragment_count: 1,
            fragment_max_size: 256,
        }
    }

    /// Parse parameters from the initialization response
    /// Format: "1,1,20000,1,256\n"
    /// Fields: protocol_version, length_present, timeout_ms, fragment_count, fragment_max_size
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
        let timeout = Duration::from_millis(timeout_ms);

        let fragment_count = parts[3].parse::<u8>().map_err(|_| {
            Error::InvalidParameters(format!("Invalid fragment_count: {}", parts[3]))
        })?;

        if fragment_count == 0 {
            return Err(Error::InvalidParameters(
                "fragment_count must be at least 1".to_string(),
            ));
        }

        let fragment_max_size = parts[4].parse::<u16>().map_err(|_| {
            Error::InvalidParameters(format!("Invalid fragment_max_size: {}", parts[4]))
        })?;

        if !length_present && fragment_count != 0 {
            return Err(Error::InvalidParameters(
                "When length_present is false, fragment_count must be 0".to_string(),
            ));
        }

        if length_present && fragment_count != 1 {
            return Err(Error::InvalidParameters(
                "When length_present is true, fragment_count must be 1".to_string(),
            ));
        }

        Ok(Parameters {
            protocol_version,
            length_present,
            timeout,
            fragment_count,
            fragment_max_size,
        })
    }

    /// Format parameters as response string
    /// Format: "protocol_version,length_present,timeout_ms,fragment_count,fragment_max_size\n"
    pub fn to_response(&self) -> String {
        format!(
            "{},{},{},{},{}\n",
            self.protocol_version,
            if self.length_present { 1 } else { 0 },
            self.timeout.as_millis(),
            self.fragment_count,
            self.fragment_max_size
        )
    }

    /// The initialization request sent by master
    pub const INIT_REQUEST: &'static str = "parameters?\0";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_parameters() {
        let response = "1,1,20000,1,256\n";
        let params = Parameters::from_response(response).unwrap();

        assert_eq!(params.protocol_version, 1);
        assert_eq!(params.length_present, true);
        assert_eq!(params.timeout, Duration::from_millis(20000));
        assert_eq!(params.fragment_count, 1);
        assert_eq!(params.fragment_max_size, 256);
    }

    #[test]
    fn test_format_parameters() {
        let params = Parameters::default_slave();
        let response = params.to_response();

        assert_eq!(response, "1,1,20000,1,256\n");
    }

    #[test]
    fn test_roundtrip() {
        let original = Parameters::default_slave();
        let response = original.to_response();
        let parsed = Parameters::from_response(&response).unwrap();

        assert_eq!(original, parsed);
    }

    #[test]
    fn test_invalid_parameters() {
        assert!(Parameters::from_response("1,1,20000").is_err());

        assert!(Parameters::from_response("1,2,20000,1,256\n").is_err());

        assert!(Parameters::from_response("1,1,20000,0,256\n").is_err());

        assert!(Parameters::from_response("1,1,20000,1,0\n").is_err());

        assert!(Parameters::from_response("1,1,20000,2,256\n").is_err());
    }
}
