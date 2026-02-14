//! # SMSP-RS
//!
//! Simple Message Streaming Protocol (SMSP) implementation in Rust.
//!
//! SMSP is a lightweight protocol designed for IoT devices that provides
//! a simple and flexible mechanism for request/response transmission with
//! support for message fragmentation and flow control.
//!
//! ## Features
//!
//! - **Abstract Transport Layer**: Works with any transport (TCP, Serial, etc.)
//! - **Fragmentation Support**: Automatic message splitting and reassembly
//! - **Flow Control**: Configurable ACK mechanism for reliable delivery
//! - **Master/Slave Architecture**: Clear roles for request initiation
//! - **Synchronous API**: Simple blocking I/O for easy integration
//!
//! ## Architecture
//!
//! The protocol consists of several layers:
//!
//! - **Protocol Layer**: Message format, headers, and parameters
//! - **Transport Layer**: Abstract interface for different transports
//! - **Session Layer**: Fragment management, sequencing, flow control
//! - **Connection Layer**: Master and slave connection management
//!
//! ## Example (Master)
//!
//! ```rust,no_run
//! use smsp_rs::{MasterConnection, TcpTransport};
//!
//! # fn main() -> smsp_rs::Result<()> {
//! // Connect to slave device
//! let transport = TcpTransport::connect("192.168.1.100:8080")?;
//! let mut master = MasterConnection::new(transport);
//!
//! // Initialize connection and negotiate parameters
//! let params = master.initialize()?;
//! println!("Connected with parameters: {:?}", params);
//!
//! // Send request and receive response
//! let response = master.request(b"Hello, Device!")?;
//! println!("Response: {:?}", response);
//!
//! // Close connection
//! master.close()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Example (Slave)
//!
//! ```rust,no_run
//! use smsp_rs::{SlaveConnection, TcpTransport, Parameters};
//! use std::net::TcpListener;
//!
//! # fn main() -> smsp_rs::Result<()> {
//! let listener = TcpListener::bind("0.0.0.0:8080")?;
//! println!("Listening on port 8080...");
//!
//! for stream in listener.incoming() {
//!     let transport = TcpTransport::from_stream(stream?)?;
//!     let mut slave = SlaveConnection::with_defaults(transport);
//!
//!     // Wait for initialization
//!     slave.wait_for_init()?;
//!     println!("Connection initialized");
//!
//!     // Receive request
//!     let request = slave.receive_request()?;
//!     println!("Request: {:?}", request);
//!
//!     // Send response
//!     slave.send_response(b"Hello, Master!")?;
//!
//!     slave.close()?;
//! }
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod protocol;
pub mod transport;
pub mod session;
pub mod connection;

// Re-export commonly used types
pub use error::{Error, Result};
pub use protocol::{Header, Message, Parameters, Status};
pub use transport::Transport;
pub use connection::{ConnectionState, MasterConnection, SlaveConnection};

// Re-export transport implementations
#[cfg(feature = "tcp")]
pub use transport::tcp::TcpTransport;

pub use transport::mock::MockTransport;

// Re-export session types for advanced usage
pub use session::SequenceTracker;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol version supported by this implementation
pub const PROTOCOL_VERSION: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_defined() {
        assert!(!VERSION.is_empty());
        assert_eq!(PROTOCOL_VERSION, 1);
    }
}
