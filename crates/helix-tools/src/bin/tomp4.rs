//! `tomp4`: convert Iron Helix movies from any release to MP4 (via ffmpeg).
//!
//! Detects the format by content, so it works on the Windows and Mac PACo
//! movies (with or without file extensions) and on Sega CD FILM movies.
//!
//! ```text
//! tomp4 <movie>  -o <file.mp4>      one movie
//! tomp4 <folder> -o <folder>        every movie under a folder, same layout
//! ```
//!
//! ffmpeg is found via `--ffmpeg`, then `$FFMPEG`, then `Tools/FFMPEG/bin`
//! in or next to the current folder, then PATH.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use clap::Parser;
use helix_tools::output::{self, AnimFrame, Mp4Options, Pixels, collect_files};

#[derive(Parser)]
#[command(about = "Convert Iron Helix movies (PC, Mac or Sega CD) to MP4 with ffmpeg")]
struct Cli {
    /// A movie file, or a folder to convert everything under.
    input: PathBuf,
    /// Output .mp4 file (for one movie) or folder (for a folder).
    #[arg(short, long)]
    out: PathBuf,
    /// Path to ffmpeg.
    #[arg(long)]
    ffmpeg: Option<PathBuf>,
    /// Integer upscale factor (nearest neighbor).
    #[arg(long, default_value_t = 4)]
    scale: u32,
    /// Encodes to run at once when converting a folder (default: CPU count).
    #[arg(long)]
    jobs: Option<usize>,
}

/// A decoded movie, ready to encode.
struct Clip {
    width: usize,
    height: usize,
    /// RGB frames with durations in milliseconds.
    frames: Vec<(Vec<u8>, f64)>,
    /// Unsigned 8-bit mono sound and its sample rate.
    sound: Option<(u32, Vec<u8>)>,
    /// Shape of one source pixel.
    pixel_aspect: (u32, u32),
}

enum Kind {
    Paco,
    SegaFilm,
}

fn detect(data: &[u8]) -> Option<Kind> {
    if data.starts_with(b"FILM") {
        Some(Kind::SegaFilm)
    } else if paco::Movie::parse(data).is_ok() {
        Some(Kind::Paco)
    } else {
        None
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

fn convert(data: &[u8], kind: Kind, out: &Path, ffmpeg: &Path, scale: u32) -> Result<usize> {
    let clip = match kind {
        Kind::Paco => load_paco(data)?,
        Kind::SegaFilm => load_film(data)?,
    };
    let frames: Vec<AnimFrame> = clip
        .frames
        .iter()
        .map(|(rgb, ms)| AnimFrame {
            pixels: Pixels::Rgb(rgb),
            duration_ms: *ms,
        })
        .collect();
    let opts = Mp4Options {
        ffmpeg: ffmpeg.to_path_buf(),
        scale,
        pixel_aspect: clip.pixel_aspect,
    };
    let sound = clip.sound.as_ref().map(|(rate, s)| (*rate, s.as_slice()));
    output::write_mp4(out, clip.width, clip.height, &frames, sound, &opts)?;
    Ok(frames.len())
}

/// Output path for one movie in a folder conversion: `ENDING.SEG` becomes
/// `ENDING.mp4`; extensionless PACo names like `1.01.T.4` keep their dots.
fn mp4_name(out: &Path, base: &Path, file: &Path) -> Result<PathBuf> {
    let rel = file.strip_prefix(base)?;
    let is_seg = rel.extension().is_some_and(|e| e.eq_ignore_ascii_case("seg"));
    let rel = if is_seg { rel.with_extension("") } else { rel.to_path_buf() };
    let mut dest = out.join(rel).into_os_string();
    dest.push(".mp4");
    Ok(PathBuf::from(dest))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ffmpeg = output::find_ffmpeg(cli.ffmpeg.as_deref());

    if cli.input.is_file() {
        let data = fs::read(&cli.input).with_context(|| format!("reading {}", cli.input.display()))?;
        let Some(kind) = detect(&data) else {
            bail!("{} is not a PACo or Sega FILM movie", cli.input.display());
        };
        let n = convert(&data, kind, &cli.out, &ffmpeg, cli.scale)?;
        println!("wrote {n} frames to {}", cli.out.display());
        return Ok(());
    }

    // Folder: find the movies first, then encode them in parallel.
    let mut jobs = Vec::new();
    let mut skipped = 0;
    for file in collect_files(&cli.input)? {
        let data = fs::read(&file)?;
        match detect(&data) {
            Some(_) => jobs.push(file),
            None => skipped += 1,
        }
    }
    let total = jobs.len();
    let workers = cli
        .jobs
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()))
        .max(1);
    println!("converting {total} movies with {workers} parallel encodes ({skipped} other files skipped)");

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(file) = jobs.get(i) else { break };
                    let result = (|| -> Result<()> {
                        let data = fs::read(file)?;
                        let kind = detect(&data).context("no longer a movie")?;
                        convert(&data, kind, &mp4_name(&cli.out, &cli.input, file)?, &ffmpeg, cli.scale)?;
                        Ok(())
                    })();
                    if let Err(e) = result {
                        failures.lock().unwrap().push(format!("{}: {e:#}", file.display()));
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(100) || n == total {
                        println!("  {n}/{total}");
                    }
                }
            });
        }
    });

    let failures = failures.into_inner().unwrap();
    for f in &failures {
        eprintln!("{f}");
    }
    println!("wrote {} MP4s to {}, {} failed", total - failures.len(), cli.out.display(), failures.len());
    Ok(())
}
