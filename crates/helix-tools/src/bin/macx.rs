//! `macx`: get files out of classic Mac containers.
//!
//! The Mac release of Iron Helix arrived as a StuffIt 5 archive holding a
//! compressed DiskCopy 6 (NDIF) image of an HFS CD. Getting at the files is
//! three steps:
//!
//! ```text
//! macx sit-extract Iron-Helix_Mac_EN.sit -o out/mac --rsrc-only   recover the image's resource fork
//! macx ndif "Iron Helix™ CD.img" --rsrc "out/mac/Iron Helix™ CD.img.rsrc" -o out/mac/helix.hfs
//! macx hfs-extract out/mac/helix.hfs -o out/mac/files
//! ```

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use macformats::{hfs, ndif, rsrc, sit5};

#[derive(Parser)]
#[command(about = "Extract files from StuffIt 5 archives, DiskCopy images and HFS volumes")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List a StuffIt 5 archive.
    Sit { archive: PathBuf },
    /// Extract a StuffIt 5 archive. Resource forks are written as `<name>.rsrc`.
    SitExtract {
        archive: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Only extract resource forks (useful when the data fork was already
        /// extracted by a tool that dropped the resource fork).
        #[arg(long)]
        rsrc_only: bool,
    },
    /// Expand a DiskCopy 6 (NDIF) image to a raw disk image.
    Ndif {
        /// The image's data fork.
        image: PathBuf,
        /// The image's resource fork (holds the chunk map).
        #[arg(long)]
        rsrc: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// List the files on a raw HFS volume.
    HfsLs { volume: PathBuf },
    /// Extract every file from a raw HFS volume. Resource forks are written as
    /// `<name>.rsrc` and Finder info goes to `_finder.tsv`.
    HfsExtract {
        volume: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Sit { archive } => sit_list(&archive),
        Command::SitExtract { archive, out, rsrc_only } => sit_extract(&archive, &out, rsrc_only),
        Command::Ndif { image, rsrc, out } => ndif_expand(&image, &rsrc, &out),
        Command::HfsLs { volume } => hfs_ls(&volume),
        Command::HfsExtract { volume, out } => hfs_extract(&volume, &out),
    }
}

fn four_cc(b: &[u8; 4]) -> String {
    macformats::macroman::decode(b)
}

fn sit_list(path: &Path) -> Result<()> {
    let data = fs::read(path)?;
    let archive = sit5::Archive::parse(&data)?;
    for e in &archive.entries {
        println!("{}  [{}/{}]", e.name, four_cc(&e.file_type), four_cc(&e.creator));
        println!("  data: {} bytes ({} packed, method {})", e.data.len, e.data.compressed_len, e.data.method);
        if let Some(r) = &e.rsrc {
            println!("  rsrc: {} bytes ({} packed, method {})", r.len, r.compressed_len, r.method);
        }
    }
    Ok(())
}

fn sit_extract(path: &Path, out: &Path, rsrc_only: bool) -> Result<()> {
    let data = fs::read(path)?;
    let archive = sit5::Archive::parse(&data)?;
    fs::create_dir_all(out)?;
    for e in &archive.entries {
        let name = safe_name(&e.name);
        if let Some(r) = &e.rsrc {
            let bytes = archive.extract(r).with_context(|| format!("resource fork of {}", e.name))?;
            fs::write(out.join(format!("{name}.rsrc")), bytes)?;
            println!("{name}.rsrc: {} bytes", r.len);
        }
        if !rsrc_only {
            let bytes = archive.extract(&e.data).with_context(|| format!("data fork of {}", e.name))?;
            fs::write(out.join(&name), bytes)?;
            println!("{name}: {} bytes", e.data.len);
        }
    }
    Ok(())
}

fn ndif_expand(image: &Path, rsrc_path: &Path, out: &Path) -> Result<()> {
    let fork = fs::read(rsrc_path)?;
    let resources = rsrc::parse(&fork)?;
    let bcem = resources
        .iter()
        .find(|r| &r.kind == b"bcem")
        .context("resource fork has no 'bcem' chunk map; is this a DiskCopy 6 image?")?;
    let map = ndif::ChunkMap::parse(&bcem.data)?;
    println!(
        "'{}': {} sectors ({} MB) in {} chunks",
        map.name,
        map.sectors,
        map.sectors as usize * ndif::SECTOR / 1_000_000,
        map.chunks.len()
    );

    let data = fs::read(image)?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut w = BufWriter::new(fs::File::create(out)?);
    let mut written = 0usize;
    ndif::expand(&map, &data, |chunk| {
        written += chunk.len();
        w.write_all(chunk)?;
        Ok(())
    })?;
    w.flush()?;
    println!("wrote {written} bytes to {}", out.display());
    Ok(())
}

fn open_volume(path: &Path) -> Result<hfs::Volume<fs::File>> {
    let f = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(hfs::Volume::open(f)?)
}

/// Mac path of a file, using `/` as the separator for display.
fn mac_path(vol: &hfs::Volume<fs::File>, file: &hfs::File) -> String {
    let mut parts = vol.folder_path(file.parent);
    parts.push(file.name.clone());
    parts.join("/")
}

fn hfs_ls(path: &Path) -> Result<()> {
    let vol = open_volume(path)?;
    println!("volume '{}': {} folders, {} files", vol.name, vol.folders.len(), vol.files.len());
    let mut lines: Vec<_> = vol
        .files
        .iter()
        .map(|f| {
            format!(
                "{:>10} {:>8}  {}/{}  {}",
                f.data.len,
                f.rsrc.len,
                four_cc(&f.file_type),
                four_cc(&f.creator),
                mac_path(&vol, f)
            )
        })
        .collect();
    lines.sort_by(|a, b| a[30..].cmp(&b[30..]));
    println!("{:>10} {:>8}  type/crtr  path", "data", "rsrc");
    for l in lines {
        println!("{l}");
    }
    Ok(())
}

fn hfs_extract(path: &Path, out: &Path) -> Result<()> {
    let mut vol = open_volume(path)?;
    fs::create_dir_all(out)?;
    let mut manifest = String::from("path\ttype\tcreator\tfinder_flags\tdata_len\trsrc_len\n");

    let files = vol.files.clone();
    for file in &files {
        let rel: PathBuf = vol
            .folder_path(file.parent)
            .iter()
            .map(|p| safe_name(p))
            .chain(std::iter::once(safe_name(&file.name)))
            .collect();
        let dest = out.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        if dest.exists() {
            bail!("two Mac files map to the same Windows name: {}", rel.display());
        }

        fs::write(&dest, vol.read_data(file)?)?;
        if file.rsrc.len > 0 {
            let mut r = dest.clone().into_os_string();
            r.push(".rsrc");
            fs::write(r, vol.read_rsrc(file)?)?;
        }
        manifest.push_str(&format!(
            "{}\t{}\t{}\t{:#06x}\t{}\t{}\n",
            rel.display(),
            four_cc(&file.file_type),
            four_cc(&file.creator),
            file.finder_flags,
            file.data.len,
            file.rsrc.len
        ));
    }
    fs::write(out.join("_finder.tsv"), manifest)?;
    println!("extracted {} files from '{}' to {}", files.len(), vol.name, out.display());
    Ok(())
}

/// Make a Mac file name safe on Windows: no reserved characters, no
/// trailing dots or spaces, no reserved device names.
fn safe_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    if s.ends_with('.') || s.ends_with(' ') {
        s.pop();
        s.push('_');
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) || s.is_empty() {
        s.insert(0, '_');
    }
    s
}
