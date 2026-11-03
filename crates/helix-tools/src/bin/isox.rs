//! `isox`: list and extract ISO9660 CD images (raw 2352-byte or cooked 2048-byte sectors).
//!
//! ```text
//! isox ls      "Iron Helix (US).img"
//! isox extract "Iron Helix (US).img" -o out/sega/files
//! ```

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use cdimage::Disc;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "List and extract ISO9660 CD images")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List every file with its size and starting sector.
    Ls { image: PathBuf },
    /// Extract every file, keeping the directory layout.
    Extract {
        image: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
}

fn open(path: &PathBuf) -> Result<Disc<fs::File>> {
    let f = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(Disc::open(f)?)
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Ls { image } => {
            let mut disc = open(&image)?;
            println!("system '{}', volume '{}'", disc.system_id, disc.volume_id);
            for e in disc.entries()?.iter().filter(|e| !e.is_dir) {
                println!("{:>10} @{:<7} {}", e.len, e.sector, e.path);
            }
        }
        Command::Extract { image, out } => {
            let mut disc = open(&image)?;
            let entries = disc.entries()?;
            let mut count = 0;
            for e in &entries {
                let dest = out.join(e.path.trim_start_matches('/'));
                if e.is_dir {
                    fs::create_dir_all(&dest)?;
                    continue;
                }
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&dest, disc.read(e.sector, e.len)?)?;
                count += 1;
            }
            println!("extracted {count} files from '{}' to {}", disc.volume_id, out.display());
        }
    }
    Ok(())
}
