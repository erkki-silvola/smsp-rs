/// Connection state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection not yet established
    Uninitialized,
    /// Negotiating parameters
    Negotiating,
    /// Ready for data transfer
    Ready,
    /// Error state
    Error,
    /// Connection closed
    Closed,
}

impl ConnectionState {
    /// Check if the connection is in a state that allows data transfer
    pub fn can_transfer(&self) -> bool {
        matches!(self, ConnectionState::Ready)
    }

    /// Check if the connection is established (negotiating or ready)
    pub fn is_established(&self) -> bool {
        matches!(
            self,
            ConnectionState::Negotiating | ConnectionState::Ready
        )
    }

    /// Check if the connection is closed or in error
    pub fn is_terminated(&self) -> bool {
        matches!(self, ConnectionState::Error | ConnectionState::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_checks() {
        assert!(!ConnectionState::Uninitialized.can_transfer());
        assert!(!ConnectionState::Negotiating.can_transfer());
        assert!(ConnectionState::Ready.can_transfer());
        assert!(!ConnectionState::Error.can_transfer());
        assert!(!ConnectionState::Closed.can_transfer());

        assert!(!ConnectionState::Uninitialized.is_established());
        assert!(ConnectionState::Negotiating.is_established());
        assert!(ConnectionState::Ready.is_established());
        
        assert!(!ConnectionState::Ready.is_terminated());
        assert!(ConnectionState::Error.is_terminated());
        assert!(ConnectionState::Closed.is_terminated());
    }
}
