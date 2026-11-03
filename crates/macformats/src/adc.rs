//! Apple Data Compression (ADC), used by DiskCopy 6 images.
//!
//! A simple LZ77 variant. Each opcode byte is one of:
//!
//! | byte        | meaning                                                          |
//! |-------------|------------------------------------------------------------------|
//! | `1xxxxxxx`  | literal: copy the next `x + 1` bytes                             |
//! | `01xxxxxx`  | copy `x + 4` bytes from distance `next u16 + 1` back             |
//! | `00xxxxdd`  | copy `x + 3` bytes from distance `(dd << 8 | next byte) + 1` back |

use crate::{MacError, Result};

/// Decompress `src`, appending exactly `out_len` bytes to `out`.
pub fn decompress(src: &[u8], out_len: usize, out: &mut Vec<u8>) -> Result<()> {
    let start = out.len();
    let end = start + out_len;
    let mut i = 0;
    let err = |what: &str| MacError::Invalid(format!("ADC: {what}"));

    while out.len() < end {
        let op = *src.get(i).ok_or_else(|| err("ran out of input"))?;
        let (len, dist) = if op & 0x80 != 0 {
            let n = (op & 0x7F) as usize + 1;
            let lit = src.get(i + 1..i + 1 + n).ok_or_else(|| err("literal past end"))?;
            out.extend_from_slice(lit);
            i += 1 + n;
            continue;
        } else if op & 0x40 != 0 {
            let hi = *src.get(i + 1).ok_or_else(|| err("truncated"))? as usize;
            let lo = *src.get(i + 2).ok_or_else(|| err("truncated"))? as usize;
            i += 3;
            ((op & 0x3F) as usize + 4, ((hi << 8) | lo) + 1)
        } else {
            let lo = *src.get(i + 1).ok_or_else(|| err("truncated"))? as usize;
            i += 2;
            (((op >> 2) & 0x0F) as usize + 3, ((((op & 3) as usize) << 8) | lo) + 1)
        };

        if dist > out.len() - start {
            return Err(err("back-reference before start of chunk"));
        }
        // Byte by byte: the source may overlap what we're writing.
        for _ in 0..len {
            let b = out[out.len() - dist];
            out.push(b);
        }
    }
    out.truncate(end);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_then_overlapping_copy() {
        // 3 literals "abc", then copy 5 bytes from 3 back (overlapping).
        let src = [0x82, b'a', b'b', b'c', 0b0000_1000, 0x02];
        let mut out = Vec::new();
        decompress(&src, 8, &mut out).unwrap();
        assert_eq!(out, b"abcabcab");
    }
}
