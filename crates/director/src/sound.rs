//! Sound members: Mac `snd ` resources.
//!
//! A `snd ` resource is a list of sound commands. Format 1 starts with
//! (u16 1, u16 modifier count, 6 bytes per modifier); format 2 with (u16 2,
//! u16 reference count). Then u16 command count and 8-byte commands
//! (u16 command, u16 param1, u32 param2). A `bufferCmd`/`soundCmd` with the
//! data-offset flag (0x8051/0x8050) points param2 at a sampled sound header:
//!
//! ```text
//! u32 sample pointer (0), u32 length in bytes, u32 rate (16.16 fixed),
//! u32 loop start, u32 loop end, u8 encoding (0 = plain), u8 base note,
//! then the samples: 8-bit unsigned mono
//! ```

use crate::{DirectorError, Result};

/// 8-bit unsigned mono samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampledSound {
    pub rate: u32,
    pub samples: Vec<u8>,
}

fn be16(d: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(d.get(o..o + 2).ok_or(DirectorError::Truncated("snd "))?.try_into().unwrap()))
}

fn be32(d: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(d.get(o..o + 4).ok_or(DirectorError::Truncated("snd "))?.try_into().unwrap()))
}

pub fn parse(d: &[u8]) -> Result<SampledSound> {
    let commands = match be16(d, 0)? {
        1 => 4 + 6 * be16(d, 2)? as usize,
        2 => 4,
        f => return Err(DirectorError::Unsupported(format!("snd format {f}"))),
    };
    let count = be16(d, commands)? as usize;
    for i in 0..count {
        let c = commands + 2 + 8 * i;
        if matches!(be16(d, c)?, 0x8050 | 0x8051) {
            let h = be32(d, c + 4)? as usize;
            let len = be32(d, h + 4)? as usize;
            let rate = (be32(d, h + 8)? as u64 + 0x8000) >> 16;
            let encoding = *d.get(h + 20).ok_or(DirectorError::Truncated("snd "))?;
            if encoding != 0 {
                return Err(DirectorError::Unsupported(format!("snd encoding {encoding:#x}")));
            }
            let samples = d.get(h + 22..h + 22 + len).ok_or(DirectorError::Truncated("snd "))?.to_vec();
            return Ok(SampledSound { rate: rate as u32, samples });
        }
    }
    Err(DirectorError::Unsupported("snd without sampled sound".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_2() {
        let mut d = vec![0, 2, 0, 0, 0, 1, 0x80, 0x51, 0, 0, 0, 0, 0, 14];
        d.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 3]); // pointer, length 3
        d.extend_from_slice(&0x56EE_8BA4u32.to_be_bytes()); // 22254.545 Hz
        d.extend_from_slice(&[0; 8]);
        d.extend_from_slice(&[0, 0x3C, 0x80, 0xFF, 0x00]);
        assert_eq!(parse(&d).unwrap(), SampledSound { rate: 22255, samples: vec![0x80, 0xFF, 0x00] });
    }
}
