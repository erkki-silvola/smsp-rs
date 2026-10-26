use crate::connection::io;
use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::Parameters;
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::Bytes;
use std::time::{Duration, Instant};

/// Master (connection initiator) side of the SMSP protocol.
///
/// The master initiates the connection, negotiates parameters with the
/// remote device, then performs request/response cycles. Large messages
/// are automatically fragmented according to the negotiated parameters.
///
/// # Example
///
/// ```rust,no_run
/// use smsp_rs::{MasterConnection, TcpTransport};
///
/// # async fn example() -> smsp_rs::Result<()> {
/// let transport = TcpTransport::connect("192.168.1.100:8080").await?;
/// let mut master = MasterConnection::new(transport);
/// let params = master.initialize().await?;
/// let response = master.request(b"Hello, Device!").await?;
/// master.close().await?;
/// # Ok(())
/// # }
/// ```
pub struct MasterConnection<T: Transport> {
    transport: T,
    state: ConnectionState,
    parameters: Option<Parameters>,
    sequence: SequenceTracker,
}

impl<T: Transport> MasterConnection<T> {
    /// Create a new master connection wrapping the given transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            state: ConnectionState::Uninitialized,
            parameters: None,
            sequence: SequenceTracker::new(),
        }
    }

    /// Initialize the connection by negotiating parameters with the remote device.
    ///
    /// Sends `"parameters?\0"` and expects a parameter response within 5 seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if not in `Uninitialized` state, on timeout,
    /// or if the remote parameters are invalid.
    pub async fn initialize(&mut self) -> Result<Parameters> {
        self.state.require_initializing()?;
        self.state = ConnectionState::Negotiating;
        self.transport
            .send(Parameters::INIT_REQUEST.as_bytes())
            .await?;

        let mut buf = [0u8; 256];
        let deadline = Instant::now() + Duration::from_secs(5);
        let parameters = loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                return Err(Error::Timeout);
            }
            let n = io::read_line(&mut self.transport, &mut buf, wait).await?;
            let response = std::str::from_utf8(&buf[..n])
                .map_err(|_| Error::InvalidParameters("Response is not valid UTF-8".into()))?;
            if response.trim().is_empty() {
                continue;
            }
            break Parameters::from_response(response)?;
        };
        if parameters.protocol_version != 1 {
            return Err(Error::InvalidParameters(format!(
                "Unsupported protocol version: {}",
                parameters.protocol_version
            )));
        }
        let caps = self.transport.capabilities();
        if caps.length_present && !parameters.length_present {
            return Err(Error::InvalidParameters(
                "Transport requires length field but device negotiated length_present=false".into(),
            ));
        }
        if !caps.length_present && parameters.length_present {
            return Err(Error::InvalidParameters(
                "Transport operates in no-length mode but device negotiated length_present=true"
                    .into(),
            ));
        }

        self.parameters = Some(parameters.clone());
        self.state = ConnectionState::Ready;
        self.sequence.reset();
        Ok(parameters)
    }

    /// Send a request and receive the response.
    ///
    /// The request data is automatically fragmented according to negotiated
    /// parameters. Returns the complete reassembled response.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection is not in `Ready` state, on
    /// timeout, or if the remote side cancels.
    pub async fn request(&mut self, data: &[u8]) -> Result<Bytes> {
        self.state.require_can_transfer()?;
        let params = self.params()?;
        let length_present = params.length_present;
        let fragment_count = params.fragment_count;
        let caps = self.transport.capabilities();
        let max_size = params.fragment_payload_limit(caps);
        let timeout = params.timeout;

        self.sequence.reset();
        io::send_fragments(
            &mut self.transport,
            data,
            length_present,
            fragment_count,
            max_size,
            timeout,
            &mut self.sequence,
        )
        .await?;
        io::receive_fragments(
            &mut self.transport,
            length_present,
            fragment_count,
            max_size,
            timeout,
        )
        .await
    }

    /// Close the connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the transport cannot be closed.
    pub async fn close(mut self) -> Result<()> {
        self.state = ConnectionState::Closed;
        self.transport.close().await
    }

    /// Get the current connection state.
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Get the negotiated parameters, if available.
    pub fn parameters(&self) -> Option<&Parameters> {
        self.parameters.as_ref()
    }

    /// Max payload bytes per SMSP fragment before an ACK.
    pub fn fragment_payload_limit(&self) -> Option<usize> {
        self.parameters
            .as_ref()
            .map(|p| p.fragment_payload_limit(self.transport.capabilities()))
    }

    fn params(&self) -> Result<Parameters> {
        self.parameters
            .clone()
            .ok_or_else(|| Error::InvalidState("Parameters not initialized".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockTransport;

    #[tokio::test]
    async fn test_master_initialization() {
        let mut transport = MockTransport::new();
        let params = Parameters::default_slave();
        transport.add_rx_data(params.to_response().as_bytes());
        let mut master = MasterConnection::new(transport);
        let negotiated = master.initialize().await.unwrap();
        assert_eq!(negotiated, params);
        assert_eq!(master.state(), ConnectionState::Ready);
    }

    #[tokio::test]
    async fn test_master_initialization_skips_spurious_newlines() {
        let mut transport = MockTransport::new();
        let params = Parameters::default_slave();
        transport.add_rx_data(b"\n\n");
        transport.add_rx_data(params.to_response().as_bytes());
        let mut master = MasterConnection::new(transport);
        let negotiated = master.initialize().await.unwrap();
        assert_eq!(negotiated, params);
    }

    #[tokio::test]
    async fn test_master_initialization_params_byte_by_byte() {
        let mut transport = MockTransport::new();
        let params = Parameters::default_slave();
        for byte in params.to_response().bytes() {
            transport.add_rx_data(&[byte]);
        }
        let mut master = MasterConnection::new(transport);
        let negotiated = master.initialize().await.unwrap();
        assert_eq!(negotiated, params);
    }

    #[tokio::test]
    async fn test_master_rejects_no_length_on_length_required_transport() {
        let mut transport = MockTransport::new();
        transport.add_rx_data(b"1,0,20000,2,0\n");
        let mut master = MasterConnection::new(transport);
        assert!(matches!(
            master.initialize().await,
            Err(Error::InvalidParameters(_))
        ));
    }

    #[tokio::test]
    async fn test_master_invalid_state() {
        let transport = MockTransport::new();
        let mut master = MasterConnection::new(transport);
        assert!(matches!(
            master.request(b"test").await,
            Err(Error::InvalidState(_))
        ));
    }
}
