//! The Sega CD's extended ending (`SEGUES/ENDING.SEG`), played full stage
//! after a skill-5 win (design doc §14.4).
//!
//! The movie is 98 seconds of Sega FILM, so frames are decoded as they're
//! shown rather than all up front. Sega CD pixels are 8:7 (wider than
//! tall); the picture is scaled to the stage's width at that aspect and
//! centered.

use std::path::Path;

use segafilm::{Film, SampleKind, VideoDecoder};

use crate::sound::Sounds;
use crate::sprites::Canvas;

pub struct Ending {
    data: Vec<u8>,
    /// Video samples: offset, length, seconds shown.
    frames: Vec<(usize, usize, f32)>,
    decoder: VideoDecoder,
    /// The frame on screen (RGB) and its size.
    current: Option<(Vec<u8>, usize, usize)>,
    index: usize,
    elapsed: f32,
    audio: Option<Vec<u8>>,
    started: bool,
}

impl Ending {
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let film = Film::parse(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        let frames = film
            .samples
            .iter()
            .filter(|s| matches!(s.kind, SampleKind::Video { .. }))
            .map(|s| (s.offset, s.len, (film.duration_ms(s) / 1000.0) as f32))
            .collect();
        let pcm = segafilm::audio::to_unsigned(&segafilm::film_audio(&film));
        let audio = (!pcm.is_empty()).then(|| crate::sound::pcm8_to_wav(segafilm::audio::SAMPLE_RATE, &pcm));
        let mut ending = Self { data, frames, decoder: VideoDecoder::new(), current: None, index: 0, elapsed: 0.0, audio, started: false };
        ending.decode(0);
        Ok(ending)
    }

    /// Decode frame `i`; a damaged frame leaves the last one up.
    fn decode(&mut self, i: usize) {
        let Some(&(offset, len, _)) = self.frames.get(i) else { return };
        let Some(sample) = self.data.get(offset..offset + len) else { return };
        let start = (0..sample.len()).find(|&a| segafilm::cinepak_sega::is_frame_start(&sample[a..])).unwrap_or(0);
        if let Ok(f) = self.decoder.decode(&self.data[offset + start..]) {
            self.current = Some((f.rgb, f.width, f.height));
        }
    }

    /// Advance; true once the movie is over.
    pub async fn update(&mut self, dt: f32, sounds: &mut Sounds) -> bool {
        if !self.started {
            self.started = true;
            if let Some(wav) = &self.audio {
                sounds.play_wav("ENDING.SEG", wav).await;
            }
            return false;
        }
        self.elapsed += dt;
        while let Some(&(_, _, duration)) = self.frames.get(self.index) {
            if self.elapsed < duration {
                return false;
            }
            self.elapsed -= duration;
            self.index += 1;
            if self.index >= self.frames.len() {
                return true;
            }
            self.decode(self.index);
        }
        true
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.fill(0, 0, canvas.width as i32, canvas.height as i32, [0, 0, 0]);
        let Some((rgb, w, h)) = &self.current else { return };
        let (w, h) = (*w, *h);
        if w == 0 || h == 0 {
            return;
        }
        // 8:7 pixels: the picture is w * 8/7 wide for every h tall.
        let display_w = w as f32 * 8.0 / 7.0;
        let scale = (canvas.width as f32 / display_w).min(canvas.height as f32 / h as f32);
        let (out_w, out_h) = ((display_w * scale) as usize, (h as f32 * scale) as usize);
        let (x0, y0) = ((canvas.width - out_w.min(canvas.width)) / 2, (canvas.height - out_h.min(canvas.height)) / 2);
        for y in 0..out_h.min(canvas.height) {
            let sy = (y * h / out_h).min(h - 1);
            for x in 0..out_w.min(canvas.width) {
                let sx = (x * w / out_w).min(w - 1);
                let s = (sy * w + sx) * 3;
                let d = ((y0 + y) * canvas.width + x0 + x) * 4;
                canvas.pixels[d..d + 3].copy_from_slice(&rgb[s..s + 3]);
            }
        }
    }
}
