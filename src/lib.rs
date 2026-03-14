//! # SMSP-RS
//!
//! Simple Message Streaming Protocol (SMSP) implementation in Rust.
//!
//! SMSP is a lightweight protocol designed for IoT devices that provides
//! reliable request/response communication with message fragmentation
//! and flow control.
//!
//! ## Architecture
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
//! let transport = TcpTransport::connect("192.168.1.100:8080")?;
//! let mut master = MasterConnection::new(transport);
//! let params = master.initialize()?;
//! let response = master.request(b"Hello, Device!")?;
//! master.close()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Example (Slave)
//!
//! ```rust,no_run
//! use smsp_rs::{SlaveConnection, TcpTransport};
//! use std::net::TcpListener;
//!
//! # fn main() -> smsp_rs::Result<()> {
//! let listener = TcpListener::bind("0.0.0.0:8080")?;
//! for stream in listener.incoming() {
//!     let transport = TcpTransport::from_stream(stream?)?;
//!     let mut slave = SlaveConnection::with_defaults(transport);
//!     slave.wait_for_init()?;
//!     let request = slave.receive_request()?;
//!     slave.send_response(b"Hello, Master!")?;
//!     slave.close()?;
//! }
//! # Ok(())
//! # }
//! ```

pub mod connection;
pub mod error;
pub mod protocol;
pub mod session;
pub mod transport;

pub use connection::{ConnectionState, MasterConnection, SlaveConnection};
pub use error::{Error, Result};
pub use protocol::{Header, Message, Parameters, Status};
pub use session::SequenceTracker;
pub use transport::Transport;

#[cfg(feature = "tcp")]
pub use transport::tcp::TcpTransport;

pub use transport::mock::MockTransport;

/// Protocol version supported by this implementation.
pub const PROTOCOL_VERSION: u8 = 1;
