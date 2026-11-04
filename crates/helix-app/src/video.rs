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
        let stretch = pacing(&movie);
        clip.frames.iter_mut().for_each(|(_, d)| *d *= stretch);
        clip.audio = sound_track(&movie, path);
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

/// How much to stretch a movie's frame delays. Without the header's
/// "maintain frame rate" flag the original player paces the pictures by the
/// sound, which runs 2-21% longer than the frame delays add up to in those
/// movies (the opening, some video messages, the eject clips): the frames
/// stretch to match. One movie with the flag (Computer:Eject) runs 22% long
/// the same way; the others with it are within 5%.
fn pacing(movie: &paco::Movie) -> f32 {
    let Some(a) = movie.audio() else { return 1.0 };
    let sound = a.samples.len() as f64 / a.sample_rate as f64;
    // The frames' running time, from the delay opcodes.
    let (mut default_ms, mut next_ms, mut frames_ms) = (movie.header.default_frame_ms(), None, 0.0);
    for op in movie.ops() {
        match op {
            paco::Op::Delay(d) if d.persist() => default_ms = d.frame_ms(),
            paco::Op::Delay(d) => next_ms = Some(d.frame_ms()),
            paco::Op::Bitmap(_) => frames_ms += next_ms.take().unwrap_or(default_ms),
            _ => {}
        }
    }
    let frames = frames_ms / 1000.0;
    let paced_by_sound = !movie.header.maintain_framerate() || sound > frames * 1.15;
    if paced_by_sound && frames > 0.0 && sound > frames { (sound / frames) as f32 } else { 1.0 }
}

/// A movie's own sound track as a WAV, named for caching.
fn sound_track(movie: &paco::Movie, path: &Path) -> Option<(String, Vec<u8>)> {
    movie.audio().map(|a| (path.display().to_string(), crate::sound::pcm8_to_wav(a.sample_rate, &a.samples)))
}

/// Plays a long movie, decoding each frame as it's shown (the title's
/// movies and the credits would take hundreds of megabytes decoded whole).
pub struct Stream {
    stream: paco::Stream,
    width: usize,
    stretch: f32,
    /// The frame on screen (RGBA) and how long it stays.
    current: Option<(Vec<u8>, f32)>,
    elapsed: f32,
    done: bool,
    /// The movie's sound track, as for [`Clip`].
    pub audio: Option<(String, Vec<u8>)>,
}

impl Stream {
    pub fn load(path: &Path) -> Result<Self, String> {
        let movie = parse(path)?;
        let (width, stretch, audio) = (movie.width(), pacing(&movie), sound_track(&movie, path));
        let mut stream = Self { stream: paco::Stream::new(movie, Default::default()), width, stretch, current: None, elapsed: 0.0, done: false, audio };
        stream.advance();
        Ok(stream)
    }

    fn advance(&mut self) -> bool {
        match self.stream.next_frame() {
            Some(Ok(f)) => {
                self.current = Some((indexed_to_rgba(&f.pixels, &f.palette), (f.duration_ms / 1000.0) as f32 * self.stretch));
                true
            }
            _ => false,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Advance by `dt` seconds; returns true when the movie has finished
    /// (its last frame stays up).
    pub fn update(&mut self, dt: f32) -> bool {
        if self.done {
            return true;
        }
        self.elapsed += dt;
        while let Some((_, duration)) = &self.current {
            if self.elapsed < *duration {
                return false;
            }
            self.elapsed -= duration;
            if !self.advance() {
                self.done = true;
                return true;
            }
        }
        true
    }

    /// Stop here (the next update reports it finished).
    pub fn finish(&mut self) {
        self.done = true;
    }

    pub fn current(&self) -> Option<&[u8]> {
        self.current.as_ref().map(|(p, _)| p.as_slice())
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

    /// Streaming gives the same frames and timing as decoding it whole.
    #[test]
    fn streams_match_clips() {
        let path = Path::new("../../out/mac/files/Iron Helix™ Files/Opening/Entering Ship PPV");
        let Ok(clip) = Clip::load(path) else { return };
        let mut stream = Stream::load(path).unwrap();
        let last = clip.frames.len() - 1;
        for (i, (pixels, duration)) in clip.frames.iter().enumerate() {
            assert_eq!(stream.current(), Some(pixels.as_slice()), "frame {i}");
            assert!(!stream.update(duration / 2.0), "frame {i} ended early");
            assert_eq!(stream.update(duration / 2.0), i == last, "frame {i}");
        }
    }
}
