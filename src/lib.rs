//! # SMSP-RS
//!
//! Simple Message Streaming Protocol (SMSP) implementation in Rust.
//!
//! SMSP is a lightweight protocol designed for IoT devices that provides
//! reliable request/response communication with message fragmentation
//! and flow control.
//!
//! This crate provides an **async, master-only** client library. The library
//! is runtime-agnostic via the [`Transport`] trait; examples use Tokio.
//!
//! ## Features
//!
//! - `tcp` (default): [`TcpTransport`] via Tokio
//! - `serial`: [`SerialTransport`] via `serialport-stream`
//!
//! [`MockTransport`] is always available for testing and integration-style simulations.
//!
//! ## Architecture
//!
//! - **Protocol Layer**: Message format, headers, and parameters
//! - **Transport Layer**: Abstract async interface with capability metadata for different transports
//! - **Session Layer**: Fragment management, sequencing, flow control
//! - **Connection Layer**: Master connection management
//!
//! ## Example (Master)
//!
//! ```rust,no_run
//! use smsp_rs::{MasterConnection, TcpTransport};
//!
//! # async fn example() -> smsp_rs::Result<()> {
//! let transport = TcpTransport::connect("192.168.1.100:8080").await?;
//! let mut master = MasterConnection::new(transport);
//! let params = master.initialize().await?;
//! let response = master.request(b"Hello, Device!").await?;
//! master.close().await?;
//! # Ok(())
//! # }
//! ```

pub mod connection;
pub mod error;
pub mod protocol;
pub mod session;
pub mod transport;

pub use connection::{ConnectionState, MasterConnection};
pub use error::{Error, Result};
pub use protocol::{Header, Message, Parameters, Status, MAX_FRAGMENT_PAYLOAD};
pub use session::SequenceTracker;
pub use transport::{Transport, TransportCapabilities};

#[cfg(feature = "serial")]
pub use transport::serial::SerialTransport;

#[cfg(feature = "tcp")]
pub use transport::tcp::TcpTransport;

pub use transport::mock::MockTransport;

/// Protocol version supported by this implementation.
pub const PROTOCOL_VERSION: u8 = 1;
