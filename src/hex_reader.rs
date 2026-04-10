//! Intel HEX file reader that builds a flat memory image from HEX records.
//!
//! Supports I32HEX format (Extended Linear Address records for 32-bit addressing),
//! which is standard for automotive ECU flash images.

use std::collections::BTreeMap;
use std::path::Path;

/// Memory image built from an Intel HEX file.
///
/// Stores contiguous data segments and provides address-based byte access.
/// Addresses are absolute 32-bit values (Extended Linear Address resolved).
#[derive(Debug, Clone)]
pub struct HexMemory {
    /// Segments keyed by start address, stored in a BTreeMap for ordered access.
    segments: BTreeMap<u32, Vec<u8>>,
}

/// Errors from HEX file loading.
#[derive(Debug, Clone, PartialEq)]
pub enum HexError {
    /// I/O error reading the file.
    Io(String),
    /// Parse error in a HEX record.
    Parse { line: usize, detail: String },
    /// Requested address range is not covered by HEX data.
    AddressNotFound { address: u32, length: usize },
}

impl std::fmt::Display for HexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HexError::Io(msg) => write!(f, "HEX I/O error: {msg}"),
            HexError::Parse { line, detail } => {
                write!(f, "HEX parse error at line {line}: {detail}")
            }
            HexError::AddressNotFound { address, length } => {
                write!(
                    f,
                    "address 0x{address:08X}..+{length} not found in HEX data"
                )
            }
        }
    }
}

impl std::error::Error for HexError {}

impl HexMemory {
    /// Load an Intel HEX file from disk.
    pub fn from_file(path: &Path) -> Result<Self, HexError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| HexError::Io(format!("{}: {}", path.display(), e)))?;
        Self::from_string(&content)
    }

    /// Parse an Intel HEX string into a memory image.
    pub fn from_string(content: &str) -> Result<Self, HexError> {
        let mut segments: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
        let mut extended_address: u32 = 0;

        let reader = ihex::Reader::new(content);
        for (line_num, result) in reader.enumerate() {
            let line_num = line_num + 1;
            let record = result.map_err(|e| HexError::Parse {
                line: line_num,
                detail: e.to_string(),
            })?;

            match record {
                ihex::Record::ExtendedLinearAddress(ela) => {
                    extended_address = (ela as u32) << 16;
                }
                ihex::Record::ExtendedSegmentAddress(esa) => {
                    extended_address = (esa as u32) << 4;
                }
                ihex::Record::Data { offset, value } => {
                    let abs_address = extended_address + offset as u32;
                    Self::merge_data(&mut segments, abs_address, &value);
                }
                ihex::Record::EndOfFile => break,
                // StartSegmentAddress and StartLinearAddress are execution entry points;
                // we don't need them for data extraction.
                _ => {}
            }
        }

        Ok(HexMemory { segments })
    }

    /// Merge data into the segment map, extending or creating segments as needed.
    fn merge_data(segments: &mut BTreeMap<u32, Vec<u8>>, address: u32, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        // Check if this data is contiguous with an existing segment
        // Look for a segment whose end matches our start address
        let end_address = address + data.len() as u32;

        // Try to append to an existing segment
        if let Some((&seg_start, seg_data)) = segments.range_mut(..=address).next_back() {
            let seg_end = seg_start + seg_data.len() as u32;
            if seg_end == address {
                // Contiguous: extend the existing segment
                seg_data.extend_from_slice(data);
                // Check if we can merge with the next segment
                if let Some((&next_start, _)) = segments.range(end_address..).next()
                    && next_start == end_address {
                        let next_data = segments.remove(&next_start).unwrap();
                        segments.get_mut(&seg_start).unwrap().extend(next_data);
                    }
                return;
            }
            if seg_end > address {
                // Overlapping: overwrite bytes within the existing segment
                let offset = (address - seg_start) as usize;
                let available = seg_data.len() - offset;
                if data.len() <= available {
                    seg_data[offset..offset + data.len()].copy_from_slice(data);
                } else {
                    // Extends past the segment end
                    seg_data.resize(offset + data.len(), 0xFF);
                    seg_data[offset..offset + data.len()].copy_from_slice(data);
                }
                return;
            }
        }

        // No existing segment to extend; create a new one
        // But first check if it's contiguous with the next segment
        if let Some((&next_start, _)) = segments.range(address..).next()
            && next_start == end_address {
                let next_data = segments.remove(&next_start).unwrap();
                let mut new_data = data.to_vec();
                new_data.extend(next_data);
                segments.insert(address, new_data);
                return;
            }

        segments.insert(address, data.to_vec());
    }

    /// Read `length` bytes starting at the given absolute address.
    /// Returns `Err` if the requested range is not fully covered.
    pub fn read_bytes(&self, address: u32, length: usize) -> Result<&[u8], HexError> {
        if length == 0 {
            // Return empty slice for zero-length reads
            return Ok(&[]);
        }

        // Find the segment containing this address
        if let Some((&seg_start, seg_data)) = self.segments.range(..=address).next_back() {
            let offset = (address - seg_start) as usize;
            if offset + length <= seg_data.len() {
                return Ok(&seg_data[offset..offset + length]);
            }
        }

        Err(HexError::AddressNotFound { address, length })
    }

    /// Read a single byte at the given address.
    pub fn read_u8(&self, address: u32) -> Result<u8, HexError> {
        self.read_bytes(address, 1).map(|b| b[0])
    }

    /// Read a little-endian u16 at the given address.
    pub fn read_u16_le(&self, address: u32) -> Result<u16, HexError> {
        let bytes = self.read_bytes(address, 2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Read a little-endian u32 at the given address.
    pub fn read_u32_le(&self, address: u32) -> Result<u32, HexError> {
        let bytes = self.read_bytes(address, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a little-endian f32 at the given address.
    pub fn read_f32_le(&self, address: u32) -> Result<f32, HexError> {
        self.read_u32_le(address).map(f32::from_bits)
    }

    /// Total number of bytes stored across all segments.
    pub fn total_bytes(&self) -> usize {
        self.segments.values().map(|s| s.len()).sum()
    }

    /// Number of separate memory segments.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Iterator over (start_address, data) for each segment.
    pub fn segments(&self) -> impl Iterator<Item = (u32, &[u8])> {
        self.segments.iter().map(|(&addr, data)| (addr, data.as_slice()))
    }

    /// The lowest address in the memory image, or None if empty.
    pub fn min_address(&self) -> Option<u32> {
        self.segments.keys().next().copied()
    }

    /// The highest address (exclusive) in the memory image, or None if empty.
    pub fn max_address(&self) -> Option<u32> {
        self.segments
            .iter()
            .next_back()
            .map(|(&addr, data)| addr + data.len() as u32)
    }

    /// Check if a given address range is fully covered by the memory image.
    pub fn contains(&self, address: u32, length: usize) -> bool {
        self.read_bytes(address, length).is_ok()
    }
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_hex() -> &'static str {
        concat!(
            ":02000004800476\n",
            ":0400000001020304F2\n",
            ":0400040005060708DE\n",
            ":00000001FF\n",
        )
    }

    #[test]
    fn parse_simple_hex() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        assert_eq!(mem.segment_count(), 1); // contiguous data
        assert_eq!(mem.total_bytes(), 8);
    }

    #[test]
    fn read_bytes_at_address() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        let bytes = mem.read_bytes(0x8004_0000, 4).unwrap();
        assert_eq!(bytes, &[0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn read_bytes_contiguous() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        // Read across the two original records (they should be merged)
        let bytes = mem.read_bytes(0x8004_0000, 8).unwrap();
        assert_eq!(bytes, &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn read_u8() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        assert_eq!(mem.read_u8(0x8004_0000).unwrap(), 0x01);
        assert_eq!(mem.read_u8(0x8004_0007).unwrap(), 0x08);
    }

    #[test]
    fn read_u16_le() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        assert_eq!(mem.read_u16_le(0x8004_0000).unwrap(), 0x0201);
    }

    #[test]
    fn read_u32_le() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        assert_eq!(mem.read_u32_le(0x8004_0000).unwrap(), 0x04030201);
    }

    #[test]
    fn read_f32_le() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        let val = mem.read_f32_le(0x8004_0000).unwrap();
        assert_eq!(val, f32::from_bits(0x04030201));
    }

    #[test]
    fn address_not_found() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        let result = mem.read_bytes(0x9000_0000, 1);
        assert!(matches!(result, Err(HexError::AddressNotFound { .. })));
    }

    #[test]
    fn read_past_segment_end() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        // Try to read 1 byte past the end of the segment
        let result = mem.read_bytes(0x8004_0000, 9);
        assert!(matches!(result, Err(HexError::AddressNotFound { .. })));
    }

    #[test]
    fn empty_hex() {
        let mem = HexMemory::from_string(":00000001FF\n").unwrap();
        assert_eq!(mem.total_bytes(), 0);
        assert_eq!(mem.segment_count(), 0);
        assert!(mem.min_address().is_none());
    }

    #[test]
    fn min_max_address() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        assert_eq!(mem.min_address(), Some(0x8004_0000));
        assert_eq!(mem.max_address(), Some(0x8004_0008));
    }

    #[test]
    fn contains_check() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        assert!(mem.contains(0x8004_0000, 8));
        assert!(!mem.contains(0x8004_0000, 9));
        assert!(!mem.contains(0x9000_0000, 1));
    }

    #[test]
    fn zero_length_read() {
        let mem = HexMemory::from_string(simple_hex()).unwrap();
        assert_eq!(mem.read_bytes(0x8004_0000, 0).unwrap(), &[] as &[u8]);
    }

    #[test]
    fn non_contiguous_segments() {
        let hex = concat!(
            ":02000004800476\n",
            ":020000001122CB\n",
            ":02001000334477\n",
            ":00000001FF\n",
        );
        let mem = HexMemory::from_string(hex).unwrap();
        assert_eq!(mem.segment_count(), 2);
        assert_eq!(mem.read_bytes(0x8004_0000, 2).unwrap(), &[0x11, 0x22]);
        assert_eq!(mem.read_bytes(0x8004_0010, 2).unwrap(), &[0x33, 0x44]);
        // Gap should not be readable
        assert!(mem.read_bytes(0x8004_0002, 1).is_err());
    }

    #[test]
    fn extended_segment_address() {
        let hex = concat!(
            ":020000021000EC\n",
            ":0200000055AAFF\n",
            ":00000001FF\n",
        );
        let mem = HexMemory::from_string(hex).unwrap();
        assert_eq!(mem.read_bytes(0x0001_0000, 2).unwrap(), &[0x55, 0xAA]);
    }

    #[test]
    fn parse_error_bad_checksum() {
        let hex = ":0400000001020304FF\n:00000001FF\n"; // bad checksum
        let result = HexMemory::from_string(hex);
        assert!(matches!(result, Err(HexError::Parse { .. })));
    }

    #[test]
    fn hex_error_display() {
        let e = HexError::AddressNotFound {
            address: 0x1234,
            length: 4,
        };
        assert_eq!(
            format!("{e}"),
            "address 0x00001234..+4 not found in HEX data"
        );
    }
}
