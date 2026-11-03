//! `paco`: inspect and convert PACo movies.
//!
//! ```text
//! paco info   <file>                 header and opcode listing
//! paco frames <file> -o <dir>        one PNG per frame
//! paco gif    <file> -o <file.gif>   animated GIF preview
//! paco apng   <file> -o <file.apng>  lossless animated PNG
//! paco export <dir>  -o <dir>        every movie under a folder to APNG, same layout
//! paco check  <dir>                  decode every movie under a folder, report failures
//! ```

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use paco::decoder::{DecoderOptions, RowOrder};
use paco::{Decoder, Frame, Movie, Op, Palette};

#[derive(Parser)]
#[command(about = "Inspect and convert PACo (CAVF) movies")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Row order to assume for interlaced bitmaps.
    #[arg(long, global = true, value_enum, default_value_t = Order::EvenThenOdd)]
    interlace: Order,
}

#[derive(Subcommand)]
enum Command {
    /// Print the header and every opcode.
    Info { file: PathBuf },
    /// Write each frame as an indexed PNG.
    Frames {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write an animated GIF.
    Gif {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write a lossless animated PNG.
    Apng {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Convert every movie under a folder to APNG, mirroring the layout.
    Export {
        dir: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Decode every movie under a folder and summarize.
    Check { dir: PathBuf },
    /// Print a hash of each movie's decoded pixels (TSV), for comparing releases.
    Digest { dir: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
enum Order {
    Sequential,
    EvenThenOdd,
}

impl From<Order> for RowOrder {
    fn from(o: Order) -> Self {
        match o {
            Order::Sequential => RowOrder::Sequential,
            Order::EvenThenOdd => RowOrder::EvenThenOdd,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let opts = DecoderOptions {
        interlaced_order: cli.interlace.into(),
        ..Default::default()
    };

    match cli.command {
        Command::Info { file } => info(&file, opts),
        Command::Frames { file, out } => frames(&file, &out, opts),
        Command::Gif { file, out } => gif(&file, &out, opts),
        Command::Apng { file, out } => {
            let frames = decode(&file, opts)?;
            write_apng(&out, &frames)?;
            println!("wrote {} frames to {}", frames.len(), out.display());
            Ok(())
        }
        Command::Export { dir, out } => export(&dir, &out, opts),
        Command::Check { dir } => check(&dir, opts),
        Command::Digest { dir } => digest(&dir, opts),
    }
}

/// One line per movie: path, size, frame count, a hash of all decoded frames
/// (pixels mapped through their palette), and whether it has sound.
fn digest(dir: &Path, opts: DecoderOptions) -> Result<()> {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut files = Vec::new();
    collect_files(dir, &mut files)?;
    files.sort();
    println!("path\twidth\theight\tframes\tpixels_hash\tunique_frames_hash\thas_audio");
    for file in &files {
        let Ok(movie) = Movie::parse(&fs::read(file)?) else {
            continue;
        };
        let frames = Decoder::new(&movie, opts.clone()).decode_all()?;
        let rgb = |f: &Frame| -> Vec<u8> {
            f.pixels
                .iter()
                .flat_map(|&i| {
                    let c = f.palette.0[i as usize];
                    [c.r, c.g, c.b]
                })
                .collect()
        };
        // Hash of every frame, and of the frames with consecutive repeats
        // removed (so a different frame rate with the same images still matches).
        let (mut all, mut unique) = (DefaultHasher::new(), DefaultHasher::new());
        let mut prev: Option<Vec<u8>> = None;
        for f in &frames {
            let px = rgb(f);
            px.hash(&mut all);
            if prev.as_ref() != Some(&px) {
                px.hash(&mut unique);
            }
            prev = Some(px);
        }
        println!(
            "{}\t{}\t{}\t{}\t{:016x}\t{:016x}\t{}",
            file.strip_prefix(dir)?.display(),
            movie.width(),
            movie.height(),
            frames.len(),
            all.finish(),
            unique.finish(),
            movie.audio().is_some()
        );
    }
    Ok(())
}

fn load(file: &Path) -> Result<Movie> {
    let data = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    Movie::parse(&data).with_context(|| format!("parsing {}", file.display()))
}

fn info(file: &Path, opts: DecoderOptions) -> Result<()> {
    let movie = load(file)?;
    let h = &movie.header;
    println!("{}", file.display());
    println!("  version {}.{}, packer {:#06x}", h.version_maj, h.version_min, h.packer_version);
    println!("  {}x{}, {} frames, {} chunks, depth {}", h.width, h.height, h.num_frames, h.num_chunks, h.bit_depth);
    println!("  default delay {} ({:.1} ms/frame)", h.default_delay, h.default_frame_ms());
    println!(
        "  bits1 {:#06x} bits2 {:#06x} bits3 {:#06x} bits4 {:#04x}",
        h.bits1, h.bits2, h.bits3, h.bits4
    );
    println!(
        "  interlaced {} indirect {} smaller_packing {} loop {} sound_alloc {}",
        h.interlaced(), h.indirect(), h.smaller_packing(), h.looping(), h.sound_allocation
    );

    for (ci, chunk) in movie.chunks.iter().enumerate() {
        println!(
            "chunk {ci} @ {:#x}: {} bytes{}{}",
            chunk.offset,
            chunk.header.len(),
            if chunk.header.sound_only() { " sound-only" } else { "" },
            if chunk.header.skippable() { " skippable" } else { "" },
        );
        for op in &chunk.ops {
            match op {
                Op::Bitmap(b) => println!(
                    "  BITMAP {}x{}+{}+{} {:?}{}{} ({} bytes)",
                    b.width, b.height, b.left, b.top, b.compression(),
                    if b.diffs_only() { " diffs" } else { "" },
                    if b.lut_encoded() { " lut" } else { "" },
                    b.data.len()
                ),
                Op::Palette(p) => println!(
                    "  PALETTE {:?} when {} time {} colors {} (num_colors {})",
                    p.palette_type(), p.when(), p.time, p.colors.len(), p.num_colors
                ),
                Op::Delay(d) => println!("  DELAY {:.1} ms persist {}", d.frame_ms(), d.persist()),
                Op::Sound(s) => println!(
                    "  SOUND {} bytes rate {} defer {} flush {}",
                    s.data.len(), s.rate(), s.defer_till_frame, s.flush()
                ),
                Op::Transition(t) => println!("  TRANSITION kind {} time {}", t.kind, t.time),
                Op::Break { user_data } => println!("  BREAK {user_data:#x}"),
                Op::Unknown { kind, data } => println!("  UNKNOWN kind {kind} ({} bytes)", data.len()),
                other => println!("  {}", other.name()),
            }
        }
    }

    println!("decode:");
    for f in Decoder::new(&movie, opts).decode_all()? {
        match (&f.stats, &f.error) {
            (_, Some(e)) => println!("  frame {}: ERROR {e}", f.index),
            (Some(s), None) => println!(
                "  frame {}: ok, {} written {} skipped, end row {} x {}, {} lut lines, {} newline-after-wrap",
                f.index, s.pixels_written, s.pixels_skipped, s.end_row, s.end_x, s.lut_lines, s.newline_after_wrap
            ),
            (None, None) => println!("  frame {}: ok (uncompressed)", f.index),
        }
    }
    Ok(())
}

fn decode(file: &Path, opts: DecoderOptions) -> Result<Vec<Frame>> {
    let movie = load(file)?;
    Ok(Decoder::new(&movie, opts).decode_all()?)
}

fn frames(file: &Path, out: &Path, opts: DecoderOptions) -> Result<()> {
    fs::create_dir_all(out)?;
    let frames = decode(file, opts)?;
    for f in &frames {
        let path = out.join(format!("frame_{:04}.png", f.index));
        write_png(&path, f)?;
        if let Some(e) = &f.error {
            eprintln!("frame {}: {e}", f.index);
        }
    }
    println!("wrote {} frames to {}", frames.len(), out.display());
    Ok(())
}

fn write_png(path: &Path, f: &Frame) -> Result<()> {
    let file = BufWriter::new(fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, f.width as u32, f.height as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(f.palette.to_rgb_bytes());
    enc.write_header()?.write_image_data(&f.pixels)?;
    Ok(())
}

fn gif(file: &Path, out: &Path, opts: DecoderOptions) -> Result<()> {
    let frames = decode(file, opts)?;
    let first = frames.first().context("movie has no frames")?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let global = Palette::mac_system().to_rgb_bytes();
    let mut enc = gif::Encoder::new(
        BufWriter::new(fs::File::create(out)?),
        first.width as u16,
        first.height as u16,
        &global,
    )?;
    enc.set_repeat(gif::Repeat::Infinite)?;
    for f in &frames {
        let mut frame = gif::Frame::from_indexed_pixels(
            f.width as u16,
            f.height as u16,
            f.pixels.clone(),
            None,
        );
        frame.palette = Some(f.palette.to_rgb_bytes());
        frame.delay = (f.duration_ms / 10.0).round().max(1.0) as u16;
        enc.write_frame(&frame)?;
    }
    println!("wrote {} frames to {}", frames.len(), out.display());
    Ok(())
}

/// Write frames as an animated PNG. If every frame shares one palette the
/// file stays 8-bit indexed (pixel-exact and small); otherwise it is RGB.
fn write_apng(path: &Path, frames: &[Frame]) -> Result<()> {
    let first = frames.first().context("movie has no frames")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let one_palette = frames.iter().all(|f| f.palette == first.palette);

    let file = BufWriter::new(fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, first.width as u32, first.height as u32);
    enc.set_depth(png::BitDepth::Eight);
    if one_palette {
        enc.set_color(png::ColorType::Indexed);
        enc.set_palette(first.palette.to_rgb_bytes());
    } else {
        enc.set_color(png::ColorType::Rgb);
    }
    enc.set_animated(frames.len() as u32, 1)?; // play once
    let mut writer = enc.write_header()?;

    for f in frames {
        // Delays are stored as a fraction of a second.
        writer.set_frame_delay(f.duration_ms.round() as u16, 1000)?;
        if one_palette {
            writer.write_image_data(&f.pixels)?;
        } else {
            let rgb: Vec<u8> = f
                .pixels
                .iter()
                .flat_map(|&i| {
                    let c = f.palette.0[i as usize];
                    [c.r, c.g, c.b]
                })
                .collect();
            writer.write_image_data(&rgb)?;
        }
    }
    writer.finish()?;
    Ok(())
}

/// Write 8-bit mono PCM as a WAV file.
fn write_wav(path: &Path, audio: &paco::Audio) -> Result<()> {
    let len = audio.samples.len() as u32;
    let mut out = Vec::with_capacity(44 + audio.samples.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&audio.sample_rate.to_le_bytes());
    out.extend_from_slice(&audio.sample_rate.to_le_bytes()); // bytes per second
    out.extend_from_slice(&1u16.to_le_bytes()); // block align
    out.extend_from_slice(&8u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&audio.samples);
    fs::write(path, out)?;
    Ok(())
}

fn export(dir: &Path, out: &Path, opts: DecoderOptions) -> Result<()> {
    let mut files = Vec::new();
    collect_files(dir, &mut files)?;
    files.sort();

    let (mut written, mut skipped) = (0, 0);
    for file in &files {
        let data = fs::read(file)?;
        let Ok(movie) = Movie::parse(&data) else {
            skipped += 1;
            continue;
        };
        let frames = Decoder::new(&movie, opts.clone()).decode_all()?;
        if frames.is_empty() {
            skipped += 1;
            continue;
        }
        let rel = file.strip_prefix(dir)?;
        let base = out.join(rel).into_os_string();
        let mut apng = base.clone();
        apng.push(".apng");
        write_apng(Path::new(&apng), &frames)?;
        if let Some(audio) = movie.audio() {
            let mut wav = base;
            wav.push(".wav");
            write_wav(Path::new(&wav), &audio)?;
        }
        written += 1;
    }
    println!("exported {written} movies to {}, skipped {skipped} other files", out.display());
    Ok(())
}

fn check(dir: &Path, opts: DecoderOptions) -> Result<()> {
    let mut files = Vec::new();
    collect_files(dir, &mut files)?;
    files.sort();

    let (mut ok, mut bad, mut not_paco) = (0, 0, 0);
    // Frames that wrote more than one full phantom line below their area;
    // worth eyeballing in case they hide a decoder bug.
    let mut heavy_phantoms = Vec::new();
    for file in &files {
        let data = fs::read(file)?;
        let movie = match Movie::parse(&data) {
            Ok(m) => m,
            Err(_) => {
                not_paco += 1;
                continue;
            }
        };
        let frames = Decoder::new(&movie, opts.clone()).decode_all()?;
        for (f, op) in frames.iter().zip(movie.ops().filter_map(|op| match op {
            Op::Bitmap(b) => Some(b),
            _ => None,
        })) {
            if let Some(s) = &f.stats
                && s.phantom_pixels > op.width as usize
            {
                heavy_phantoms.push(format!("{} frame {}: {} phantom pixels", file.display(), f.index, s.phantom_pixels));
            }
        }
        let errors: Vec<_> = frames.iter().filter_map(|f| f.error.as_ref().map(|e| (f.index, e))).collect();
        if errors.is_empty() {
            ok += 1;
        } else {
            bad += 1;
            let (i, e) = errors[0];
            println!("{}: {} of {} frames failed, first at {i}: {e}", file.display(), errors.len(), frames.len());
        }
    }
    for line in &heavy_phantoms {
        println!("{line}");
    }
    println!("{ok} movies decoded cleanly, {bad} with errors, {not_paco} files were not PACo");
    println!("{} frames wrote more than one line below their area", heavy_phantoms.len());
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}
