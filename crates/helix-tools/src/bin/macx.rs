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
use macformats::{hfs, ndif, rsrc, sit1, sit5};

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
    /// Dump a Director 3 movie's text: Lingo scripts, data tables, other text
    /// members, score scripts and frame labels. Takes the resource fork of
    /// the movie or projector.
    Director {
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
        Command::Director { rsrc, out } => director_dump(&rsrc, &out),
        Command::HfsLs { volume } => hfs_ls(&volume),
        Command::HfsExtract { volume, out } => hfs_extract(&volume, &out),
    }
}

fn four_cc(b: &[u8; 4]) -> String {
    macformats::macroman::decode(b)
}

fn sit_list(path: &Path) -> Result<()> {
    let data = fs::read(path)?;
    if sit1::is_sit1(&data) {
        let archive = sit1::Archive::parse(&data)?;
        for e in &archive.entries {
            println!(
                "{}  [{}/{}]  data {} bytes (method {}), rsrc {} bytes (method {})",
                e.path.join("/"),
                four_cc(&e.file_type),
                four_cc(&e.creator),
                e.data.len,
                e.data.method,
                e.rsrc.len,
                e.rsrc.method
            );
        }
        return Ok(());
    }
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
    if sit1::is_sit1(&data) {
        return sit1_extract(&data, out, rsrc_only);
    }
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

/// Extract a classic StuffIt archive, keeping its folder layout.
fn sit1_extract(data: &[u8], out: &Path, rsrc_only: bool) -> Result<()> {
    let archive = sit1::Archive::parse(data)?;
    let mut manifest = String::from("path\ttype\tcreator\tfinder_flags\tdata_len\trsrc_len\n");
    for e in &archive.entries {
        let rel: PathBuf = e.path.iter().map(|p| safe_name(p)).collect();
        let dest = out.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        if e.rsrc.len > 0 {
            let mut r = dest.clone().into_os_string();
            r.push(".rsrc");
            fs::write(r, archive.extract(&e.rsrc).with_context(|| format!("resource fork of {}", rel.display()))?)?;
        }
        if !rsrc_only {
            fs::write(&dest, archive.extract(&e.data).with_context(|| format!("data fork of {}", rel.display()))?)?;
        }
        manifest.push_str(&format!(
            "{}\t{}\t{}\t{:#06x}\t{}\t{}\n",
            rel.display(),
            four_cc(&e.file_type),
            four_cc(&e.creator),
            e.finder_flags,
            e.data.len,
            e.rsrc.len
        ));
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("_finder.tsv"), manifest)?;
    println!("extracted {} files to {}", archive.entries.len(), out.display());
    Ok(())
}

fn director_dump(rsrc_path: &Path, out: &Path) -> Result<()> {
    use director::{Movie, TextKind};

    let fork = fs::read(rsrc_path).with_context(|| format!("reading {}", rsrc_path.display()))?;
    let movie = Movie::from_resource_fork(&fork)?;
    let mut index = String::from("cast\tkind\ttitle\tchars\tfile\n");

    let mut write = |folder: &str, ext: &str, m: &director::TextMember, kind: &str| -> Result<()> {
        let title = m.title().map(|t| safe_name(&t)).unwrap_or_default();
        let file = if title.is_empty() {
            format!("{folder}/{:04}.{ext}", m.cast)
        } else {
            format!("{folder}/{:04} {title}.{ext}", m.cast)
        };
        let path = out.join(&file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &m.text)?;
        index.push_str(&format!("{}\t{kind}\t{title}\t{}\t{file}\n", m.cast, m.text.chars().count()));
        Ok(())
    };

    let texts = movie.texts()?;
    for m in &texts {
        match m.kind() {
            TextKind::Script => write("scripts", "lingo", m, "script")?,
            TextKind::Table => write("tables", "txt", m, "table")?,
            TextKind::Text => write("text", "txt", m, "text")?,
        }
    }
    for m in &movie.member_scripts()? {
        write("member-scripts", "lingo", m, "member-script")?;
    }

    let actions = movie.actions()?;
    let mut s = String::new();
    for a in &actions {
        s.push_str(&format!("-- score script {}\n{}\n\n", a.id, a.text));
    }
    fs::write(out.join("score-scripts.lingo"), s)?;

    let labels = movie.labels()?;
    let s: String = labels.iter().map(|l| format!("{}\t{}\n", l.frame, l.name)).collect();
    fs::write(out.join("labels.tsv"), s)?;

    let names = movie.cast_names()?;
    let s: String = names.iter().map(|(c, n)| format!("{c}\t{n}\n")).collect();
    fs::write(out.join("cast-names.tsv"), s)?;
    fs::write(out.join("index.tsv"), index)?;

    // Bitmaps, drawn with the Mac system palette (all Iron Helix bitmaps use it).
    let palette = paco::Palette::mac_system().to_rgb_bytes();
    let bitmaps = movie.bitmaps()?;
    let mut table = String::from("cast\tname\twidth\theight\tleft\ttop\treg_h\treg_v\tbpp\tpalette\tfile\n");
    fs::create_dir_all(out.join("bitmaps"))?;
    for b in &bitmaps {
        let name = names.get(&b.cast).map(|n| safe_name(n)).unwrap_or_default();
        let file = if name.is_empty() { format!("bitmaps/{:04}.png", b.cast) } else { format!("bitmaps/{:04} {name}.png", b.cast) };
        helix_tools::output::write_png(
            &out.join(&file),
            b.width,
            b.height,
            &helix_tools::output::Pixels::Indexed { data: &b.pixels, palette: &palette },
        )?;
        let i = &b.info;
        table.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{file}\n",
            b.cast, name, b.width, b.height, i.rect.left, i.rect.top, i.reg_h, i.reg_v, i.bits_per_pixel, i.palette
        ));
    }
    fs::write(out.join("bitmaps.tsv"), table)?;

    // The score, frame by frame (only occupied channels).
    let labels_by_frame: std::collections::HashMap<u16, String> = labels.iter().map(|l| (l.frame, l.name.clone())).collect();
    let mut score = String::from("frame\tlabel\tframe_script\tchannel\tcast\tname\tloc_h\tloc_v\twidth\theight\tscript\tink\n");
    for (i, f) in movie.score()?.iter().enumerate() {
        let n = i as u16 + 1;
        for (ch, s) in f.sprites.iter().enumerate().filter(|(_, s)| !s.is_empty()) {
            score.push_str(&format!(
                "{n}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                labels_by_frame.get(&n).map(String::as_str).unwrap_or(""),
                f.script,
                ch + 1,
                s.cast,
                names.get(&s.cast).map(String::as_str).unwrap_or(""),
                s.loc_h,
                s.loc_v,
                s.width,
                s.height,
                s.script,
                s.ink
            ));
        }
    }
    fs::write(out.join("score.tsv"), score)?;
    let config = movie.config()?;
    println!(
        "Director version {:#x}, stage {}x{}, {} bitmaps",
        config.version,
        config.stage.width(),
        config.stage.height(),
        bitmaps.len()
    );

    let count = |k| texts.iter().filter(|m| m.kind() == k).count();
    println!(
        "{} scripts, {} tables, {} other text members, {} score scripts, {} labels, {} named members -> {}",
        count(TextKind::Script),
        count(TextKind::Table),
        count(TextKind::Text),
        actions.len(),
        labels.len(),
        names.len(),
        out.display()
    );
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
