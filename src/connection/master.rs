use crate::connection::io;
use crate::connection::state::ConnectionState;
use crate::error::{Error, Result};
use crate::protocol::Parameters;
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::Bytes;
use std::time::Duration;

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
        let n = self
            .transport
            .receive_timeout(&mut buf, Duration::from_secs(5))
            .await?;

        let response = std::str::from_utf8(&buf[..n])
            .map_err(|_| Error::InvalidParameters("Response is not valid UTF-8".into()))?;

        let parameters = Parameters::from_response(response)?;
        if parameters.protocol_version != 1 {
            return Err(Error::InvalidParameters(format!(
                "Unsupported protocol version: {}",
                parameters.protocol_version
            )));
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
        let max_size = params.fragment_max_size as usize;

        self.sequence.reset();
        let timeout = self.params()?.timeout;
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
        io::receive_fragments(&mut self.transport, length_present, fragment_count, timeout).await
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
    async fn test_master_invalid_state() {
        let transport = MockTransport::new();
        let mut master = MasterConnection::new(transport);
        assert!(matches!(
            master.request(b"test").await,
            Err(Error::InvalidState(_))
        ));
    }
}
