//! Readers for the classic Macintosh formats the Mac release of Iron Helix
//! comes wrapped in:
//!
//! - [`sit5`]: StuffIt 5 archives, including the "Arsenic" compression method
//! - [`rsrc`]: resource forks
//! - [`ndif`]: DiskCopy 6 (NDIF) disk images, including ADC compression
//! - [`hfs`]: the HFS file system inside the disk image
//!
//! Everything is big-endian, as on the 68k Macs these came from.

pub mod adc;
pub mod arsenic;
pub mod hfs;
pub mod macroman;
pub mod ndif;
pub mod rsrc;
pub mod sit5;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MacError {
    #[error("unexpected end of data at offset {offset}")]
    Eof { offset: usize },

    #[error("invalid data: {0}")]
    Invalid(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MacError>;

/// Big-endian field access on byte slices, with bounds checks.
pub(crate) trait BeBytes {
    fn be_u8(&self, at: usize) -> Result<u8>;
    fn be_u16(&self, at: usize) -> Result<u16>;
    fn be_u32(&self, at: usize) -> Result<u32>;
    fn be_slice(&self, at: usize, len: usize) -> Result<&[u8]>;
}

impl BeBytes for [u8] {
    fn be_u8(&self, at: usize) -> Result<u8> {
        self.get(at).copied().ok_or(MacError::Eof { offset: at })
    }

    fn be_u16(&self, at: usize) -> Result<u16> {
        let b = self.be_slice(at, 2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn be_u32(&self, at: usize) -> Result<u32> {
        let b = self.be_slice(at, 4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn be_slice(&self, at: usize, len: usize) -> Result<&[u8]> {
        self.get(at..at + len).ok_or(MacError::Eof { offset: at })
    }
}
