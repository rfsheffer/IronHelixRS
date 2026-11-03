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
    pub fn to_rgb(&self) -> Vec<u8> {
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

/// Settings for [`write_mp4`].
pub struct Mp4Options {
    pub ffmpeg: PathBuf,
    /// Integer upscale factor (nearest neighbor, so pixel art stays crisp).
    pub scale: u32,
    /// Shape of one source pixel, e.g. (1, 1) for square or (8, 7) for the
    /// Genesis 256-pixel mode on a TV.
    pub pixel_aspect: (u32, u32),
}

/// Find ffmpeg: an explicit path, then `$FFMPEG`, then `Tools/FFMPEG/bin`
/// in or next to the current folder, then whatever `ffmpeg` is on PATH.
pub fn find_ffmpeg(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    if let Some(p) = std::env::var_os("FFMPEG") {
        return PathBuf::from(p);
    }
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    ["Tools/FFMPEG/bin", "../Tools/FFMPEG/bin", "../../Tools/FFMPEG/bin"]
        .iter()
        .map(|dir| Path::new(dir).join(exe))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// Pick a constant frame rate for ffmpeg. If every frame lasts the same whole
/// number of frames-per-second (8, 10, 12...), use that; otherwise use 60 fps
/// and repeat frames to match each one's duration.
fn choose_fps(durations: &[f64]) -> u32 {
    let first = durations.first().copied().unwrap_or(0.0);
    let fps = (1000.0 / first).round();
    let uniform = first > 0.0
        && (1.0..=60.0).contains(&fps)
        && durations.iter().all(|&d| (d - 1000.0 / fps).abs() < 0.5);
    if uniform { fps as u32 } else { 60 }
}

/// Encode frames (and optional unsigned 8-bit mono sound) to H.264/AAC MP4
/// by piping raw RGB into ffmpeg.
pub fn write_mp4(
    path: &Path,
    width: usize,
    height: usize,
    frames: &[AnimFrame],
    sound: Option<(u32, &[u8])>,
    opts: &Mp4Options,
) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    anyhow::ensure!(!frames.is_empty(), "no frames to write");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Frames with no timing (some PACo movies) get 1/12 s.
    let durations: Vec<f64> =
        frames.iter().map(|f| if f.duration_ms > 0.0 { f.duration_ms } else { 1000.0 / 12.0 }).collect();
    let fps = choose_fps(&durations);

    // ffmpeg reads video from stdin, so the sound goes through a temp file.
    let wav = match sound {
        Some((rate, samples)) if !samples.is_empty() => {
            let p = std::env::temp_dir().join(format!(
                "helix-{}-{}.wav",
                std::process::id(),
                path.file_stem().and_then(|s| s.to_str()).unwrap_or("audio")
            ));
            write_wav_u8(&p, rate, samples)?;
            Some(p)
        }
        _ => None,
    };

    let (num, den) = opts.pixel_aspect;
    let filters = format!(
        "scale=iw*{s}:ih*{s}:flags=neighbor,pad=ceil(iw/2)*2:ceil(ih/2)*2,setsar={num}/{den},format=yuv420p",
        s = opts.scale.max(1)
    );
    let mut cmd = Command::new(&opts.ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{width}x{height}")])
        .args(["-framerate", &fps.to_string(), "-i", "-"]);
    if let Some(w) = &wav {
        cmd.arg("-i").arg(w);
    }
    cmd.args(["-vf", &filters])
        .args(["-c:v", "libx264", "-preset", "slow", "-crf", "16", "-tune", "animation"]);
    if wav.is_some() {
        cmd.args(["-c:a", "aac", "-b:a", "128k", "-ar", "48000"]);
    }
    cmd.args(["-movflags", "+faststart"]).arg(path).stdin(Stdio::piped());

    let mut child = cmd.spawn().with_context(|| {
        format!(
            "running {} (pass --ffmpeg <path> or set FFMPEG)",
            opts.ffmpeg.display()
        )
    })?;
    {
        let mut stdin = BufWriter::new(child.stdin.take().context("ffmpeg stdin")?);
        // Show each frame for its share of the timeline, rounding on the
        // running total so long movies don't drift.
        let (mut elapsed_ms, mut shown) = (0.0, 0u64);
        for (f, d) in frames.iter().zip(&durations) {
            elapsed_ms += d;
            let until = (elapsed_ms * fps as f64 / 1000.0).round() as u64;
            let rgb = f.pixels.to_rgb();
            while shown < until {
                stdin.write_all(&rgb)?;
                shown += 1;
            }
        }
        stdin.flush()?;
    }
    let status = child.wait()?;
    if let Some(w) = wav {
        let _ = fs::remove_file(w);
    }
    anyhow::ensure!(status.success(), "ffmpeg failed ({status}) writing {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::choose_fps;

    #[test]
    fn uniform_frame_rates_are_kept() {
        assert_eq!(choose_fps(&[125.0; 10]), 8); // PACo -8 delay
        assert_eq!(choose_fps(&[1000.0 / 12.0; 5]), 12); // PACo 5 ticks
        assert_eq!(choose_fps(&[100.0; 3]), 10); // Sega FILM 60/600
    }

    #[test]
    fn uneven_timing_falls_back_to_60() {
        assert_eq!(choose_fps(&[100.0, 100.0, 250.0]), 60);
        assert_eq!(choose_fps(&[]), 60);
    }
}
