//! Bitmap cast members (`BITD`) and palettes (`CLUT`).
//!
//! Format as documented by ScummVM's Director engine, checked against
//! Iron Helix (all 274 of its bitmaps decode to exactly the expected size).
//!
//! The member's cast record (in `VWCR`) gives its size: after the cast type
//! byte come a flags byte, a u16 whose top bit says "bit depth and palette
//! follow", the initial rect (top, left, bottom, right), a bounding rect,
//! the registration point (v, h), then optionally the bit depth and a
//! palette id (0 = Mac system palette, negative = other built-in palettes,
//! positive = a palette cast member).
//!
//! The `BITD` data is raw if its size is exactly what the pixels need,
//! otherwise run-length encoded: a control byte `c < 0x80` copies the next
//! `c + 1` bytes, `c >= 0x80` repeats the next byte `257 - c` times. Rows are
//! padded to an even number of bytes (1-bit rows to 16 pixels).

use crate::{DirectorError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub top: i16,
    pub left: i16,
    pub bottom: i16,
    pub right: i16,
}

impl Rect {
    pub fn width(&self) -> usize {
        (self.right - self.left).max(0) as usize
    }

    pub fn height(&self) -> usize {
        (self.bottom - self.top).max(0) as usize
    }

    pub(crate) fn parse(d: &[u8]) -> Option<Self> {
        let v = |i: usize| -> Option<i16> { Some(i16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?])) };
        Some(Self { top: v(0)?, left: v(2)?, bottom: v(4)?, right: v(6)? })
    }
}

/// The parsed cast record of a bitmap member.
#[derive(Debug, Clone, Copy)]
pub struct BitmapInfo {
    pub rect: Rect,
    /// Registration point (relative to the stage, like `rect`).
    pub reg_h: i16,
    pub reg_v: i16,
    pub bits_per_pixel: u16,
    /// 0 = Mac system palette; negative = other built-ins; positive = a cast member.
    pub palette: i16,
}

impl BitmapInfo {
    /// `record` is the cast record after its size byte (starts with the type byte).
    pub fn parse(record: &[u8]) -> Result<Self> {
        let err = || DirectorError::Truncated("bitmap cast record");
        let flags_and_size = u16::from_be_bytes([*record.get(2).ok_or_else(err)?, *record.get(3).ok_or_else(err)?]);
        let rect = Rect::parse(record.get(4..12).ok_or_else(err)?).ok_or_else(err)?;
        let reg_v = i16::from_be_bytes([record[20], record[21]]);
        let reg_h = i16::from_be_bytes(record.get(22..24).ok_or_else(err)?.try_into().unwrap());
        let (bits_per_pixel, palette) = if flags_and_size & 0x8000 != 0 {
            let d = record.get(24..28).ok_or_else(err)?;
            (u16::from_be_bytes([d[0], d[1]]), i16::from_be_bytes([d[2], d[3]]))
        } else {
            (1, 0)
        };
        Ok(Self { rect, reg_h, reg_v, bits_per_pixel, palette })
    }

    /// Bytes per row as stored.
    pub fn pitch(&self) -> usize {
        let mut pixels = self.rect.width();
        if self.bits_per_pixel == 1 && !pixels.is_multiple_of(16) {
            pixels += 16 - pixels % 16;
        }
        let bytes = (pixels * self.bits_per_pixel as usize) / 8;
        bytes + bytes % 2
    }
}

/// A decoded bitmap: one palette index per pixel.
///
/// 1-bit bitmaps decode to index 0 (white) and 255 (black), which is how
/// the Mac system palette numbers those colors.
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub cast: u16,
    pub info: BitmapInfo,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

fn unpack_rle(data: &[u8], expected: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        i += 1;
        if c & 0x80 != 0 {
            let Some(&v) = data.get(i) else { break };
            i += 1;
            out.resize(out.len() + (257 - c as usize), v);
        } else {
            let n = c as usize + 1;
            let end = (i + n).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i = end;
        }
    }
    out
}

/// Decode a `BITD` resource using its member's cast record.
pub fn decode(cast: u16, info: BitmapInfo, data: &[u8]) -> Result<Bitmap> {
    let (w, h) = (info.rect.width(), info.rect.height());
    let pitch = info.pitch();
    let bpp = info.bits_per_pixel as usize;
    let unpadded = w * h * bpp / 8;

    // Raw if the size is exactly what's needed: padded rows for 1-bit,
    // unpadded for deeper bitmaps (as ScummVM decides for Director 3).
    let raw = if bpp == 1 { data.len() == pitch * h } else { data.len() == unpadded };
    let stored = if raw {
        data.to_vec()
    } else {
        unpack_rle(data, pitch * h)
    };

    let mut pixels = vec![0u8; w * h];
    match bpp {
        8 => {
            // Rows are padded to even length when the data has room for it.
            let stride = if h > 0 && stored.len() >= pitch * h { pitch } else { w };
            for y in 0..h {
                let row = stored.get(y * stride..y * stride + w).unwrap_or_default();
                pixels[y * w..y * w + row.len()].copy_from_slice(row);
            }
        }
        1 => {
            for y in 0..h {
                for x in 0..w {
                    let byte = stored.get(y * pitch + x / 8).copied().unwrap_or(0);
                    pixels[y * w + x] = if byte & (0x80 >> (x % 8)) != 0 { 255 } else { 0 };
                }
            }
        }
        other => {
            return Err(DirectorError::Unsupported(format!("{other}-bit bitmaps")));
        }
    }
    Ok(Bitmap { cast, info, width: w, height: h, pixels })
}

/// A palette stored as a standard Mac color table (`CLUT`/`clut`): an
/// 8-byte header (seed, flags, entry count - 1), then 8 bytes per entry
/// (value, red, green, blue as 16-bit; the high byte is the 8-bit color).
pub fn parse_clut(data: &[u8]) -> Result<Vec<[u8; 3]>> {
    let count = u16::from_be_bytes([
        *data.get(6).ok_or(DirectorError::Truncated("CLUT"))?,
        *data.get(7).ok_or(DirectorError::Truncated("CLUT"))?,
    ]) as usize
        + 1;
    (0..count)
        .map(|i| {
            let e = data.get(8 + i * 8..16 + i * 8).ok_or(DirectorError::Truncated("CLUT"))?;
            Ok([e[2], e[4], e[6]])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(w: i16, h: i16, bpp: u16) -> BitmapInfo {
        BitmapInfo { rect: Rect { top: 0, left: 0, bottom: h, right: w }, reg_h: 0, reg_v: 0, bits_per_pixel: bpp, palette: 0 }
    }

    #[test]
    fn rle_and_padding() {
        // 3x2 8-bit image, rows padded to 4 bytes: [1,1,1,pad],[2,3,4,pad]
        // RLE: repeat 1 three times (0xFE), literal [0,2,3,4,0] (0x04 + 5 bytes)
        let data = [0xFE, 1, 0x04, 0, 2, 3, 4, 0];
        let b = decode(1, info(3, 2, 8), &data).unwrap();
        assert_eq!(b.pixels, [1, 1, 1, 2, 3, 4]);
    }

    #[test]
    fn raw_and_one_bit() {
        // 16x1 1-bit, raw (2 bytes): first pixel black, rest white.
        let b = decode(1, info(16, 1, 1), &[0x80, 0x00]).unwrap();
        assert_eq!(b.pixels[0], 255);
        assert_eq!(b.pixels[1], 0);
    }

    #[test]
    fn color_table() {
        let mut d = vec![0, 0, 0, 0, 0, 0, 0, 1];
        d.extend_from_slice(&[0, 0, 0xFF, 0xFF, 0, 0, 0, 0]);
        d.extend_from_slice(&[0, 1, 0x12, 0x00, 0x34, 0x00, 0x56, 0x00]);
        assert_eq!(parse_clut(&d).unwrap(), vec![[0xFF, 0, 0], [0x12, 0x34, 0x56]]);
    }
}
