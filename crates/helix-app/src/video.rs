//! Playing PACo movies, with their own sound tracks where they have one.

use std::path::Path;

use crate::assets::indexed_to_rgba;

/// A decoded movie, ready to play.
pub struct Clip {
    pub width: usize,
    /// RGBA frames and how long each is shown, in seconds.
    pub frames: Vec<(Vec<u8>, f32)>,
    /// The movie's own sound track as a WAV file (computer clips, video
    /// messages, phase movies, segues), and a name to cache it by.
    pub audio: Option<(String, Vec<u8>)>,
}

fn parse(path: &Path) -> Result<paco::Movie, String> {
    let data = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    paco::Movie::parse(&data).map_err(|e| format!("{}: {e}", path.display()))
}

/// Decode a movie's frames (palette indices).
pub fn decode(path: &Path) -> Result<Vec<paco::Frame>, String> {
    let movie = parse(path)?;
    paco::Decoder::new(&movie, Default::default()).decode_all().map_err(|e| format!("{}: {e}", path.display()))
}

impl Clip {
    pub fn load(path: &Path) -> Result<Self, String> {
        let movie = parse(path)?;
        let frames = paco::Decoder::new(&movie, Default::default())
            .decode_all()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut clip = Self::from_frames(&frames);
        let audio = movie.audio();
        // Without the header's "maintain frame rate" flag the original player
        // paces the pictures by the sound, which runs 2-21% longer than the
        // frame delays add up to in those movies (the opening, some video
        // messages, the eject clips): stretch the frames to match. One movie
        // with the flag (Computer:Eject) runs 22% long the same way; the
        // others with it are within 5%.
        if let Some(a) = &audio {
            let sound = a.samples.len() as f32 / a.sample_rate as f32;
            let frames: f32 = clip.frames.iter().map(|(_, d)| d).sum();
            let paced_by_sound = !movie.header.maintain_framerate() || sound > frames * 1.15;
            if paced_by_sound && frames > 0.0 && sound > frames {
                let stretch = sound / frames;
                clip.frames.iter_mut().for_each(|(_, d)| *d *= stretch);
            }
        }
        clip.audio = audio.map(|a| (path.display().to_string(), crate::sound::pcm8_to_wav(a.sample_rate, &a.samples)));
        Ok(clip)
    }

    pub fn from_frames(frames: &[paco::Frame]) -> Self {
        Self {
            width: frames.first().map_or(0, |f| f.width),
            frames: frames
                .iter()
                .map(|f| (indexed_to_rgba(&f.pixels, &f.palette), (f.duration_ms / 1000.0) as f32))
                .collect(),
            audio: None,
        }
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

    /// Jump to the end (the next update reports it finished).
    pub fn finish(&mut self) {
        self.frame = self.clip.frames.len().saturating_sub(1);
        self.elapsed = f32::MAX;
    }

    pub fn current(&self) -> Option<&[u8]> {
        self.clip.frames.get(self.frame).map(|(p, _)| p.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The opening's frames last as long as its sound (it has no "maintain
    /// frame rate" flag). Needs the extracted Mac files; skipped without.
    #[test]
    fn opening_frames_follow_the_sound() {
        let path = Path::new("../../out/mac/files/Iron Helix™ Files/Opening/OPENING");
        let Ok(clip) = Clip::load(path) else { return };
        let frames: f32 = clip.frames.iter().map(|(_, d)| d).sum();
        let (_, wav) = clip.audio.expect("the opening has sound");
        let sound = (wav.len() - 44) as f32 / 11_025.0;
        assert!((frames - sound).abs() < 0.1, "frames {frames:.2} s, sound {sound:.2} s");
        assert!(frames > 130.0);
    }
}
