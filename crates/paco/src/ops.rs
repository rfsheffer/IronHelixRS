//! Opcodes found inside chunks.
//!
//! Every opcode starts with a 32-bit header: the top 8 bits are the opcode
//! kind, the low 24 bits are the opcode's total length in bytes *including*
//! the 4-byte header itself. Opcodes are always padded to a multiple of 4.

use crate::error::{PacoError, Result};
use crate::header::bits;
use crate::palette::Rgb;
use crate::reader::Reader;

pub const OP_TRANSITION: u8 = 1;
pub const OP_PALETTE: u8 = 2;
pub const OP_DELAY: u8 = 3;
pub const OP_SOUND: u8 = 4;
pub const OP_CYCLE: u8 = 5;
pub const OP_FILL: u8 = 6;
pub const OP_INPUT: u8 = 7;
pub const OP_BITMAP: u8 = 8;
pub const OP_RECTCOPY: u8 = 9;
pub const OP_BREAK: u8 = 10;
pub const OP_END_OF_CHUNK: u8 = 11;

#[derive(Debug, Clone)]
pub enum Op {
    Transition(OpTransition),
    Palette(OpPalette),
    Delay(OpDelay),
    Sound(OpSound),
    Cycle { data: Vec<u8> },
    Fill { rgb: Rgb },
    Input { bits1: u16 },
    Bitmap(OpBitmap),
    RectCopy { data: Vec<u8> },
    Break { user_data: i32 },
    EndOfChunk,
    Unknown { kind: u8, data: Vec<u8> },
}

impl Op {
    /// Read one opcode, consuming exactly its declared length.
    pub fn parse(r: &mut Reader) -> Result<Self> {
        let start = r.position();
        let header = r.u32()?;
        let kind = (header >> 24) as u8;
        let len = (header & 0x00FF_FFFF) as usize;
        if len < 4 {
            return Err(PacoError::Invalid(format!(
                "opcode {kind} at offset {start} has impossible length {len}"
            )));
        }
        let body = r.bytes(len - 4)?;
        let mut b = Reader::new(body);

        let op = match kind {
            OP_TRANSITION => Op::Transition(OpTransition {
                kind: b.u8()?,
                bits1: b.u8()?,
                time: b.u8()?,
                chunk_size: b.u8()?,
                part_size: b.u8()?,
                custom: b.rest().to_vec(),
            }),
            OP_PALETTE => Op::Palette(OpPalette::parse(&mut b)?),
            OP_DELAY => Op::Delay(OpDelay { bits1: b.u16()? }),
            OP_SOUND => Op::Sound(OpSound {
                bits1: b.u16()?,
                defer_till_frame: b.i16()?,
                data: b.rest().to_vec(),
            }),
            OP_CYCLE => Op::Cycle { data: body.to_vec() },
            OP_FILL => Op::Fill {
                rgb: Rgb::new(b.u8()?, b.u8()?, b.u8()?),
            },
            OP_INPUT => Op::Input { bits1: b.u16()? },
            OP_BITMAP => Op::Bitmap(OpBitmap {
                left: b.u16()?,
                top: b.u16()?,
                width: b.u16()?,
                height: b.u16()?,
                bits1: b.u16()?,
                data: b.rest().to_vec(),
            }),
            OP_RECTCOPY => Op::RectCopy { data: body.to_vec() },
            OP_BREAK => Op::Break { user_data: b.i32()? },
            OP_END_OF_CHUNK => Op::EndOfChunk,
            _ => Op::Unknown {
                kind,
                data: body.to_vec(),
            },
        };
        Ok(op)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Op::Transition(_) => "TRANSITION",
            Op::Palette(_) => "PALETTE",
            Op::Delay(_) => "DELAY",
            Op::Sound(_) => "SOUND",
            Op::Cycle { .. } => "CYCLE",
            Op::Fill { .. } => "FILL",
            Op::Input { .. } => "INPUT",
            Op::Bitmap(_) => "BITMAP",
            Op::RectCopy { .. } => "RECTCOPY",
            Op::Break { .. } => "BREAK",
            Op::EndOfChunk => "END_OF_CHUNK",
            Op::Unknown { .. } => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpTransition {
    pub kind: u8,
    pub bits1: u8,
    /// In tenths of a second.
    pub time: u8,
    pub chunk_size: u8,
    pub part_size: u8,
    pub custom: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteType {
    /// The palette is stored in the opcode.
    Given,
    /// Use the system palette (the Mac 8-bit system CLUT).
    System,
    /// Fill the palette with a single solid color (used for fades).
    Solid,
    Other(u8),
}

#[derive(Debug, Clone)]
pub struct OpPalette {
    pub bits1: u8,
    /// Transition time; 0 means immediate.
    pub time: u8,
    pub solid: Rgb,
    /// 0 means a full 256-entry table follows (for `Given` palettes).
    pub num_colors: u8,
    pub colors: Vec<Rgb>,
}

impl OpPalette {
    fn parse(b: &mut Reader) -> Result<Self> {
        let bits1 = b.u8()?;
        let time = b.u8()?;
        let solid = Rgb::new(b.u8()?, b.u8()?, b.u8()?);
        let _filler0 = b.u8()?;
        let _filler = b.u8()?;
        let num_colors = b.u8()?;
        let mut colors = Vec::new();
        while b.remaining() >= 3 {
            colors.push(Rgb::new(b.u8()?, b.u8()?, b.u8()?));
        }
        Ok(Self {
            bits1,
            time,
            solid,
            num_colors,
            colors,
        })
    }

    pub fn palette_type(&self) -> PaletteType {
        match bits(self.bits1 as u32, 5, 7) {
            0 => PaletteType::Given,
            1 => PaletteType::System,
            2 => PaletteType::Solid,
            n => PaletteType::Other(n as u8),
        }
    }

    /// When and how to apply the change (fade timing).
    pub fn when(&self) -> u8 {
        bits(self.bits1 as u32, 2, 7) as u8
    }
}

#[derive(Debug, Clone)]
pub struct OpDelay {
    pub bits1: u16,
}

impl OpDelay {
    pub fn persist(&self) -> bool { bits(self.bits1 as u32, 15, 1) != 0 }
    pub fn frames_per_sec(&self) -> bool { bits(self.bits1 as u32, 12, 1) != 0 }
    pub fn rate(&self) -> u32 { bits(self.bits1 as u32, 0, 0xFFF) }

    pub fn frame_ms(&self) -> f64 {
        let rate = self.rate() as f64;
        if self.frames_per_sec() {
            if rate == 0.0 { 0.0 } else { 1000.0 / rate }
        } else {
            rate * 1000.0 / 60.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpSound {
    pub bits1: u16,
    pub defer_till_frame: i16,
    pub data: Vec<u8>,
}

impl OpSound {
    pub fn flush(&self) -> bool { bits(self.bits1 as u32, 15, 1) != 0 }
    pub fn rate(&self) -> u32 { bits(self.bits1 as u32, 10, 7) }

    /// Sample rate in Hz. Codes 0 and 2 (the Windows release's, and the Mac
    /// computer clips and video messages) match 11,025 Hz WAV files shipped
    /// alongside the movies; code 3 (the Mac phase movies) runs at twice
    /// that, judging by sample counts against running time.
    pub fn sample_rate_hz(&self) -> u32 {
        match self.rate() {
            3 => 22_050,
            _ => 11_025,
        }
    }
}

/// How a bitmap's pixel data is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Rle,
    RleInterlaced,
    Other(u8),
}

#[derive(Debug, Clone)]
pub struct OpBitmap {
    /// The area of change within the movie frame.
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub bits1: u16,
    pub data: Vec<u8>,
}

impl OpBitmap {
    pub fn black_and_white(&self) -> bool { bits(self.bits1 as u32, 15, 1) != 0 }
    /// The bitmap only holds changes and must be drawn over the previous frame.
    pub fn diffs_only(&self) -> bool { bits(self.bits1 as u32, 14, 1) != 0 }
    /// At least one scan line uses compact (4-bit LUT) encoding.
    pub fn lut_encoded(&self) -> bool { bits(self.bits1 as u32, 13, 1) != 0 }

    pub fn compression(&self) -> Compression {
        match bits(self.bits1 as u32, 8, 0xF) {
            0 => Compression::None,
            1 => Compression::Rle,
            2 => Compression::RleInterlaced,
            n => Compression::Other(n as u8),
        }
    }
}
