/// Sequence number tracker for 4-bit sequence numbers (0-15)
#[derive(Debug, Clone)]
pub struct SequenceTracker {
    /// Current sequence number
    current: u8,
}

impl SequenceTracker {
    /// Maximum sequence number (4 bits)
    const MAX_SEQ: u8 = 15;

    /// Create a new sequence tracker starting at 0
    pub fn new() -> Self {
        SequenceTracker { current: 0 }
    }

    /// Get the current sequence number
    pub fn current(&self) -> u8 {
        self.current
    }

    /// Get the next sequence number and advance
    pub fn next_seq(&mut self) -> u8 {
        let seq = self.current;
        self.advance();
        seq
    }

    /// Advance to the next sequence number with wraparound
    pub fn advance(&mut self) {
        self.current = (self.current + 1) & Self::MAX_SEQ;
    }

    /// Reset sequence to 0
    pub fn reset(&mut self) {
        self.current = 0;
    }

    /// Check if a sequence number is expected
    pub fn is_expected(&self, seq: u8) -> bool {
        seq == self.current
    }

    /// Validate and advance if sequence matches
    pub fn validate_and_advance(&mut self, seq: u8) -> bool {
        if self.is_expected(seq) {
            self.advance();
            true
        } else {
            false
        }
    }
}

impl Default for SequenceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequence_advance() {
        let mut seq = SequenceTracker::new();
        assert_eq!(seq.current(), 0);

        for i in 0..16 {
            assert_eq!(seq.next_seq(), i % 16);
        }

        assert_eq!(seq.current(), 0); // Wrapped around
    }

    #[test]
    fn test_sequence_validation() {
        let mut seq = SequenceTracker::new();
        
        assert!(seq.validate_and_advance(0));
        assert_eq!(seq.current(), 1);
        
        assert!(!seq.validate_and_advance(0)); // Wrong sequence
        assert_eq!(seq.current(), 1); // Didn't advance
        
        assert!(seq.validate_and_advance(1));
        assert_eq!(seq.current(), 2);
    }

    #[test]
    fn test_sequence_wraparound() {
        let mut seq = SequenceTracker::new();
        
        // Advance to 15
        for _ in 0..15 {
            seq.advance();
        }
        assert_eq!(seq.current(), 15);
        
        // Should wrap to 0
        seq.advance();
        assert_eq!(seq.current(), 0);
    }

    #[test]
    fn test_reset() {
        let mut seq = SequenceTracker::new();
        seq.advance();
        seq.advance();
        assert_eq!(seq.current(), 2);
        
        seq.reset();
        assert_eq!(seq.current(), 0);
    }
}
