//! The CAVF file header and chunk table.

use crate::error::Result;
use crate::reader::Reader;

/// Extract `mask` bits starting at `shift` from a packed bitfield.
pub(crate) fn bits(value: u32, shift: u32, mask: u32) -> u32 {
    (value >> shift) & mask
}

/// Size of [`CavfHeader`] on disk, not counting the chunk table that follows.
pub const CAVF_HEADER_SIZE: usize = 38;

#[derive(Debug, Clone)]
pub struct CavfHeader {
    pub version_min: u8,
    pub version_maj: u8,
    pub header_size: i16,
    pub width: i16,
    pub height: i16,
    /// Positive: ticks (1/60 s) per frame. Negative: frames per second.
    pub default_delay: i16,
    pub bits1: u16,
    /// Size of the largest non-sound-only chunk.
    pub chunk_allocation: i32,
    pub coalesced_chunk_allocation: i32,
    /// Zero means the movie has no sound.
    pub sound_allocation: i32,
    /// Number of BITMAP opcodes in the movie.
    pub num_frames: i16,
    pub num_chunks: i16,
    pub bits4: u8,
    /// 1, 2, 4 or 8 (zero also means 8).
    pub bit_depth: u8,
    pub packer_version: i16,
    pub bits2: u16,
    pub bits3: u16,
    /// Zero means the packer finished, otherwise frames done before cancel.
    pub finished_frames: i16,
}

impl CavfHeader {
    pub fn parse(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            version_min: r.u8()?,
            version_maj: r.u8()?,
            header_size: r.i16()?,
            width: r.i16()?,
            height: r.i16()?,
            default_delay: r.i16()?,
            bits1: r.u16()?,
            chunk_allocation: r.i32()?,
            coalesced_chunk_allocation: r.i32()?,
            sound_allocation: r.i32()?,
            num_frames: r.i16()?,
            num_chunks: r.i16()?,
            bits4: r.u8()?,
            bit_depth: r.u8()?,
            packer_version: r.i16()?,
            bits2: r.u16()?,
            bits3: r.u16()?,
            finished_frames: r.i16()?,
        })
    }

    fn b1(&self, shift: u32, mask: u32) -> u32 {
        bits(self.bits1 as u32, shift, mask)
    }

    fn b2(&self, shift: u32, mask: u32) -> u32 {
        bits(self.bits2 as u32, shift, mask)
    }

    // bits1
    pub fn user_interrupt(&self) -> bool { self.b1(15, 1) != 0 }
    pub fn looping(&self) -> bool { self.b1(10, 1) != 0 }
    pub fn no_transition_ops(&self) -> bool { self.b1(9, 1) != 0 }
    pub fn maintain_framerate(&self) -> bool { self.b1(8, 1) != 0 }
    /// 0 = none, 1 = 2x, 2 = 4x, 3 = 8x
    pub fn pixel_rep_x(&self) -> u32 { self.b1(6, 3) }
    /// 0 = none, 1 = 2x, 2 = 3x, ...
    pub fn pixel_rep_y(&self) -> u32 { self.b1(0, 7) }

    // bits2
    pub fn interlaced(&self) -> bool { self.b2(15, 1) != 0 }
    pub fn indirect(&self) -> bool { self.b2(14, 1) != 0 }
    pub fn smaller_packing(&self) -> bool { self.b2(13, 1) != 0 }
    pub fn sound_sample_rate(&self) -> u32 { self.b2(11, 3) }

    /// Default frame duration in milliseconds.
    pub fn default_frame_ms(&self) -> f64 {
        delay_to_ms(self.default_delay)
    }
}

/// Convert a CAVF delay value to milliseconds.
pub fn delay_to_ms(delay: i16) -> f64 {
    match delay {
        d if d > 0 => d as f64 * 1000.0 / 60.0,
        d if d < 0 => 1000.0 / (-(d as f64)),
        _ => 0.0,
    }
}

/// One entry of the chunk table: flags plus a 24-bit length.
#[derive(Debug, Clone, Copy)]
pub struct ChunkHeader(pub u32);

impl ChunkHeader {
    pub fn sound_only(&self) -> bool { bits(self.0, 31, 1) != 0 }
    pub fn skippable(&self) -> bool { bits(self.0, 30, 1) != 0 }
    pub fn combinable(&self) -> bool { bits(self.0, 29, 1) != 0 }
    pub fn len(&self) -> usize { bits(self.0, 0, 0x00FF_FFFF) as usize }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
}
