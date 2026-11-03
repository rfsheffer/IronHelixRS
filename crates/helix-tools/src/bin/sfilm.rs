//! `sfilm`: inspect and convert Sega CD FILM movies (`.SEG` in Iron Helix).
//!
//! ```text
//! sfilm info   <file>                 container and frame summary
//! sfilm frames <file> -o <dir>        one PNG per frame
//! sfilm apng   <file> -o <file.apng>  animated PNG (plus a .wav next to it if there is sound)
//! sfilm export <dir>  -o <dir>        every movie under a folder, same layout
//! sfilm check  <dir>                  decode everything, report failures
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use helix_tools::output::{self, AnimFrame, Pixels, collect_files, mirrored};
use segafilm::{Film, SampleKind, audio, decode_film};

#[derive(Parser)]
#[command(about = "Inspect and convert Sega CD FILM movies")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the container header and a frame summary.
    Info { file: PathBuf },
    /// Write each frame as a PNG.
    Frames {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write an animated PNG, plus a WAV when the movie has sound.
    Apng {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Convert every movie under a folder, mirroring the layout.
    Export {
        dir: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Decode every movie under a folder and summarize.
    Check { dir: PathBuf },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Info { file } => info(&file),
        Command::Frames { file, out } => frames(&file, &out),
        Command::Apng { file, out } => {
            let data = read(&file)?;
            let n = convert(&data, &out)?;
            println!("wrote {n} frames to {}", out.display());
            Ok(())
        }
        Command::Export { dir, out } => export(&dir, &out),
        Command::Check { dir } => check(&dir),
    }
}

fn read(file: &Path) -> Result<Vec<u8>> {
    fs::read(file).with_context(|| format!("reading {}", file.display()))
}

/// The movie's sound as unsigned 8-bit PCM, if it has any.
fn sound(film: &Film) -> Option<Vec<u8>> {
    let raw = segafilm::film_audio(film);
    (!raw.is_empty()).then(|| audio::to_unsigned(&raw))
}

fn info(file: &Path) -> Result<()> {
    let data = read(file)?;
    let film = Film::parse(&data)?;
    let video: Vec<_> = film.video().collect();
    let audio_bytes: usize = film.audio().map(|s| s.len).sum();
    println!("{}", file.display());
    println!("  codec '{}', {}x{}, time base {} Hz", film.fourcc_str(), film.width, film.height, film.time_base);
    println!("  {} video frames, {} audio chunks ({} bytes)", video.len(), film.audio().count(), audio_bytes);
    if let Some(last) = video.last()
        && let SampleKind::Video { timestamp, duration, .. } = last.kind
    {
        let secs = (timestamp + duration) as f64 / film.time_base.max(1) as f64;
        println!("  video length {secs:.2} s, audio length {:.2} s", audio_bytes as f64 / audio::SAMPLE_RATE as f64);
    }
    let decoded = decode_film(&film)?;
    let sizes: Vec<usize> = decoded.frames.iter().map(|(f, _)| f.encoded_len).collect();
    println!(
        "  {} frames decoded ({} damaged table entries held the previous frame); encoded size {}..{} bytes",
        decoded.frames.len(),
        decoded.damaged,
        sizes.iter().min().unwrap_or(&0),
        sizes.iter().max().unwrap_or(&0)
    );
    Ok(())
}

fn frames(file: &Path, out: &Path) -> Result<()> {
    let data = read(file)?;
    let film = Film::parse(&data)?;
    let decoded = decode_film(&film)?;
    for (i, (f, _)) in decoded.frames.iter().enumerate() {
        output::write_png(&out.join(format!("frame_{i:04}.png")), f.width, f.height, &Pixels::Rgb(&f.rgb))?;
    }
    println!("wrote {} frames to {}", decoded.frames.len(), out.display());
    Ok(())
}

/// Write `<out>` as APNG and, if there is sound, `<out minus extension>.wav`.
/// Returns the frame count.
fn convert(data: &[u8], out: &Path) -> Result<usize> {
    let film = Film::parse(data)?;
    let decoded = decode_film(&film)?;
    let Some((first, _)) = decoded.frames.first() else {
        bail!("movie has no video frames");
    };
    let anim: Vec<AnimFrame> = decoded
        .frames
        .iter()
        .map(|(f, ms)| AnimFrame {
            pixels: Pixels::Rgb(&f.rgb),
            duration_ms: *ms,
        })
        .collect();
    output::write_apng(out, first.width, first.height, &anim)?;
    if let Some(pcm) = sound(&film) {
        output::write_wav_u8(&out.with_extension("wav"), audio::SAMPLE_RATE, &pcm)?;
    }
    Ok(decoded.frames.len())
}

fn is_film(data: &[u8]) -> bool {
    data.starts_with(b"FILM")
}

fn export(dir: &Path, out: &Path) -> Result<()> {
    let (mut written, mut skipped, mut failed) = (0, 0, 0);
    for file in collect_files(dir)? {
        let data = read(&file)?;
        if !is_film(&data) {
            skipped += 1;
            continue;
        }
        // NAV/L1/1_01_E.SEG -> NAV/L1/1_01_E.apng (+ .wav)
        let dest = mirrored(out, dir, &file.with_extension(""), ".apng")?;
        match convert(&data, &dest) {
            Ok(_) => written += 1,
            Err(e) => {
                failed += 1;
                eprintln!("{}: {e:#}", file.display());
            }
        }
    }
    println!(
        "exported {written} movies to {}, {failed} failed, skipped {skipped} other files",
        out.display()
    );
    Ok(())
}

fn check(dir: &Path) -> Result<()> {
    let (mut ok, mut bad, mut frames_total, mut damaged) = (0, 0, 0, 0);
    for file in collect_files(dir)? {
        let data = read(&file)?;
        if !is_film(&data) {
            continue;
        }
        let result = Film::parse(&data).map_err(anyhow::Error::from).and_then(|film| Ok(decode_film(&film)?));
        match result {
            Ok(decoded) => {
                ok += 1;
                frames_total += decoded.frames.len();
                if decoded.damaged > 0 {
                    damaged += 1;
                    println!("{}: {} damaged table entries", file.display(), decoded.damaged);
                }
            }
            Err(e) => {
                bad += 1;
                println!("{}: {e:#}", file.display());
            }
        }
    }
    println!("{ok} movies decoded ({frames_total} frames; {damaged} movies with damaged tables), {bad} failed");
    Ok(())
}
