//! Plays a [`Movie`]'s opcodes and produces finished frames.

use crate::error::{PacoError, Result};
use crate::movie::Movie;
use crate::ops::{Compression, Op, OpBitmap, OpPalette, PaletteType};
use crate::palette::Palette;
use crate::rle::{self, RleStats, Target};

/// Order in which an interlaced bitmap stores its scan lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOrder {
    /// Top to bottom.
    Sequential,
    /// All even rows (0, 2, 4, ...) then all odd rows. This is what PACo
    /// uses for COMP_RLE_INTERLACED (verified against Iron Helix NAV movies).
    EvenThenOdd,
}

impl RowOrder {
    /// Build a table mapping stream row number to row within the area.
    pub fn map(self, height: usize) -> Vec<usize> {
        match self {
            RowOrder::Sequential => (0..height).collect(),
            RowOrder::EvenThenOdd => (0..height).step_by(2).chain((1..height).step_by(2)).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecoderOptions {
    /// Row order used for `RleInterlaced` bitmaps.
    pub interlaced_order: RowOrder,
    /// Palette used when a movie asks for the system palette.
    pub system_palette: Palette,
    /// Keep going after a bitmap fails to decode (the frame gets an error).
    pub lenient: bool,
}

impl Default for DecoderOptions {
    fn default() -> Self {
        Self {
            interlaced_order: RowOrder::EvenThenOdd,
            system_palette: Palette::mac_system(),
            lenient: true,
        }
    }
}

/// One displayed frame: a full canvas of palette indices plus its palette.
#[derive(Debug, Clone)]
pub struct Frame {
    pub index: usize,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub palette: Palette,
    pub duration_ms: f64,
    pub stats: Option<RleStats>,
    /// Set when the bitmap failed to decode (only in lenient mode).
    pub error: Option<String>,
}

pub struct Decoder<'m> {
    movie: &'m Movie,
    state: State,
}

impl<'m> Decoder<'m> {
    pub fn new(movie: &'m Movie, opts: DecoderOptions) -> Self {
        Self { movie, state: State::new(movie, opts) }
    }

    /// Decode every frame in the movie.
    pub fn decode_all(mut self) -> Result<Vec<Frame>> {
        let mut frames = Vec::with_capacity(self.movie.header.num_frames.max(0) as usize);
        for op in self.movie.ops() {
            if let Some(frame) = self.state.apply(op)? {
                frames.push(frame);
            }
        }
        Ok(frames)
    }
}

/// Decodes a movie it owns one frame at a time, for long movies whose
/// frames would take too much memory all at once.
pub struct Stream {
    movie: Movie,
    state: State,
    /// The next opcode: chunk and position in it.
    chunk: usize,
    op: usize,
}

impl Stream {
    pub fn new(movie: Movie, opts: DecoderOptions) -> Self {
        let state = State::new(&movie, opts);
        Self { movie, state, chunk: 0, op: 0 }
    }

    pub fn movie(&self) -> &Movie {
        &self.movie
    }

    /// The next frame, or None at the end.
    pub fn next_frame(&mut self) -> Option<Result<Frame>> {
        while let Some(chunk) = self.movie.chunks.get(self.chunk) {
            let Some(op) = chunk.ops.get(self.op) else {
                self.chunk += 1;
                self.op = 0;
                continue;
            };
            self.op += 1;
            match self.state.apply(op) {
                Ok(Some(frame)) => return Some(Ok(frame)),
                Ok(None) => {}
                Err(e) => return Some(Err(e)),
            }
        }
        None
    }
}

/// The canvas and settings carried from opcode to opcode.
struct State {
    opts: DecoderOptions,
    width: usize,
    height: usize,
    canvas: Vec<u8>,
    palette: Palette,
    lut: [u8; 16],
    default_ms: f64,
    next_ms: Option<f64>,
    frames: usize,
}

impl State {
    fn new(movie: &Movie, opts: DecoderOptions) -> Self {
        Self {
            width: movie.width(),
            height: movie.height(),
            canvas: vec![0; movie.width() * movie.height()],
            palette: opts.system_palette.clone(),
            lut: [0; 16],
            default_ms: movie.header.default_frame_ms(),
            next_ms: None,
            frames: 0,
            opts,
        }
    }

    /// Play one opcode; a bitmap finishes a frame.
    fn apply(&mut self, op: &Op) -> Result<Option<Frame>> {
        match op {
            Op::Palette(p) => self.apply_palette(p),
            Op::Delay(d) => {
                if d.persist() {
                    self.default_ms = d.frame_ms();
                } else {
                    self.next_ms = Some(d.frame_ms());
                }
            }
            Op::Fill { rgb } => {
                // The fill union holds an index when the movie is 8-bit.
                self.canvas.fill(rgb.r);
            }
            Op::Bitmap(bm) => {
                let (stats, error) = match self.draw_bitmap(bm) {
                    Ok(stats) => (Some(stats), None),
                    Err(e) if self.opts.lenient => (None, Some(e.to_string())),
                    Err(e) => return Err(e),
                };
                let frame = Frame {
                    index: self.frames,
                    width: self.width,
                    height: self.height,
                    pixels: self.canvas.clone(),
                    palette: self.palette.clone(),
                    duration_ms: self.next_ms.take().unwrap_or(self.default_ms),
                    stats,
                    error,
                };
                self.frames += 1;
                return Ok(Some(frame));
            }
            _ => {}
        }
        Ok(None)
    }

    fn apply_palette(&mut self, p: &OpPalette) {
        self.palette = match p.palette_type() {
            PaletteType::System => self.opts.system_palette.clone(),
            PaletteType::Solid => Palette::solid(p.solid),
            PaletteType::Given => Palette::from_colors(&p.colors),
            PaletteType::Other(_) => return,
        };
    }

    fn draw_bitmap(&mut self, bm: &OpBitmap) -> Result<RleStats> {
        let (w, h) = (self.width, self.height);
        let (left, top) = (bm.left as usize, bm.top as usize);
        let (bw, bh) = (bm.width as usize, bm.height as usize);
        if bw == 0 || bh == 0 {
            // "Nothing changed" frame: the packer still emits a BITMAP (so the
            // frame count and timing hold) with an empty area and padding.
            return Ok(RleStats::default());
        }
        if left + bw > w || top + bh > h {
            return Err(PacoError::Invalid(format!(
                "bitmap area {bw}x{bh}+{left}+{top} is outside the {w}x{h} frame"
            )));
        }

        let order = match bm.compression() {
            Compression::Rle => RowOrder::Sequential,
            Compression::RleInterlaced => self.opts.interlaced_order,
            Compression::None => {
                // Raw rows of palette indices.
                for row in 0..bh {
                    let src = bm.data.get(row * bw..(row + 1) * bw).ok_or_else(|| {
                        PacoError::Invalid("uncompressed bitmap is too short".into())
                    })?;
                    let dst = (top + row) * w + left;
                    self.canvas[dst..dst + bw].copy_from_slice(src);
                }
                return Ok(RleStats::default());
            }
            Compression::Other(n) => {
                return Err(PacoError::Invalid(format!("unsupported compression {n}")));
            }
        };

        let row_map = order.map(bh);
        let mut target = Target {
            canvas: &mut self.canvas,
            stride: w,
            left,
            top,
            width: bw,
            height: bh,
            row_map: &row_map,
        };
        rle::decode(&bm.data, &mut target, &mut self.lut)
    }
}
