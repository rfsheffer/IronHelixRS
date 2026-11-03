//! The CAVF "comprehensive RLE" bitmap compression.
//!
//! Pixels are 8-bit palette indices written left to right, scan line by scan
//! line, into the bitmap's area of change. The stream is made of byte-sized
//! descriptors:
//!
//! | descriptor `N` | meaning                                                   |
//! |----------------|-----------------------------------------------------------|
//! | `+N`           | N literal bytes follow                                    |
//! | `0`            | generic escape, see [`decode_escape`]                     |
//! | `-1 ab ...`    | `ab > 0`: next word repeated ab times; `ab < 0`: next long repeated -ab times |
//! | `-2 ab`        | `ab != 0`: skip ab pixels; `ab == 0`: the rest of this line is compact LUT encoded |
//! | `-N` (N > 2)   | repeat the following byte N times                         |
//!
//! "Compact" (LUT encoded) lines pack pixels as 4-bit indices into a 16-entry
//! lookup table of palette indices. The table persists between lines *and*
//! between frames: each LUT line starts with a 16-bit mask saying which
//! entries are replaced, followed by one byte per replaced entry.

use crate::error::{PacoError, Result};

/// Where decoded pixels go.
pub struct Target<'a> {
    /// The full movie canvas (one palette index per pixel).
    pub canvas: &'a mut [u8],
    /// Width of the canvas in pixels.
    pub stride: usize,
    /// The bitmap's area of change within the canvas.
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
    /// Maps the n-th scan line in the stream to a row within the area. This
    /// is how interlaced bitmaps get their rows put back in order.
    pub row_map: &'a [usize],
}

/// Counters describing how a bitmap decoded. Useful for spotting decoder bugs.
#[derive(Debug, Default, Clone)]
pub struct RleStats {
    pub bytes_consumed: usize,
    pub pixels_written: usize,
    pub pixels_skipped: usize,
    /// Cursor position when END OF BITMAP was read.
    pub end_row: usize,
    pub end_x: usize,
    pub lut_lines: usize,
    /// "Go to next line" escapes that arrived right after a line had wrapped
    /// (each still moves down a line).
    pub newline_after_wrap: usize,
    /// Pixels written to the phantom line just below the area (see
    /// [`Cursor::put`]); these are discarded.
    pub phantom_pixels: usize,
}

/// Write cursor over the area of change.
struct Cursor<'t, 'a> {
    t: &'t mut Target<'a>,
    x: usize,
    row: usize,
    /// True when the last pixel operation wrapped onto a new line.
    wrapped: bool,
    stats: RleStats,
}

impl<'t, 'a> Cursor<'t, 'a> {
    fn new(t: &'t mut Target<'a>) -> Self {
        Self {
            t,
            x: 0,
            row: 0,
            wrapped: false,
            stats: RleStats::default(),
        }
    }

    fn advance(&mut self) {
        self.x += 1;
        self.wrapped = false;
        if self.x == self.t.width {
            self.x = 0;
            self.row += 1;
            self.wrapped = true;
        }
    }

    /// Write one pixel and advance.
    ///
    /// The packer sometimes encodes scan lines past the bottom of the area,
    /// built from whatever was in memory after its buffer (sometimes
    /// recognizable 68k machine code). Usually that's a single "next line"
    /// escape, but occasionally it holds literal pixels. Anything below the
    /// area is counted and dropped.
    fn put(&mut self, value: u8) {
        if self.row < self.t.height {
            let y = self.t.top + self.t.row_map[self.row];
            let i = y * self.t.stride + self.t.left + self.x;
            self.t.canvas[i] = value;
            self.stats.pixels_written += 1;
        } else {
            self.stats.phantom_pixels += 1;
        }
        self.advance();
    }

    fn put_run(&mut self, pattern: &[u8], times: usize) {
        for _ in 0..times {
            for &v in pattern {
                self.put(v);
            }
        }
    }

    fn skip(&mut self, n: usize) {
        for _ in 0..n {
            if self.row < self.t.height {
                self.stats.pixels_skipped += 1;
            } else {
                self.stats.phantom_pixels += 1;
            }
            self.advance();
        }
    }

    /// Move to the start of the next scan line. A newline right after a line
    /// that ended exactly at the area's right edge still moves down: the
    /// packer means an unchanged line there. (Treating it as a no-op shifted
    /// every later line of the field, which showed as streaks through text in
    /// interlaced movies, and made the Mac death clips differ from the PC's.)
    fn next_line(&mut self) {
        if self.x == 0 && self.wrapped {
            self.stats.newline_after_wrap += 1;
        }
        self.x = 0;
        self.row += 1;
        self.wrapped = false;
    }

    /// Finish the current line, then skip `n` more whole lines.
    fn skip_lines(&mut self, n: usize) {
        self.next_line();
        self.row += n;
    }

    fn rows_left_in_area(&self) -> usize {
        self.t.height.saturating_sub(self.row)
    }
}

/// Byte reader that reports RLE-specific errors.
struct Bytes<'d> {
    data: &'d [u8],
    pos: usize,
}

impl Bytes<'_> {
    fn u8(&mut self) -> Result<u8> {
        let v = *self.data.get(self.pos).ok_or_else(|| PacoError::Rle {
            offset: self.pos,
            message: "ran out of data before END OF BITMAP".into(),
        })?;
        self.pos += 1;
        Ok(v)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0; N];
        for b in &mut out {
            *b = self.u8()?;
        }
        Ok(out)
    }

    /// The 12-bit count packed into an escape: low nibble of `first` + next byte.
    fn count12(&mut self, first: u8) -> Result<usize> {
        Ok((((first & 0x0F) as usize) << 8) | self.u8()? as usize)
    }
}

/// Decode one RLE bitmap into `target`. `lut` is the compact lookup table,
/// which the caller must keep between bitmaps.
pub fn decode(data: &[u8], target: &mut Target, lut: &mut [u8; 16]) -> Result<RleStats> {
    let mut cur = Cursor::new(target);
    let mut src = Bytes { data, pos: 0 };

    loop {
        let at = src.pos;
        let n = src.u8()? as i8;
        match n {
            1.. => {
                for _ in 0..n {
                    let v = src.u8()?;
                    cur.put(v);
                }
            }
            0 => {
                if decode_escape(&mut src, &mut cur, at)? {
                    break;
                }
            }
            -1 => {
                let ab = src.u8()? as i8;
                if ab > 0 {
                    let word: [u8; 2] = src.take()?;
                    cur.put_run(&word, ab as usize);
                } else if ab < 0 {
                    let long: [u8; 4] = src.take()?;
                    cur.put_run(&long, ab.unsigned_abs() as usize);
                }
            }
            -2 => {
                let ab = src.u8()?;
                if ab != 0 {
                    cur.skip(ab as usize);
                } else {
                    decode_lut_line(&mut src, &mut cur, lut, at)?;
                }
            }
            _ => {
                let v = src.u8()?;
                cur.put_run(&[v], n.unsigned_abs() as usize);
            }
        }
    }

    let mut stats = cur.stats.clone();
    stats.bytes_consumed = src.pos;
    stats.end_row = cur.row;
    stats.end_x = cur.x;
    Ok(stats)
}

/// Generic escape: `0 ab cd ...`. The high nibble of `ab` picks the action and
/// the low 12 bits of `abcd` are usually a count. Returns true at END OF BITMAP.
fn decode_escape(src: &mut Bytes, cur: &mut Cursor, at: usize) -> Result<bool> {
    let first = src.u8()?;
    match first >> 4 {
        0 => {
            let count = src.count12(first)?;
            for _ in 0..count {
                let v = src.u8()?;
                cur.put(v);
            }
        }
        1 => {
            let count = src.count12(first)?;
            let v = src.u8()?;
            cur.put_run(&[v], count);
        }
        2 => {
            let count = src.count12(first)?;
            let word: [u8; 2] = src.take()?;
            cur.put_run(&word, count);
        }
        3 => {
            let count = src.count12(first)?;
            let long: [u8; 4] = src.take()?;
            cur.put_run(&long, count);
        }
        4 => {
            let count = src.count12(first)?;
            cur.skip(count);
        }
        5 => {
            // Skip scan lines; 0 means just go to the next line.
            let count = src.count12(first)?;
            cur.skip_lines(count);
        }
        6 => {
            // Fill whole scan lines with a byte, word or long pattern.
            let lines = src.u8()? as usize;
            let pattern: Vec<u8> = match first & 0x0F {
                0 => src.take::<1>()?.to_vec(),
                1 => src.take::<2>()?.to_vec(),
                3 => src.take::<4>()?.to_vec(),
                other => {
                    return Err(PacoError::Rle {
                        offset: at,
                        message: format!("unknown fill-lines pattern size {other}"),
                    });
                }
            };
            if cur.x != 0 {
                cur.next_line();
            }
            let lines = lines.min(cur.rows_left_in_area());
            let pixels = lines * cur.t.width;
            for i in 0..pixels {
                cur.put(pattern[i % pattern.len()]);
            }
        }
        15 => return Ok(true),
        other => {
            return Err(PacoError::Rle {
                offset: at,
                message: format!("unknown escape code {other}"),
            });
        }
    }
    Ok(false)
}

/// Decode the rest of the current scan line in compact (4-bit LUT) form.
///
/// Each byte is `op << 4 | n`:
///
/// | op      | meaning                                                         |
/// |---------|-----------------------------------------------------------------|
/// | 0       | escape: `n` selects an action and the next byte is a count      |
/// | 1..=7   | `op` literal nibbles: `n` is the first, the rest follow packed  |
/// | 8..=13  | repeat nibble `n` (16 - op) times, i.e. 8 down to 3 times       |
/// | 14      | skip `n` pixels                                                 |
/// | 15      | `n < 8`: next byte (2 nibbles) repeated n times; else next 2 bytes (4 nibbles) repeated 16 - n times |
fn decode_lut_line(src: &mut Bytes, cur: &mut Cursor, lut: &mut [u8; 16], _at: usize) -> Result<()> {
    cur.stats.lut_lines += 1;

    // Which LUT entries change, bit 0 = entry 0, followed by the new values.
    let mask = u16::from_be_bytes(src.take()?);
    for (i, entry) in lut.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            *entry = src.u8()?;
        }
    }

    let start_row = cur.row;
    while cur.row == start_row {
        let at = src.pos;
        let byte = src.u8()?;
        let op = byte >> 4;
        let n = byte & 0x0F;
        match op {
            0 => {
                let count = src.u8()? as usize;
                match n {
                    0 => {
                        // `count` literal nibbles, packed two per byte.
                        let mut left = count;
                        while left > 0 {
                            let b = src.u8()?;
                            cur.put(lut[(b >> 4) as usize]);
                            left -= 1;
                            if left > 0 {
                                cur.put(lut[(b & 0x0F) as usize]);
                                left -= 1;
                            }
                        }
                    }
                    1 => {
                        let b = src.u8()?;
                        cur.put_run(&[lut[(b & 0x0F) as usize]], count);
                    }
                    2 => {
                        let b = src.u8()?;
                        let pair = [lut[(b >> 4) as usize], lut[(b & 0x0F) as usize]];
                        cur.put_run(&pair, count);
                    }
                    3 => {
                        let [b0, b1] = src.take()?;
                        let quad = [
                            lut[(b0 >> 4) as usize],
                            lut[(b0 & 0x0F) as usize],
                            lut[(b1 >> 4) as usize],
                            lut[(b1 & 0x0F) as usize],
                        ];
                        cur.put_run(&quad, count);
                    }
                    4 => cur.skip(count),
                    5 => cur.skip_lines(count),
                    other => {
                        return Err(PacoError::Rle {
                            offset: at,
                            message: format!("unknown compact escape {other}"),
                        });
                    }
                }
            }
            1..=7 => {
                cur.put(lut[n as usize]);
                let mut left = op as usize - 1;
                while left > 0 {
                    let b = src.u8()?;
                    cur.put(lut[(b >> 4) as usize]);
                    left -= 1;
                    if left > 0 {
                        cur.put(lut[(b & 0x0F) as usize]);
                        left -= 1;
                    }
                }
            }
            8..=13 => cur.put_run(&[lut[n as usize]], 16 - op as usize),
            14 => cur.skip(n as usize),
            _ => {
                if n < 8 {
                    let b = src.u8()?;
                    let pair = [lut[(b >> 4) as usize], lut[(b & 0x0F) as usize]];
                    cur.put_run(&pair, n as usize);
                } else {
                    let [b0, b1] = src.take()?;
                    let quad = [
                        lut[(b0 >> 4) as usize],
                        lut[(b0 & 0x0F) as usize],
                        lut[(b1 >> 4) as usize],
                        lut[(b1 & 0x0F) as usize],
                    ];
                    cur.put_run(&quad, 16 - n as usize);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_after_a_full_line_skips_a_line() {
        // A 2x3 area: two literals fill row 0, "next line" moves past row 1,
        // one literal lands on row 2, then END OF BITMAP.
        let data = [0x02, 7, 7, 0x00, 0x50, 0x00, 0x01, 9, 0x00, 0xF0];
        let mut canvas = vec![0u8; 6];
        let rows = [0, 1, 2];
        let mut target = Target { canvas: &mut canvas, stride: 2, left: 0, top: 0, width: 2, height: 3, row_map: &rows };
        let stats = decode(&data, &mut target, &mut [0; 16]).unwrap();
        assert_eq!(canvas, [7, 7, 0, 0, 9, 0]);
        assert_eq!(stats.newline_after_wrap, 1);
    }
}
