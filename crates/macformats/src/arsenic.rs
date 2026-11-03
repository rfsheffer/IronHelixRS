//! StuffIt 5 "Arsenic" decompression (method 15).
//!
//! Arsenic is a bzip2-like pipeline:
//!
//! 1. an adaptive arithmetic coder produces symbols,
//! 2. which are move-to-front indices with runs of zeros coded specially,
//! 3. which undo into a Burrows-Wheeler-transformed block,
//! 4. whose inverse is finally run-length decoded (4 equal bytes + count).
//!
//! The stream ends with a CRC-32 of the decompressed data, which we check.

use crate::{MacError, Result};

const NUM_BITS: u32 = 26;
const ONE: u32 = 1 << (NUM_BITS - 1);
const HALF: u32 = 1 << (NUM_BITS - 2);

/// MSB-first bit reader. Reading past the end yields zeros, which the
/// arithmetic decoder needs for its final renormalizations.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    fn bit(&mut self) -> u32 {
        let byte = self.data.get(self.pos >> 3).copied().unwrap_or(0);
        let bit = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        bit as u32
    }
}

/// An adaptive frequency model over a contiguous range of symbols.
struct Model {
    first: u16,
    freq: Vec<u32>,
    total: u32,
    increment: u32,
    limit: u32,
}

impl Model {
    fn new(first: u16, last: u16, increment: u32, limit: u32) -> Self {
        let mut m = Self {
            first,
            freq: vec![0; (last - first + 1) as usize],
            total: 0,
            increment,
            limit,
        };
        m.reset();
        m
    }

    fn reset(&mut self) {
        self.freq.fill(self.increment);
        self.total = self.increment * self.freq.len() as u32;
    }

    fn bump(&mut self, i: usize) {
        self.freq[i] += self.increment;
        self.total += self.increment;
        if self.total > self.limit {
            self.total = 0;
            for f in &mut self.freq {
                *f = (*f + 1) >> 1;
                self.total += *f;
            }
        }
    }
}

struct ArithDecoder<'a> {
    bits: BitReader<'a>,
    range: u32,
    code: u32,
}

impl<'a> ArithDecoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut bits = BitReader { data, pos: 0 };
        let code = (0..NUM_BITS).fold(0, |c, _| (c << 1) | bits.bit());
        Self {
            bits,
            range: ONE,
            code,
        }
    }

    fn symbol(&mut self, m: &mut Model) -> u16 {
        let renorm = self.range / m.total;
        let target = self.code / renorm;

        let mut cum = 0;
        let mut n = 0;
        while n < m.freq.len() - 1 && cum + m.freq[n] <= target {
            cum += m.freq[n];
            n += 1;
        }

        let low = renorm * cum;
        self.code -= low;
        if cum + m.freq[n] == m.total {
            self.range -= low;
        } else {
            self.range = m.freq[n] * renorm;
        }
        while self.range <= HALF {
            self.range <<= 1;
            self.code = (self.code << 1) | self.bits.bit();
        }

        m.bump(n);
        m.first + n as u16
    }

    /// Read `n` bits, least significant first, each coded through `m`.
    fn bit_string(&mut self, m: &mut Model, n: u32) -> u32 {
        (0..n).fold(0, |v, i| v | ((self.symbol(m) as u32) << i))
    }

    /// True once every input bit has been consumed (with slack for the
    /// decoder's lookahead).
    fn exhausted(&self) -> bool {
        self.bits.pos > self.bits.data.len() * 8 + NUM_BITS as usize
    }
}

/// Decompress an Arsenic stream. `expected_len` (from the archive header)
/// is used to pre-size the output and to sanity check the result.
pub fn decompress(data: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    let mut d = ArithDecoder::new(data);
    let mut initial = Model::new(0, 1, 1, 256);
    let mut selector = Model::new(0, 10, 8, 1024);
    let mut mtf_models = [
        Model::new(2, 3, 8, 1024),
        Model::new(4, 7, 4, 1024),
        Model::new(8, 15, 4, 1024),
        Model::new(16, 31, 4, 1024),
        Model::new(32, 63, 2, 1024),
        Model::new(64, 127, 2, 1024),
        Model::new(128, 255, 1, 1024),
    ];

    let magic = [d.bit_string(&mut initial, 8), d.bit_string(&mut initial, 8)];
    if magic != [b'A' as u32, b's' as u32] {
        return Err(MacError::Invalid("not an Arsenic stream (bad magic)".into()));
    }
    let block_bits = d.bit_string(&mut initial, 4) + 9;
    let block_size = 1usize << block_bits;

    let mut out = Vec::with_capacity(expected_len);
    let mut rle = RunLength::default();
    let mut block = Vec::with_capacity(block_size);
    let mut transform = Vec::with_capacity(block_size);

    let mut end_of_blocks = d.symbol(&mut initial) != 0;
    while !end_of_blocks {
        selector.reset();
        mtf_models.iter_mut().for_each(Model::reset);
        let mut mtf = MoveToFront::new();

        let randomized = d.symbol(&mut initial) != 0;
        let mut index = d.bit_string(&mut initial, block_bits) as usize;

        block.clear();
        loop {
            let mut sel = d.symbol(&mut selector);
            if sel < 2 {
                // A run of zeros, its length written in bijective base 2
                // with digits 1 (sel 0) and 2 (sel 1).
                let (mut weight, mut count) = (1usize, 0usize);
                while sel < 2 {
                    count += weight * (sel as usize + 1);
                    weight *= 2;
                    sel = d.symbol(&mut selector);
                }
                if block.len() + count > block_size {
                    return Err(MacError::Invalid("Arsenic block overflow".into()));
                }
                let v = mtf.decode(0);
                block.resize(block.len() + count, v);
            }
            if sel == 10 {
                break;
            }
            let symbol = if sel == 2 {
                1
            } else {
                d.symbol(&mut mtf_models[sel as usize - 3])
            };
            if block.len() >= block_size {
                return Err(MacError::Invalid("Arsenic block overflow".into()));
            }
            block.push(mtf.decode(symbol as usize));
            if d.exhausted() {
                return Err(MacError::Invalid("Arsenic stream ended mid-block".into()));
            }
        }

        if index >= block.len() {
            return Err(MacError::Invalid("Arsenic BWT index out of range".into()));
        }
        if randomized {
            return Err(MacError::Unsupported(
                "randomized Arsenic blocks (randomization table not implemented)".into(),
            ));
        }

        inverse_bwt(&block, &mut transform);
        for _ in 0..block.len() {
            index = transform[index] as usize;
            rle.push(block[index], &mut out);
        }

        end_of_blocks = d.symbol(&mut initial) != 0;
    }

    let stored_crc = d.bit_string(&mut initial, 32);
    let actual_crc = crc32fast::hash(&out);
    if stored_crc != actual_crc {
        return Err(MacError::Invalid(format!(
            "Arsenic CRC mismatch: stored {stored_crc:#010x}, got {actual_crc:#010x}"
        )));
    }
    if out.len() != expected_len {
        return Err(MacError::Invalid(format!(
            "Arsenic output is {} bytes, expected {expected_len}",
            out.len()
        )));
    }
    Ok(out)
}

struct MoveToFront([u8; 256]);

impl MoveToFront {
    fn new() -> Self {
        Self(std::array::from_fn(|i| i as u8))
    }

    fn decode(&mut self, i: usize) -> u8 {
        let v = self.0[i];
        self.0.copy_within(0..i, 1);
        self.0[0] = v;
        v
    }
}

/// Build the BWT "next" vector: following it from the stored index walks the
/// original data in order.
fn inverse_bwt(block: &[u8], transform: &mut Vec<u32>) {
    let mut start = [0u32; 256];
    for &c in block {
        start[c as usize] += 1;
    }
    let mut sum = 0;
    for s in &mut start {
        let n = *s;
        *s = sum;
        sum += n;
    }
    transform.clear();
    transform.resize(block.len(), 0);
    for (i, &c) in block.iter().enumerate() {
        transform[start[c as usize] as usize] = i as u32;
        start[c as usize] += 1;
    }
}

/// Final stage: after four equal bytes, the next byte is a count of extra
/// copies. State carries across blocks.
#[derive(Default)]
struct RunLength {
    last: Option<u8>,
    count: u32,
}

impl RunLength {
    fn push(&mut self, byte: u8, out: &mut Vec<u8>) {
        if self.count == 4 {
            self.count = 0;
            if let Some(last) = self.last {
                out.resize(out.len() + byte as usize, last);
            }
            return;
        }
        if Some(byte) == self.last {
            self.count += 1;
        } else {
            self.count = 1;
            self.last = Some(byte);
        }
        out.push(byte);
    }
}
