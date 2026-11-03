//! DiskCopy 6 "NDIF" disk images.
//!
//! The data fork is a sequence of chunks; the chunk map lives in the resource
//! fork as a `bcem` resource:
//!
//! ```text
//! 0    u16   version
//! 4    Str63 image name
//! 68   u32   sector count (512-byte sectors)
//! 124  u32   chunk count
//! 128  chunk[count], 12 bytes each:
//!        u24 first sector, u8 type, u32 data offset, u32 data length
//! ```
//!
//! A chunk covers sectors up to the next chunk's first sector. Types seen:
//! 0 = all zeros, 2 = stored, 0x83 = ADC compressed, 0xFF = end marker.

use crate::{BeBytes, MacError, Result, adc};

pub const SECTOR: usize = 512;

#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub first_sector: u32,
    pub kind: u8,
    pub offset: u32,
    pub len: u32,
}

#[derive(Debug, Clone)]
pub struct ChunkMap {
    pub name: String,
    pub sectors: u32,
    pub chunks: Vec<Chunk>,
}

impl ChunkMap {
    pub fn parse(bcem: &[u8]) -> Result<Self> {
        let name_len = bcem.be_u8(4)? as usize;
        let name = crate::macroman::decode(bcem.be_slice(5, name_len.min(63))?);
        let sectors = bcem.be_u32(68)?;
        let count = bcem.be_u32(124)? as usize;
        let chunks = (0..count)
            .map(|i| {
                let e = 128 + i * 12;
                Ok(Chunk {
                    first_sector: bcem.be_u32(e)? >> 8,
                    kind: bcem.be_u8(e + 3)?,
                    offset: bcem.be_u32(e + 4)?,
                    len: bcem.be_u32(e + 8)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            name,
            sectors,
            chunks,
        })
    }
}

/// Expand the whole image to raw sectors, calling `sink` with each chunk's
/// bytes in order (so callers can stream to disk).
pub fn expand(map: &ChunkMap, data_fork: &[u8], mut sink: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
    let mut buf = Vec::new();
    for (i, c) in map.chunks.iter().enumerate() {
        if c.kind == 0xFF {
            break;
        }
        let next = map.chunks.get(i + 1).map_or(map.sectors, |n| n.first_sector);
        let out_len = (next - c.first_sector) as usize * SECTOR;
        let src = data_fork.be_slice(c.offset as usize, c.len as usize)?;

        buf.clear();
        match c.kind {
            0 => buf.resize(out_len, 0),
            2 => {
                buf.extend_from_slice(src);
                buf.resize(out_len, 0);
            }
            0x83 => adc::decompress(src, out_len, &mut buf)?,
            k => {
                return Err(MacError::Unsupported(format!(
                    "NDIF chunk type {k:#x} at sector {}",
                    c.first_sector
                )));
            }
        }
        sink(&buf)?;
    }
    Ok(())
}
