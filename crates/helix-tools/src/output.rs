//! Writing decoded frames and sound to standard files: PNG, animated PNG and WAV.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Pixels of one image.
pub enum Pixels<'a> {
    /// One byte per pixel, indexing `palette` (`[r, g, b, r, g, b, ...]`).
    Indexed { data: &'a [u8], palette: &'a [u8] },
    /// Three bytes per pixel.
    Rgb(&'a [u8]),
}

impl Pixels<'_> {
    fn to_rgb(&self) -> Vec<u8> {
        match self {
            Pixels::Rgb(rgb) => rgb.to_vec(),
            Pixels::Indexed { data, palette } => data
                .iter()
                .flat_map(|&i| {
                    let o = i as usize * 3;
                    [palette[o], palette[o + 1], palette[o + 2]]
                })
                .collect(),
        }
    }
}

pub struct AnimFrame<'a> {
    pub pixels: Pixels<'a>,
    pub duration_ms: f64,
}

fn create(path: &Path) -> Result<BufWriter<fs::File>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let f = fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    Ok(BufWriter::new(f))
}

/// Write a single image.
pub fn write_png(path: &Path, width: usize, height: usize, pixels: &Pixels) -> Result<()> {
    let mut enc = png::Encoder::new(create(path)?, width as u32, height as u32);
    enc.set_depth(png::BitDepth::Eight);
    match pixels {
        Pixels::Indexed { data, palette } => {
            enc.set_color(png::ColorType::Indexed);
            enc.set_palette(palette.to_vec());
            enc.write_header()?.write_image_data(data)?;
        }
        Pixels::Rgb(rgb) => {
            enc.set_color(png::ColorType::Rgb);
            enc.write_header()?.write_image_data(rgb)?;
        }
    }
    Ok(())
}

/// Write an animated PNG that plays once. If every frame is indexed with the
/// same palette the file stays 8-bit indexed (pixel-exact and small);
/// otherwise it is RGB.
pub fn write_apng(path: &Path, width: usize, height: usize, frames: &[AnimFrame]) -> Result<()> {
    let first = frames.first().context("no frames to write")?;
    let shared_palette = match &first.pixels {
        Pixels::Indexed { palette, .. } => frames
            .iter()
            .all(|f| matches!(&f.pixels, Pixels::Indexed { palette: p, .. } if p == palette))
            .then_some(*palette),
        Pixels::Rgb(_) => None,
    };

    let mut enc = png::Encoder::new(create(path)?, width as u32, height as u32);
    enc.set_depth(png::BitDepth::Eight);
    if let Some(palette) = shared_palette {
        enc.set_color(png::ColorType::Indexed);
        enc.set_palette(palette.to_vec());
    } else {
        enc.set_color(png::ColorType::Rgb);
    }
    enc.set_animated(frames.len() as u32, 1)?;
    let mut writer = enc.write_header()?;

    for f in frames {
        // Delays are stored as a fraction of a second.
        writer.set_frame_delay(f.duration_ms.round() as u16, 1000)?;
        match (&f.pixels, shared_palette) {
            (Pixels::Indexed { data, .. }, Some(_)) => writer.write_image_data(data)?,
            (pixels, _) => writer.write_image_data(&pixels.to_rgb())?,
        }
    }
    writer.finish()?;
    Ok(())
}

/// Write unsigned 8-bit mono PCM as a WAV file.
pub fn write_wav_u8(path: &Path, sample_rate: u32, samples: &[u8]) -> Result<()> {
    let len = samples.len() as u32;
    let mut out = Vec::with_capacity(44 + samples.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes()); // bytes per second
    out.extend_from_slice(&1u16.to_le_bytes()); // block align
    out.extend_from_slice(&8u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(samples);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, out)?;
    Ok(())
}

/// Every file under `dir`, recursively, sorted.
pub fn collect_files(dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// `out/<path relative to base><suffix>`, e.g. for mirroring a folder tree.
pub fn mirrored(out: &Path, base: &Path, file: &Path, suffix: &str) -> Result<PathBuf> {
    let mut dest = out.join(file.strip_prefix(base)?).into_os_string();
    dest.push(suffix);
    Ok(PathBuf::from(dest))
}
