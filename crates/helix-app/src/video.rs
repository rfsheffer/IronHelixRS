//! Playing PACo movies in the view window.

use std::path::Path;

use crate::assets::indexed_to_rgba;

/// A decoded movie, ready to play.
pub struct Clip {
    pub width: usize,
    /// RGBA frames and how long each is shown, in seconds.
    pub frames: Vec<(Vec<u8>, f32)>,
}

impl Clip {
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let movie = paco::Movie::parse(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        let frames = paco::Decoder::new(&movie, Default::default())
            .decode_all()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            width: movie.width(),
            frames: frames
                .iter()
                .map(|f| (indexed_to_rgba(&f.pixels, &f.palette), (f.duration_ms / 1000.0) as f32))
                .collect(),
        })
    }
}

/// Steps through a clip in real time.
pub struct Player {
    pub clip: Clip,
    frame: usize,
    elapsed: f32,
}

impl Player {
    pub fn new(clip: Clip) -> Self {
        Self { clip, frame: 0, elapsed: 0.0 }
    }

    /// Advance by `dt` seconds; returns true when the clip has finished.
    pub fn update(&mut self, dt: f32) -> bool {
        self.elapsed += dt;
        while let Some((_, duration)) = self.clip.frames.get(self.frame) {
            if self.elapsed < *duration {
                return false;
            }
            self.elapsed -= duration;
            if self.frame + 1 >= self.clip.frames.len() {
                return true;
            }
            self.frame += 1;
        }
        true
    }

    pub fn current(&self) -> Option<&[u8]> {
        self.clip.frames.get(self.frame).map(|(p, _)| p.as_slice())
    }
}
