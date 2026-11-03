//! Picture members: QuickDraw `PICT` resources.
//!
//! Only what Director 3 stores for pasted artwork is handled: version 2
//! pictures holding 8-bit `PackBitsRect`/`PackBitsRgn` pixel maps with their
//! own color table. The picture starts with a u16 size and its frame rect;
//! then opcodes (u16 each, version 2), every one word-aligned.

use crate::bitmap::Rect;
use crate::{DirectorError, Result};

/// A decoded picture: RGBA over its frame (alpha 0 where nothing was drawn).
#[derive(Debug, Clone)]
pub struct Picture {
    /// The frame's top-left, in the picture's own (stage) coordinates.
    pub left: i32,
    pub top: i32,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        let b = self.d.get(self.p..self.p + n).ok_or(DirectorError::Truncated("PICT"))?;
        self.p += n;
        Ok(b)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn rect(&mut self) -> Result<Rect> {
        Rect::parse(self.bytes(8)?).ok_or(DirectorError::Truncated("PICT"))
    }
}

pub fn decode(data: &[u8]) -> Result<Picture> {
    let mut r = Reader { d: data, p: 2 };
    let frame = r.rect()?;
    let (width, height) = (frame.width(), frame.height());
    let mut rgba = vec![0u8; width * height * 4];
    if r.u16()? != 0x0011 || r.u16()? != 0x02FF {
        return Err(DirectorError::Unsupported("PICT version 1".into()));
    }
    loop {
        if r.p % 2 == 1 {
            r.p += 1;
        }
        match r.u16()? {
            0x0000 | 0x001E => {}
            0x00FF => break,
            0x0C00 => r.p += 24,
            0x0001 => {
                let size = r.u16()? as usize;
                r.p += size - 2;
            }
            0x00A0 => r.p += 2,
            0x00A1 => {
                r.p += 2;
                let size = r.u16()? as usize;
                r.p += size;
            }
            op @ (0x0098 | 0x0099) => pack_bits_rect(&mut r, op == 0x0099, &frame, width, &mut rgba)?,
            op => return Err(DirectorError::Unsupported(format!("PICT opcode {op:#06x}"))),
        }
    }
    Ok(Picture { left: frame.left as i32, top: frame.top as i32, width, height, rgba })
}

fn pack_bits_rect(r: &mut Reader, region: bool, frame: &Rect, width: usize, rgba: &mut [u8]) -> Result<()> {
    let row_bytes = (r.u16()? & 0x3FFF) as usize;
    let bounds = r.rect()?;
    r.p += 2 + 2 + 4 + 4 + 4 + 2; // version, pack type and size, resolution, pixel type
    let pixel_size = r.u16()?;
    r.p += 2 + 2 + 4 + 4 + 4; // components, plane bytes, table, reserved
    if pixel_size != 8 {
        return Err(DirectorError::Unsupported(format!("{pixel_size}-bit PICT")));
    }
    r.p += 4; // color table seed
    let device = r.u16()? & 0x8000 != 0;
    let colors = r.u16()? as usize + 1;
    let mut palette = [[0u8; 3]; 256];
    for i in 0..colors {
        let e = r.bytes(8)?;
        // Entries carry their index, except in device tables (in order).
        let index = if device { i } else { u16::from_be_bytes([e[0], e[1]]) as usize };
        palette[index.min(255)] = [e[2], e[4], e[6]];
    }
    let src = r.rect()?;
    let dst = r.rect()?;
    r.p += 2; // transfer mode
    if region {
        let size = r.u16()? as usize;
        r.p += size - 2;
    }

    let rows = bounds.height();
    let height = rgba.len() / 4 / width.max(1);
    for y in 0..rows {
        let row = if row_bytes < 8 {
            r.bytes(row_bytes)?.to_vec()
        } else {
            let count = if row_bytes > 250 { r.u16()? as usize } else { r.u8()? as usize };
            unpack_bits(r.bytes(count)?, row_bytes)
        };
        // Copy src (in bounds coordinates) to dst (in frame coordinates).
        let sy = y as i32 + bounds.top as i32;
        if sy < src.top as i32 || sy >= src.bottom as i32 {
            continue;
        }
        let dy = sy - src.top as i32 + dst.top as i32 - frame.top as i32;
        if dy < 0 || dy as usize >= height {
            continue;
        }
        for sx in src.left as i32..src.right as i32 {
            let bx = (sx - bounds.left as i32) as usize;
            let dx = sx - src.left as i32 + dst.left as i32 - frame.left as i32;
            let Some(&index) = row.get(bx) else { continue };
            if dx < 0 || dx as usize >= width {
                continue;
            }
            let o = (dy as usize * width + dx as usize) * 4;
            let [red, green, blue] = palette[index as usize];
            rgba[o..o + 4].copy_from_slice(&[red, green, blue, 255]);
        }
    }
    Ok(())
}

/// PackBits: n < 128 copies n + 1 bytes, n > 128 repeats the next byte 257 - n times.
fn unpack_bits(data: &[u8], expected: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0;
    while i < data.len() {
        let n = data[i] as usize;
        i += 1;
        if n < 128 {
            let end = (i + n + 1).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i = end;
        } else if n > 128 {
            let Some(&v) = data.get(i) else { break };
            out.resize(out.len() + 257 - n, v);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits() {
        assert_eq!(unpack_bits(&[0xFE, 7, 0x01, 1, 2, 0x80], 5), [7, 7, 7, 1, 2]);
    }

    #[test]
    fn tiny_picture() {
        // 2x1 frame at (0,0); a PackBitsRect with a 2-color table.
        let mut d = vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 0x00, 0x11, 0x02, 0xFF, 0x00, 0x98];
        d.extend_from_slice(&[0x80, 0x02, 0, 0, 0, 0, 0, 1, 0, 2]); // row bytes 2, bounds
        d.extend_from_slice(&[0; 18]);
        d.extend_from_slice(&[0, 8, 0, 1, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        d.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]); // seed, flags, 2 colors
        d.extend_from_slice(&[0, 0, 0xFF, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xFF, 0]);
        d.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 2, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0]); // src, dst, mode
        d.extend_from_slice(&[0, 1, 0x00, 0xFF]); // row (raw since row bytes < 8), end
        let p = decode(&d).unwrap();
        assert_eq!((p.width, p.height), (2, 1));
        assert_eq!(&p.rgba, &[0xFF, 0, 0, 255, 0, 0, 0xFF, 255]);
    }
}
