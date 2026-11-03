//! The Sega FILM container.
//!
//! ```text
//! 0x00  'FILM'
//! 0x04  u32  header length (sample data starts here)
//! 0x08  u32  version (ASCII like "1.09" on Saturn, zero on early Sega CD files)
//! 0x0C  u32  reserved
//! 0x10  'FDSC' chunk: u32 length (20 on Sega CD, 32 on Saturn), fourcc,
//!              u32 height, u32 width, [Saturn only: bpp and audio fields]
//! ....  'STAB' chunk: u32 length, u32 time base (ticks per second),
//!              u32 sample count, then 16-byte entries:
//!              u32 offset (from the start of sample data), u32 length,
//!              u32 info1, u32 info2
//! ```
//!
//! An entry whose info1 is `0xFFFFFFFF` is audio. For video, the top bit of
//! info1 is set for inter-coded frames, the low 31 bits are the timestamp in
//! ticks, and info2 is the frame's duration in ticks.

use crate::{FilmError, Result, be_u32, slice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    Audio,
    Video {
        timestamp: u32,
        duration: u32,
        keyframe: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Absolute offset within the file.
    pub offset: usize,
    pub len: usize,
    pub kind: SampleKind,
}

pub struct Film<'a> {
    data: &'a [u8],
    pub fourcc: [u8; 4],
    pub width: u32,
    pub height: u32,
    /// Ticks per second for sample timestamps and durations.
    pub time_base: u32,
    pub samples: Vec<Sample>,
}

impl<'a> Film<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.get(0..4) != Some(b"FILM") {
            return Err(FilmError::Invalid("not a FILM file".into()));
        }
        let header_len = be_u32(data, 0x04)? as usize;
        let version = be_u32(data, 0x08)?;
        if version == 0x0002_0000 {
            return Err(FilmError::Unsupported("interleaved (version 0x00020000) FILM files".into()));
        }

        if slice(data, 0x10, 4)? != b"FDSC" {
            return Err(FilmError::Invalid("missing FDSC chunk".into()));
        }
        let fdsc_len = be_u32(data, 0x14)? as usize;
        let fourcc = slice(data, 0x18, 4)?.try_into().unwrap();
        let height = be_u32(data, 0x1C)?;
        let width = be_u32(data, 0x20)?;

        let stab = 0x10 + fdsc_len;
        if slice(data, stab, 4)? != b"STAB" {
            return Err(FilmError::Invalid("missing STAB chunk".into()));
        }
        let time_base = be_u32(data, stab + 8)?;
        let count = be_u32(data, stab + 12)? as usize;

        let mut samples = Vec::with_capacity(count);
        for i in 0..count {
            let e = stab + 16 + i * 16;
            let offset = header_len + be_u32(data, e)? as usize;
            let len = be_u32(data, e + 4)? as usize;
            let info1 = be_u32(data, e + 8)?;
            let info2 = be_u32(data, e + 12)?;
            let kind = if info1 == 0xFFFF_FFFF {
                SampleKind::Audio
            } else {
                SampleKind::Video {
                    timestamp: info1 & 0x7FFF_FFFF,
                    duration: info2,
                    keyframe: info1 & 0x8000_0000 == 0,
                }
            };
            slice(data, offset, len)?; // bounds check up front
            samples.push(Sample { offset, len, kind });
        }

        Ok(Self {
            data,
            fourcc,
            width,
            height,
            time_base,
            samples,
        })
    }

    /// The whole file.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn sample_data(&self, s: &Sample) -> &'a [u8] {
        &self.data[s.offset..s.offset + s.len]
    }

    pub fn video(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter().filter(|s| matches!(s.kind, SampleKind::Video { .. }))
    }

    pub fn audio(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter().filter(|s| s.kind == SampleKind::Audio)
    }

    /// Duration of a sample in milliseconds (0 for audio).
    pub fn duration_ms(&self, s: &Sample) -> f64 {
        match s.kind {
            SampleKind::Video { duration, .. } if self.time_base > 0 => {
                duration as f64 * 1000.0 / self.time_base as f64
            }
            _ => 0.0,
        }
    }

    pub fn fourcc_str(&self) -> String {
        String::from_utf8_lossy(&self.fourcc).into_owned()
    }
}
