//! Cast bitmaps as drawable images, and drawing onto the stage.

use std::collections::HashMap;

use director::Movie;

use crate::assets::indexed_to_rgba;

/// A cast member's bitmap, ready to draw at a sprite position.
pub struct CastImage {
    pub width: usize,
    pub height: usize,
    /// Registration point from the top-left; a sprite's loc goes here.
    pub reg: (i32, i32),
    /// RGBA; alpha is 0 for white (palette index 0), which Director's
    /// transparent inks drop.
    pub rgba: Vec<u8>,
}

impl CastImage {
    /// The image's top-left on stage when its sprite is at `loc`.
    pub fn top_left(&self, loc: (i32, i32)) -> (i32, i32) {
        (loc.0 - self.reg.0, loc.1 - self.reg.1)
    }

    /// Whether a stage point hits the image's rectangle at `loc`.
    pub fn hit(&self, loc: (i32, i32), x: f32, y: f32) -> bool {
        let (l, t) = self.top_left(loc);
        x >= l as f32 && y >= t as f32 && x < (l + self.width as i32) as f32 && y < (t + self.height as i32) as f32
    }
}

pub struct Cast {
    images: HashMap<u16, CastImage>,
}

impl Cast {
    pub fn load(movie: &Movie, palette: &paco::Palette) -> Result<Self, String> {
        let mut images = HashMap::new();
        for b in movie.bitmaps().map_err(|e| e.to_string())? {
            let mut rgba = indexed_to_rgba(&b.pixels, palette);
            for (px, &index) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(&b.pixels) {
                if index == 0 {
                    px[3] = 0;
                }
            }
            images.insert(b.cast, CastImage { width: b.width, height: b.height, reg: b.info.reg_offset(), rgba });
        }
        Ok(Self { images })
    }

    pub fn get(&self, cast: u16) -> Option<&CastImage> {
        self.images.get(&cast)
    }
}

/// The stage as an RGBA pixel buffer.
pub struct Canvas<'a> {
    pub pixels: &'a mut [u8],
    pub width: usize,
    pub height: usize,
}

impl Canvas<'_> {
    /// Draw a cast image with its registration point at `loc`. With
    /// `transparent`, white pixels are skipped (Director's matte and
    /// background-transparent inks).
    pub fn draw(&mut self, img: &CastImage, loc: (i32, i32), transparent: bool) {
        let (left, top) = img.top_left(loc);
        self.blit(&img.rgba, img.width, (0, 0, img.width, img.height), left, top, transparent);
    }

    /// Copy `rect` (x, y, w, h) of an RGBA image to the stage at (x, y).
    pub fn blit(&mut self, src: &[u8], src_w: usize, rect: (usize, usize, usize, usize), x: i32, y: i32, transparent: bool) {
        let (sx, sy, w, h) = rect;
        for row in 0..h {
            let dy = y + row as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for col in 0..w {
                let dx = x + col as i32;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let s = ((sy + row) * src_w + sx + col) * 4;
                let Some(px) = src.get(s..s + 4) else { continue };
                if transparent && px[3] == 0 {
                    continue;
                }
                let d = (dy as usize * self.width + dx as usize) * 4;
                self.pixels[d..d + 3].copy_from_slice(&px[..3]);
                self.pixels[d + 3] = 255;
            }
        }
    }

    pub fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgb: [u8; 3]) {
        for y in y0.max(0)..y1.min(self.height as i32) {
            for x in x0.max(0)..x1.min(self.width as i32) {
                let d = (y as usize * self.width + x as usize) * 4;
                self.pixels[d..d + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
}
