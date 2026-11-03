//! Load a movie from any Iron Helix release into one common shape.

use anyhow::{Context, Result};

use crate::output::Pixels;

/// A decoded movie.
pub struct Clip {
    pub width: usize,
    pub height: usize,
    /// RGB frames with durations in milliseconds.
    pub frames: Vec<(Vec<u8>, f64)>,
    /// Unsigned 8-bit mono sound and its sample rate.
    pub sound: Option<(u32, Vec<u8>)>,
    /// Shape of one source pixel.
    pub pixel_aspect: (u32, u32),
}

impl Clip {
    pub fn duration_ms(&self) -> f64 {
        self.frames.iter().map(|(_, ms)| ms).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// PACo/CAVF, the Windows and Mac releases.
    Paco,
    /// Sega FILM, the Sega CD release.
    SegaFilm,
}

/// Identify a movie by its contents.
pub fn detect(data: &[u8]) -> Option<Kind> {
    if data.starts_with(b"FILM") {
        Some(Kind::SegaFilm)
    } else if paco::Movie::parse(data).is_ok() {
        Some(Kind::Paco)
    } else {
        None
    }
}

pub fn load(data: &[u8], kind: Kind) -> Result<Clip> {
    match kind {
        Kind::Paco => load_paco(data),
        Kind::SegaFilm => load_film(data),
    }
}

fn load_paco(data: &[u8]) -> Result<Clip> {
    let movie = paco::Movie::parse(data)?;
    let frames = paco::Decoder::new(&movie, Default::default()).decode_all()?;
    let frames = frames
        .iter()
        .map(|f| {
            let palette = f.palette.to_rgb_bytes();
            let rgb = Pixels::Indexed { data: &f.pixels, palette: &palette }.to_rgb();
            (rgb, f.duration_ms)
        })
        .collect();
    Ok(Clip {
        width: movie.width(),
        height: movie.height(),
        frames,
        sound: movie.audio().map(|a| (a.sample_rate, a.samples)),
        pixel_aspect: (1, 1), // Mac and PC pixels are square
    })
}

fn load_film(data: &[u8]) -> Result<Clip> {
    let film = segafilm::Film::parse(data)?;
    let decoded = segafilm::decode_film(&film)?;
    let (width, height) = decoded.frames.first().map(|(f, _)| (f.width, f.height)).context("no video frames")?;
    let raw = segafilm::film_audio(&film);
    Ok(Clip {
        width,
        height,
        frames: decoded.frames.into_iter().map(|(f, ms)| (f.rgb, ms)).collect(),
        sound: (!raw.is_empty()).then(|| (segafilm::audio::SAMPLE_RATE, segafilm::audio::to_unsigned(&raw))),
        pixel_aspect: (8, 7), // Genesis 256-pixel mode on a 4:3 TV
    })
}
