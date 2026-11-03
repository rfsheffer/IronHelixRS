//! Sega FILM movies as used on the Sega CD.
//!
//! - [`film`]: the FILM container (header, FDSC description, STAB sample table)
//! - [`cinepak_sega`]: the "Cinepak for Sega" video codec (`SM` frames)
//! - [`audio`]: Sega CD sign-magnitude PCM
//!
//! The video codec was documented by bgvanbur's scdtools
//! (<https://github.com/classiccoding/scdtools>); this is an independent
//! implementation of that format description.

pub mod audio;
pub mod cinepak_sega;
pub mod film;

pub use cinepak_sega::{DecodedFilm, Frame, VideoDecoder, decode_film, film_audio};
pub use film::{Film, Sample, SampleKind};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FilmError {
    #[error("unexpected end of data at offset {0}")]
    Eof(usize),

    #[error("invalid data: {0}")]
    Invalid(String),

    #[error("unsupported: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, FilmError>;

pub(crate) fn be_u16(d: &[u8], at: usize) -> Result<u16> {
    let b = d.get(at..at + 2).ok_or(FilmError::Eof(at))?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

pub(crate) fn be_u32(d: &[u8], at: usize) -> Result<u32> {
    let b = d.get(at..at + 4).ok_or(FilmError::Eof(at))?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

pub(crate) fn slice(d: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    d.get(at..at + len).ok_or(FilmError::Eof(at))
}
