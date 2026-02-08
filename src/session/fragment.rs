use bytes::{Bytes, BytesMut, BufMut};
use crate::error::Result;

/// Fragment manager for splitting and assembling messages
#[derive(Debug)]
pub struct FragmentManager {
    /// Maximum size of a single fragment
    max_fragment_size: usize,
    /// Assembly buffer for incoming fragments
    assembly_buffer: BytesMut,
}

impl FragmentManager {
    /// Create a new fragment manager
    pub fn new(max_fragment_size: usize) -> Self {
        FragmentManager {
            max_fragment_size,
            assembly_buffer: BytesMut::new(),
        }
    }

    /// Split data into fragments
    pub fn split(&self, data: &[u8]) -> Vec<Bytes> {
        if data.is_empty() {
            return vec![Bytes::new()];
        }

        let mut fragments = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let end = (offset + self.max_fragment_size).min(data.len());
            fragments.push(Bytes::copy_from_slice(&data[offset..end]));
            offset = end;
        }

        fragments
    }

    /// Add a fragment to the assembly buffer
    pub fn add_fragment(&mut self, fragment: Bytes) -> Result<()> {
        self.assembly_buffer.put(fragment);
        Ok(())
    }

    /// Finalize and get the assembled message
    pub fn finalize(&mut self) -> Bytes {
        let buffer = self.assembly_buffer.split();
        buffer.freeze()
    }

    /// Clear the assembly buffer (e.g., on error)
    pub fn clear(&mut self) {
        self.assembly_buffer.clear();
    }

    /// Get current assembly buffer size
    pub fn current_size(&self) -> usize {
        self.assembly_buffer.len()
    }

    /// Check if assembly buffer is empty
    pub fn is_empty(&self) -> bool {
        self.assembly_buffer.is_empty()
    }

    /// Get maximum fragment size
    pub fn max_fragment_size(&self) -> usize {
        self.max_fragment_size
    }

    /// Calculate number of fragments needed for data
    pub fn fragment_count(&self, data_len: usize) -> usize {
        if data_len == 0 {
            return 1;
        }
        data_len.div_ceil(self.max_fragment_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_fragments() {
        let fm = FragmentManager::new(10);
        let data = b"Hello, World! This is a test.";
        
        let fragments = fm.split(data);
        assert_eq!(fragments.len(), 3);
        assert_eq!(fragments[0].len(), 10);
        assert_eq!(fragments[1].len(), 10);
        assert_eq!(fragments[2].len(), 9); // Last fragment is smaller
    }

    #[test]
    fn test_split_exact_size() {
        let fm = FragmentManager::new(5);
        let data = b"Hello";
        
        let fragments = fm.split(data);
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].len(), 5);
    }

    #[test]
    fn test_split_empty() {
        let fm = FragmentManager::new(10);
        let data = b"";
        
        let fragments = fm.split(data);
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].len(), 0);
    }

    #[test]
    fn test_assemble_fragments() {
        let mut fm = FragmentManager::new(10);
        
        fm.add_fragment(Bytes::from_static(b"Hello, ")).unwrap();
        fm.add_fragment(Bytes::from_static(b"World!")).unwrap();
        
        let assembled = fm.finalize();
        assert_eq!(assembled, Bytes::from_static(b"Hello, World!"));
        assert!(fm.is_empty());
    }

    #[test]
    fn test_clear() {
        let mut fm = FragmentManager::new(10);
        
        fm.add_fragment(Bytes::from_static(b"test")).unwrap();
        assert_eq!(fm.current_size(), 4);
        
        fm.clear();
        assert_eq!(fm.current_size(), 0);
        assert!(fm.is_empty());
    }

    #[test]
    fn test_fragment_count() {
        let fm = FragmentManager::new(10);
        
        assert_eq!(fm.fragment_count(0), 1);
        assert_eq!(fm.fragment_count(5), 1);
        assert_eq!(fm.fragment_count(10), 1);
        assert_eq!(fm.fragment_count(11), 2);
        assert_eq!(fm.fragment_count(20), 2);
        assert_eq!(fm.fragment_count(21), 3);
    }

    #[test]
    fn test_roundtrip() {
        let fm = FragmentManager::new(100);
        let original = b"The quick brown fox jumps over the lazy dog. ".repeat(10);
        
        let fragments = fm.split(&original);
        
        let mut assembler = FragmentManager::new(100);
        for fragment in fragments {
            assembler.add_fragment(fragment).unwrap();
        }
        
        let assembled = assembler.finalize();
        assert_eq!(assembled.as_ref(), original.as_slice());
    }
}
