//! `xref`: find which movies in one release look like movies in others.
//!
//! Each movie gets a perceptual fingerprint: up to 24 evenly spaced frames,
//! each shrunk to an 8x8 grid of brightness, normalized so palette and
//! brightness differences between releases matter little. A query movie's
//! score against a candidate is the average, over its sampled frames, of the
//! distance to the closest candidate frame: low means "everything in this
//! movie also appears in that one", regardless of frame rate or length.
//!
//! ```text
//! xref <query dir> --against <dir> [--against <dir> ...] > matches.tsv
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use clap::Parser;
use helix_tools::movies::{self, Clip};
use helix_tools::output::collect_files;

#[derive(Parser)]
#[command(about = "Match movies between Iron Helix releases by how they look")]
struct Cli {
    /// Folder of movies to look up.
    query: PathBuf,
    /// Folder(s) of movies to compare against.
    #[arg(long, required = true)]
    against: Vec<PathBuf>,
    /// How many best matches to list per movie.
    #[arg(long, default_value_t = 3)]
    top: usize,
}

const SAMPLES: usize = 24;
const GRID: usize = 8;
const DIMS: usize = GRID * GRID + 1;

struct Print {
    path: PathBuf,
    width: usize,
    height: usize,
    frames: usize,
    seconds: f64,
    samples: Vec<[f32; DIMS]>,
}

/// Shrink one RGB frame to a normalized brightness grid (+ mean brightness).
fn frame_print(rgb: &[u8], w: usize, h: usize) -> [f32; DIMS] {
    let mut sum = [0f32; GRID * GRID];
    let mut count = [0f32; GRID * GRID];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 3;
            let luma = 0.299 * rgb[o] as f32 + 0.587 * rgb[o + 1] as f32 + 0.114 * rgb[o + 2] as f32;
            let cell = (y * GRID / h) * GRID + x * GRID / w;
            sum[cell] += luma;
            count[cell] += 1.0;
        }
    }
    let cells: Vec<f32> = sum.iter().zip(&count).map(|(s, c)| s / c.max(1.0)).collect();
    let mean = cells.iter().sum::<f32>() / cells.len() as f32;
    let std = (cells.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / cells.len() as f32).sqrt();
    let mut out = [0f32; DIMS];
    for (o, v) in out.iter_mut().zip(&cells) {
        // The +10 keeps nearly flat frames (black screens) from blowing up noise.
        *o = (v - mean) / (std + 10.0);
    }
    out[GRID * GRID] = mean / 32.0;
    out
}

fn movie_print(path: &Path, clip: &Clip) -> Print {
    let n = clip.frames.len();
    let picks: Vec<usize> = if n <= SAMPLES { (0..n).collect() } else { (0..SAMPLES).map(|i| i * n / SAMPLES).collect() };
    Print {
        path: path.to_path_buf(),
        width: clip.width,
        height: clip.height,
        frames: n,
        seconds: clip.duration_ms() / 1000.0,
        samples: picks.iter().map(|&i| frame_print(&clip.frames[i].0, clip.width, clip.height)).collect(),
    }
}

fn distance(a: &[f32; DIMS], b: &[f32; DIMS]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>().sqrt()
}

/// How well `candidate` covers everything in `query` (lower is better).
fn score(query: &Print, candidate: &Print) -> f32 {
    let total: f32 = query
        .samples
        .iter()
        .map(|q| candidate.samples.iter().map(|c| distance(q, c)).fold(f32::INFINITY, f32::min))
        .sum();
    total / query.samples.len().max(1) as f32
}

/// Fingerprint every movie under `dir`, decoding in parallel.
fn prints(dir: &Path) -> Result<Vec<Print>> {
    let files = collect_files(dir)?;
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut all: Vec<Print> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = files.get(i) else { break };
                        let Ok(data) = std::fs::read(file) else { continue };
                        let Some(kind) = movies::detect(&data) else { continue };
                        if let Ok(clip) = movies::load(&data, kind)
                            && !clip.frames.is_empty()
                        {
                            mine.push(movie_print(file, &clip));
                        }
                    }
                    mine
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    all.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(all)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let queries = prints(&cli.query)?;
    let mut refs = Vec::new();
    for dir in &cli.against {
        refs.extend(prints(dir)?);
    }
    eprintln!("{} query movies, {} reference movies", queries.len(), refs.len());

    print!("path\twidth\theight\tframes\tseconds");
    for k in 1..=cli.top {
        print!("\tmatch{k}\tscore{k}");
    }
    println!();

    for q in &queries {
        let aspect = q.width as f32 / q.height as f32;
        let mut scored: Vec<(f32, &Print)> = refs
            .iter()
            // Only compare movies of roughly the same shape.
            .filter(|r| ((r.width as f32 / r.height as f32) / aspect - 1.0).abs() < 0.25)
            .map(|r| (score(q, r), r))
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        print!(
            "{}\t{}\t{}\t{}\t{:.2}",
            q.path.strip_prefix(&cli.query).unwrap_or(&q.path).display(),
            q.width,
            q.height,
            q.frames,
            q.seconds
        );
        for (s, r) in scored.iter().take(cli.top) {
            print!("\t{}\t{s:.3}", r.path.display());
        }
        println!();
    }
    Ok(())
}
