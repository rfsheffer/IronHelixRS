//! The score (`VWSC`): what's on stage in each frame.
//!
//! Director 3 layout (per ScummVM's Director engine, checked against Iron
//! Helix): a u32 total size (including itself), then frames. Each frame is a
//! u16 byte size (including itself) followed by patches to a persistent
//! 512-byte channel buffer: u8 length in words, u8 offset in words, then the
//! bytes. Anything a frame doesn't patch carries over from the frame before.
//!
//! The buffer holds 32 bytes of main channels (frame script, tempo,
//! transition, sounds, palette) and 30 sprite channels of 16 bytes:
//!
//! ```text
//! +0 script   +1 type   +2 fore color   +3 back color   +4 thickness
//! +5 ink (low 6 bits; 0x40 trails, 0x80 stretch)   +6 u16 cast member
//! +8 i16 loc v   +10 i16 loc h   +12 i16 height   +14 i16 width
//! ```
//!
//! A sprite's loc is where its cast member's registration point goes.
//! Script numbers refer to score scripts (`VWAC`) with id = number + 1.

use crate::{DirectorError, Result};

pub const SPRITE_CHANNELS: usize = 30;
const MAIN_SIZE: usize = 32;
const SPRITE_SIZE: usize = 16;
const BUFFER: usize = MAIN_SIZE + SPRITE_CHANNELS * SPRITE_SIZE;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sprite {
    pub script: u8,
    /// 0 = empty channel; 1 = bitmap; other values for shapes, text, etc.
    pub kind: u8,
    pub ink: u8,
    pub cast: u16,
    pub loc_h: i16,
    pub loc_v: i16,
    pub width: i16,
    pub height: i16,
}

impl Sprite {
    pub fn is_empty(&self) -> bool {
        self.kind == 0 && self.cast == 0
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame script number (refers to the score script with id + 1).
    pub script: u8,
    pub tempo: u8,
    /// Channels 1-30 at indices 0-29.
    pub sprites: [Sprite; SPRITE_CHANNELS],
}

impl Frame {
    /// Sprite channel `n` (1-based, as Lingo numbers them).
    pub fn sprite(&self, n: usize) -> Option<&Sprite> {
        n.checked_sub(1).and_then(|i| self.sprites.get(i))
    }

    fn from_buffer(b: &[u8; BUFFER]) -> Self {
        let i16_at = |o: usize| i16::from_be_bytes([b[o], b[o + 1]]);
        let sprites = std::array::from_fn(|i| {
            let o = MAIN_SIZE + i * SPRITE_SIZE;
            Sprite {
                script: b[o],
                kind: b[o + 1],
                ink: b[o + 5] & 0x3F,
                cast: u16::from_be_bytes([b[o + 6], b[o + 7]]),
                loc_v: i16_at(o + 8),
                loc_h: i16_at(o + 10),
                height: i16_at(o + 12),
                width: i16_at(o + 14),
            }
        });
        Self { script: b[0], tempo: b[4], sprites }
    }
}

/// Parse a `VWSC` resource into frames (frame 1 at index 0).
pub fn parse(d: &[u8]) -> Result<Vec<Frame>> {
    let err = || DirectorError::Truncated("VWSC");
    let total = u32::from_be_bytes(d.get(0..4).ok_or_else(err)?.try_into().unwrap()) as usize;
    let end_of_score = total.min(d.len());
    let mut buffer = [0u8; BUFFER];
    let mut frames = Vec::new();
    let mut p = 4;
    while p + 2 <= end_of_score {
        let size = u16::from_be_bytes([d[p], d[p + 1]]) as usize;
        if size < 2 {
            break;
        }
        let end = (p + size).min(end_of_score);
        let mut q = p + 2;
        while q + 2 <= end {
            let len = d[q] as usize * 2;
            let offset = d[q + 1] as usize * 2;
            let patch = d.get(q + 2..q + 2 + len).ok_or_else(err)?;
            let dest = buffer.get_mut(offset..offset + len).ok_or(DirectorError::Truncated("VWSC channel"))?;
            dest.copy_from_slice(patch);
            q += 2 + len;
        }
        frames.push(Frame::from_buffer(&buffer));
        p = end;
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_carry_over() {
        // Frame 1 sets channel 1's cast (offset 32 + 6 = 38 = word 19) to 7.
        // Frame 2 changes only its loc h (offset 42 = word 21) to 100.
        let mut d = vec![0, 0, 0, 0];
        d.extend_from_slice(&[0, 6, 1, 19, 0, 7]);
        d.extend_from_slice(&[0, 6, 1, 21, 0, 100]);
        let len = d.len() as u32;
        d[0..4].copy_from_slice(&len.to_be_bytes());
        let frames = parse(&d).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].sprite(1).unwrap().cast, 7);
        assert_eq!(frames[1].sprite(1).unwrap().cast, 7);
        assert_eq!(frames[1].sprite(1).unwrap().loc_h, 100);
    }
}
