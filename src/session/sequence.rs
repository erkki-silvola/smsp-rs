/// Sequence number tracker for 4-bit sequence numbers (0-15).
#[derive(Debug, Clone)]
pub struct SequenceTracker {
    current: u8,
}

impl SequenceTracker {
    const MAX_SEQ: u8 = 15;

    pub fn new() -> Self {
        Self { current: 0 }
    }

    pub fn current(&self) -> u8 {
        self.current
    }

    pub fn next_seq(&mut self) -> u8 {
        let seq = self.current;
        self.advance();
        seq
    }

    pub fn advance(&mut self) {
        self.current = (self.current + 1) & Self::MAX_SEQ;
    }

    pub fn reset(&mut self) {
        self.current = 0;
    }

    pub fn is_expected(&self, seq: u8) -> bool {
        seq == self.current
    }

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
    fn test_sequence_advance_and_wraparound() {
        let mut seq = SequenceTracker::new();
        for i in 0..16 {
            assert_eq!(seq.next_seq(), i % 16);
        }
        assert_eq!(seq.current(), 0);
    }

    #[test]
    fn test_validate_and_advance() {
        let mut seq = SequenceTracker::new();
        assert!(seq.validate_and_advance(0));
        assert_eq!(seq.current(), 1);
        assert!(!seq.validate_and_advance(0));
        assert_eq!(seq.current(), 1);
        assert!(seq.validate_and_advance(1));
        assert_eq!(seq.current(), 2);
    }

    #[test]
    fn test_reset() {
        let mut seq = SequenceTracker::new();
        seq.advance();
        seq.advance();
        seq.reset();
        assert_eq!(seq.current(), 0);
    }
}
