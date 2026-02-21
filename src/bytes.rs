//! Byte slice utilities for binary format parsing

/// Read integers from byte slices in little-endian format
pub trait ReadIntLe {
    /// Read a u16 in little-endian format at the given offset
    fn read_u16_le(&self, offset: usize) -> u16;

    /// Read a u32 in little-endian format at the given offset
    fn read_u32_le(&self, offset: usize) -> u32;
}

impl ReadIntLe for [u8] {
    fn read_u16_le(&self, offset: usize) -> u16 {
        u16::from_le_bytes(self[offset..offset + 2].try_into().unwrap())
    }

    fn read_u32_le(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self[offset..offset + 4].try_into().unwrap())
    }
}
