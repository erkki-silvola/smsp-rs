/// Connection state machine.
///
/// ```text
/// Uninitialized ──[connect/accept]──> Negotiating ──[params]──> Ready
///       │                                  │                       │
///       └──────────────────────────────────┴──[error]──> Closed <──┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Uninitialized,
    Negotiating,
    Ready,
    Error,
    Closed,
}

impl ConnectionState {
    /// Check if the connection is in a state that allows data transfer.
    pub fn can_transfer(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Check if the connection is established (negotiating or ready).
    pub fn is_established(&self) -> bool {
        matches!(self, Self::Negotiating | Self::Ready)
    }

    /// Check if the connection is closed or in error.
    pub fn is_terminated(&self) -> bool {
        matches!(self, Self::Error | Self::Closed)
    }
}
