//! The "Cinepak for Sega" video codec used by Sega CD FILM files.
//!
//! Despite the name it has little to do with desktop Cinepak. It is a vector
//! quantizer built around the Genesis video hardware: 8x8 tiles with 4-bit
//! pixels, each tile drawn with one of up to four 16-color palettes.
//!
//! Each video sample holds one frame:
//!
//! ```text
//! 'SM'
//! u16    flags: palette count - 1 in the low 2 bits
//! u8     flags
//! u24    nominal unpacked length (unused for decoding)
//! u16    width in tiles
//! u16    height in tiles
//! palettes        count x 16 Genesis CRAM words (entry 0 is transparent)
//! palette map     2 bits per tile (only when there are 2+ palettes)
//! u32 n, cb32[n]  codebook of 4x2-pixel blocks, 4 bytes each
//! u32 n, cb16[n]  codebook of 2x2-pixel blocks, 2 bytes each
//! method map      2 bits per tile
//! tile data       per tile, by method:
//!                   0: nothing; reuse this tile from two frames ago
//!                   1: 32 bytes, a raw 8x8 tile
//!                   2: 8 cb32 indices
//!                   3: 16 cb16 indices
//! ```
//!
//! Both 2-bit maps are in raster tile order, most significant bits first,
//! padded to a multiple of 4 bytes. Tiles are stored the Genesis way: 8 rows
//! of 4 bytes, high nibble = left pixel.
//!
//! Format documented by bgvanbur's scdtools
//! (<https://github.com/classiccoding/scdtools>); this is an independent
//! implementation of that description.

use crate::{FilmError, Result, be_u16, be_u32, slice};

const TILE_BYTES: usize = 32;
type Tile = [u8; TILE_BYTES];

/// One decoded frame as 24-bit RGB.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
    /// Size of the encoded frame in bytes, from its `SM` header.
    pub encoded_len: usize,
}

/// True if `data` starts with an `SM` header with plausible flags.
pub fn is_frame_start(data: &[u8]) -> bool {
    data.len() >= 4 && data[..2] == *b"SM" && {
        let flags = u16::from_be_bytes([data[2], data[3]]);
        matches!(flags & 0xFFFC, 0x0020 | 0x2000)
    }
}

/// All frames of a movie, one per video sample table entry.
pub struct DecodedFilm {
    /// Each frame with its duration in milliseconds.
    pub frames: Vec<(Frame, f64)>,
    /// Table entries that held no decodable frame; the previous frame was
    /// held for them (see [`decode_film`]).
    pub damaged: usize,
}

/// Decode every video frame of a movie.
///
/// Normally each sample table entry points at exactly one frame. A few Iron
/// Helix movies (15 of 2,334, mostly short probe-failure clips) have damaged
/// stretches: entries that all claim the same length while the real frames
/// are shorter and packed back to back, sometimes with a corrupt frame in
/// between. For an entry that doesn't start with a frame header, decoding
/// resumes where the previous frame ended if a frame starts there; failing
/// that, the previous frame is held so the timing stays intact.
pub fn decode_film(film: &crate::Film) -> Result<DecodedFilm> {
    if !film.fourcc.eq_ignore_ascii_case(b"SEGA") {
        return Err(FilmError::Unsupported(format!("video codec '{}'", film.fourcc_str())));
    }
    let data = film.data();
    let mut decoder = VideoDecoder::new();
    let mut frames: Vec<(Frame, f64)> = Vec::new();
    let mut damaged = 0;
    let mut previous_end: Option<usize> = None;

    for (i, s) in film.video().enumerate() {
        let duration = film.duration_ms(s);
        let start = if is_frame_start(&data[s.offset..]) {
            Some(s.offset)
        } else {
            previous_end
                .filter(|&e| e < data.len() && is_frame_start(&data[e..]))
                // Padding before the header inside the sample (seen in one file).
                .or_else(|| (s.offset..s.offset + s.len).find(|&a| is_frame_start(&data[a..])))
        };
        let decoded = start.and_then(|at| decoder.decode(&data[at..]).ok().map(|f| (at, f)));
        match decoded {
            Some((at, frame)) => {
                previous_end = Some(at + frame.encoded_len);
                frames.push((frame, duration));
            }
            None => {
                let held = frames
                    .last()
                    .map(|(f, _)| f.clone())
                    .ok_or_else(|| FilmError::Invalid(format!("video frame {i}: no frame to decode")))?;
                damaged += 1;
                previous_end = None;
                frames.push((held, duration));
            }
        }
    }
    Ok(DecodedFilm { frames, damaged })
}

/// The movie's sound as raw sign-magnitude samples, in table order.
pub fn film_audio(film: &crate::Film) -> Vec<u8> {
    film.audio().flat_map(|s| film.sample_data(s).iter().copied()).collect()
}

/// Length in bytes of the encoded frame at the start of `d`, worked out from
/// its header and maps without decoding pixels.
pub fn frame_len(d: &[u8]) -> Result<usize> {
    if !is_frame_start(d) {
        return Err(FilmError::Invalid("no 'SM' frame header".into()));
    }
    let palette_count = (be_u16(d, 2)? & 3) as usize + 1;
    let tile_count = be_u16(d, 8)? as usize * be_u16(d, 10)? as usize;
    let map_len = 4 * tile_count.div_ceil(16);
    let mut p = 12 + palette_count * 32;
    if palette_count > 1 {
        p += map_len;
    }
    p += 4 + be_u32(d, p)? as usize; // cb32
    p += 4 + be_u32(d, p)? as usize; // cb16
    let (methods, next) = two_bit_map(d, p, tile_count)?;
    let tiles: usize = methods.iter().map(|&m| [0, 32, 8, 16][m as usize]).sum();
    let len = next + tiles;
    if len > d.len() {
        return Err(FilmError::Eof(len));
    }
    Ok(len)
}

/// Decodes a sequence of frames. Keeps the tiles of the last two frames,
/// because method 0 reuses a tile from two frames back (the player
/// double-buffers).
#[derive(Default)]
pub struct VideoDecoder {
    previous: Vec<Tile>,
    two_back: Vec<Tile>,
}

/// Genesis 3-bit color channel to 8 bits (0, 36, 73, ... 255).
fn channel(v: u16) -> u8 {
    let v = (v & 7) as u8;
    (v << 5) | (v << 2) | (v >> 1)
}

/// Codebook entry `index` of `size` bytes.
fn entry(codebook: &[u8], index: u8, size: usize) -> Result<&[u8]> {
    slice(codebook, index as usize * size, size)
}

/// Read a map of 2-bit values, one per tile.
fn two_bit_map(d: &[u8], at: usize, tiles: usize) -> Result<(Vec<u8>, usize)> {
    let len = 4 * tiles.div_ceil(16);
    let bytes = slice(d, at, len)?;
    let map = (0..tiles).map(|t| (bytes[t / 4] >> (2 * (3 - t % 4))) & 3).collect();
    Ok((map, at + len))
}

impl VideoDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode the frame at the start of `d`. Bytes after the frame are ignored.
    pub fn decode(&mut self, d: &[u8]) -> Result<Frame> {
        if !is_frame_start(d) {
            return Err(FilmError::Invalid("no 'SM' frame header".into()));
        }

        let flags = be_u16(d, 2)?;
        let palette_count = (flags & 3) as usize + 1;
        let tiles_wide = be_u16(d, 8)? as usize;
        let tiles_high = be_u16(d, 10)? as usize;
        let tile_count = tiles_wide * tiles_high;

        let mut p = 12;
        let mut palettes = Vec::with_capacity(palette_count);
        for _ in 0..palette_count {
            let mut colors = [[0u8; 3]; 16];
            for (i, c) in colors.iter_mut().enumerate() {
                let w = be_u16(d, p + i * 2)?;
                if w & 0xF111 != 0 {
                    return Err(FilmError::Invalid(format!("{w:#06x} is not a Genesis color")));
                }
                *c = [channel(w >> 1), channel(w >> 5), channel(w >> 9)];
            }
            palettes.push(colors);
            p += 32;
        }

        let palette_map = if palette_count > 1 {
            let (map, next) = two_bit_map(d, p, tile_count)?;
            p = next;
            map
        } else {
            vec![0; tile_count]
        };

        let cb32_len = be_u32(d, p)? as usize;
        let cb32 = slice(d, p + 4, cb32_len)?;
        p += 4 + cb32_len;
        let cb16_len = be_u32(d, p)? as usize;
        let cb16 = slice(d, p + 4, cb16_len)?;
        p += 4 + cb16_len;

        let (methods, next) = two_bit_map(d, p, tile_count)?;
        p = next;

        let mut tiles = Vec::with_capacity(tile_count);
        for (t, &method) in methods.iter().enumerate() {
            let mut tile = [0u8; TILE_BYTES];
            match method {
                0 => {
                    tile = *self.two_back.get(t).ok_or_else(|| {
                        FilmError::Invalid("tile reuses a frame that doesn't exist".into())
                    })?;
                }
                1 => {
                    tile.copy_from_slice(slice(d, p, 32)?);
                    p += 32;
                }
                2 => {
                    // Four pairs of 4x2 blocks, each pair covering two rows.
                    let idx = slice(d, p, 8)?;
                    p += 8;
                    for pair in 0..4 {
                        let a = entry(cb32, idx[pair * 2], 4)?;
                        let b = entry(cb32, idx[pair * 2 + 1], 4)?;
                        let row = pair * 8;
                        tile[row..row + 8].copy_from_slice(&[a[0], a[1], b[0], b[1], a[2], a[3], b[2], b[3]]);
                    }
                }
                _ => {
                    // Four rows of four 2x2 blocks.
                    let idx = slice(d, p, 16)?;
                    p += 16;
                    for band in 0..4 {
                        let c = [
                            entry(cb16, idx[band * 4], 2)?,
                            entry(cb16, idx[band * 4 + 1], 2)?,
                            entry(cb16, idx[band * 4 + 2], 2)?,
                            entry(cb16, idx[band * 4 + 3], 2)?,
                        ];
                        let row = band * 8;
                        tile[row..row + 8]
                            .copy_from_slice(&[c[0][0], c[1][0], c[2][0], c[3][0], c[0][1], c[1][1], c[2][1], c[3][1]]);
                    }
                }
            }
            tiles.push(tile);
        }

        let width = tiles_wide * 8;
        let height = tiles_high * 8;
        let mut rgb = vec![0u8; width * height * 3];
        for (t, tile) in tiles.iter().enumerate() {
            let palette = palettes.get(palette_map[t] as usize).ok_or_else(|| {
                FilmError::Invalid(format!("tile {t} uses palette {} of {palette_count}", palette_map[t]))
            })?;
            let (tx, ty) = (t % tiles_wide * 8, t / tiles_wide * 8);
            for y in 0..8 {
                for x in 0..8 {
                    let byte = tile[y * 4 + x / 2];
                    let index = if x % 2 == 0 { byte >> 4 } else { byte & 0x0F };
                    // Index 0 is transparent; the backdrop behind the movie is black.
                    let color = if index == 0 { [0, 0, 0] } else { palette[index as usize] };
                    let o = ((ty + y) * width + tx + x) * 3;
                    rgb[o..o + 3].copy_from_slice(&color);
                }
            }
        }

        self.two_back = std::mem::replace(&mut self.previous, tiles);
        Ok(Frame {
            width,
            height,
            rgb,
            encoded_len: p,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1x1-tile, single-palette frame using a raw tile (method 1).
    #[test]
    fn raw_tile_single_palette() {
        let mut f = b"SM\x00\x20\x00\x00\x00\x00\x00\x01\x00\x01".to_vec();
        let mut palette = [0u16; 16];
        palette[1] = 0x000E; // bright red
        palette[2] = 0x0E00; // bright blue
        for w in palette {
            f.extend_from_slice(&w.to_be_bytes());
        }
        f.extend_from_slice(&0u32.to_be_bytes()); // empty cb32
        f.extend_from_slice(&0u32.to_be_bytes()); // empty cb16
        f.extend_from_slice(&[0b0100_0000, 0, 0, 0]); // tile 0: method 1
        f.extend(std::iter::repeat_n(0x12, 32)); // pixels alternate 1, 2

        let frame = VideoDecoder::new().decode(&f).unwrap();
        assert_eq!((frame.width, frame.height), (8, 8));
        assert_eq!(&frame.rgb[0..6], &[255, 0, 0, 0, 0, 255]);
        assert_eq!(frame.encoded_len, f.len());
    }
}
