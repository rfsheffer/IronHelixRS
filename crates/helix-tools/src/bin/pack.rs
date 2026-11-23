//! Builds a playable Iron Helix from your own copies of the game.
//!
//! ```text
//! pack data --mac <file or folder> [--pc <folder>] [--sega <cue>] [-o IronHelixData]
//! pack windows [--data IronHelixData] [-o dist/IronHelix]
//! pack android [--data IronHelixData] [-o dist/IronHelix.apk] [--sdk <Android SDK>] [--install]
//! ```
//!
//! `pack data` gathers the files the game reads into one folder (see
//! `helixdb::datapack` for its layout), straight from the original discs:
//!
//! - `--mac` (required): the Mac release as the `Iron-Helix_Mac_EN.sit`
//!   archive, its DiskCopy image (`Iron Helix™ CD.img`, with the archive or a
//!   `.rsrc` file beside it for the image's resource fork), a raw HFS image of
//!   the CD, or a folder holding the CD's `Iron Helix™ Files` folder and
//!   `Iron Helix™ Installer`.
//! - `--pc`: the PC CD's `HELIX` folder (or the CD itself), for the hidden
//!   rooms and the scrolling credits.
//! - `--sega`: the Sega CD image's `.cue` sheet, for the theme music and the
//!   extended ending.
//! - `--manual`: a scan of the printed manual as a PDF (one image per page,
//!   like the `Manual.pdf` that comes with copies of the PC release), for the
//!   game's manual viewer. Without it, a `Manual.pdf` in or beside the `--pc`
//!   folder is used.
//!
//! `pack windows` builds the game and puts it in a folder with the data,
//! ready to copy anywhere. `pack android` builds an APK with the data inside,
//! using the Android SDK's own tools (NDK, d8, aapt, zipalign, apksigner) and
//! miniquad's activity, signed with Android's debug key. Nothing from the game is ever part of this
//! repository: the packages are yours, made from your copies.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use helix_tools::mac::safe_name;
use helixdb::datapack as dp;
use macformats::{hfs, ndif, rsrc, sit1, sit5};

#[derive(Parser)]
#[command(about = "Build a playable Iron Helix from your own copies of the game")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Gather the files the game reads into one folder.
    Data {
        /// The Mac release: the .sit archive, the CD's .img, an HFS image, or
        /// a folder with the CD's files.
        #[arg(long)]
        mac: PathBuf,
        /// The PC CD's HELIX folder (optional).
        #[arg(long)]
        pc: Option<PathBuf>,
        /// The Sega CD image's .cue sheet (optional).
        #[arg(long)]
        sega: Option<PathBuf>,
        /// The manual as a scanned PDF (optional; else a Manual.pdf in or
        /// beside the --pc folder).
        #[arg(long)]
        manual: Option<PathBuf>,
        #[arg(short, long, default_value = dp::FOLDER)]
        out: PathBuf,
    },
    /// Build the game for Windows and put it in a folder with the data.
    Windows {
        #[arg(long, default_value = dp::FOLDER)]
        data: PathBuf,
        #[arg(short, long, default_value = "dist/IronHelix")]
        out: PathBuf,
    },
    /// Build the game for Android: an APK with the data inside.
    Android {
        #[arg(long, default_value = dp::FOLDER)]
        data: PathBuf,
        #[arg(short, long, default_value = "dist/IronHelix.apk")]
        out: PathBuf,
        /// The Android SDK (else ANDROID_HOME).
        #[arg(long)]
        sdk: Option<PathBuf>,
        /// Install it on the connected device (adb) when built.
        #[arg(long)]
        install: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Data { mac, pc, sega, manual, out } => data(&mac, pc.as_deref(), sega.as_deref(), manual.as_deref(), &out),
        Command::Windows { data, out } => windows(&data, &out),
        Command::Android { data, out, sdk, install } => android(&data, &out, sdk.as_deref(), install),
    }
}

fn data(mac_in: &Path, pc_in: Option<&Path>, sega_in: Option<&Path>, manual_in: Option<&Path>, out: &Path) -> Result<()> {
    if out.exists() {
        if !dp::is_data_folder(out) && fs::read_dir(out)?.next().is_some() {
            bail!("{} exists and isn't a data folder; choose another --out", out.display());
        }
        fs::remove_dir_all(out).with_context(|| format!("clearing {}", out.display()))?;
    }
    fs::create_dir_all(out)?;
    let work = out.join(".work");
    let mut manifest = vec![dp::VERSION.to_string()];

    println!("Mac release: {}", mac_in.display());
    mac(mac_in, out, &work)?;
    manifest.push(format!("mac: {}", file_name(mac_in)));

    if let Some(pc_in) = pc_in {
        println!("PC release: {}", pc_in.display());
        let n = pc(pc_in, out)?;
        manifest.push(format!("pc: {} ({n} files)", file_name(pc_in)));
    }
    if let Some(cue) = sega_in {
        println!("Sega CD: {}", cue.display());
        sega(cue, out)?;
        manifest.push(format!("sega: {}", file_name(cue)));
    }
    // The manual: given, or a Manual.pdf in or beside the PC folder.
    let manual_in = manual_in.map(Path::to_path_buf).or_else(|| {
        let pc = pc_in?;
        [Some(pc), pc.parent()].into_iter().flatten().find_map(|dir| find_ci(dir, "Manual.pdf"))
    });
    if let Some(pdf) = manual_in {
        println!("Manual: {}", pdf.display());
        match manual(&pdf, out) {
            Ok(n) => manifest.push(format!("manual: {} ({n} pages)", file_name(&pdf))),
            // Optional: a PDF that isn't a scan is skipped.
            Err(e) => println!("  skipped: {e:#}"),
        }
    }
    let _ = fs::remove_dir_all(&work);
    fs::write(out.join(dp::MANIFEST), manifest.join("\n") + "\n")?;
    println!("Done: {} ({} MB)", out.display(), folder_size(out) / 1_000_000);
    Ok(())
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn folder_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| if e.path().is_dir() { folder_size(&e.path()) } else { e.metadata().map_or(0, |m| m.len()) })
        .sum()
}

// --- Mac ---------------------------------------------------------------

/// The CD's movie folder (`Iron Helix™ Files`).
fn is_files_folder(name: &str) -> bool {
    name.starts_with("Iron Helix") && name.ends_with("Files")
}

/// The full installer (not the `Minimum Installer`).
fn is_installer(name: &str) -> bool {
    name.starts_with("Iron Helix") && name.ends_with("Installer") && !name.contains("Minimum")
}

fn mac(input: &Path, out: &Path, work: &Path) -> Result<()> {
    if input.is_dir() {
        return mac_folder(input, out);
    }
    let ext = input.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let hfs_image = match ext.as_str() {
        "sit" => image_from_sit(input, work)?,
        "img" | "image" | "dsk" if !looks_like_hfs(input) => image_from_diskcopy(input, None, work)?,
        _ => input.to_path_buf(),
    };
    mac_hfs(&hfs_image, out)
}

/// A raw HFS volume (or one behind an Apple partition map)?
fn looks_like_hfs(path: &Path) -> bool {
    fs::File::open(path).ok().is_some_and(|f| hfs::Volume::open(f).is_ok())
}

/// The CD image out of the StuffIt archive: the archive gives the image's
/// resource fork (its chunk map); the image itself is the unpacked copy
/// beside the archive if there is one, else unpacked from it (slow).
fn image_from_sit(sit_path: &Path, work: &Path) -> Result<PathBuf> {
    let data = fs::read(sit_path)?;
    if sit1::is_sit1(&data) {
        bail!("{} is a classic StuffIt archive; expected the StuffIt 5 one holding the CD image", sit_path.display());
    }
    let archive = sit5::Archive::parse(&data)?;
    let entry = archive
        .entries
        .iter()
        .find(|e| e.rsrc.is_some() && e.data.len > 100_000_000)
        .context("the archive holds no CD image")?;
    let rsrc_fork = archive.extract(entry.rsrc.as_ref().unwrap())?;
    let beside = sit_path.parent().unwrap_or(Path::new(".")).join(safe_name(&entry.name));
    let image = if fs::metadata(&beside).is_ok_and(|m| m.len() == entry.data.len as u64) {
        println!("  using the unpacked {}", beside.display());
        beside
    } else {
        println!("  unpacking {} from the archive (this takes a while)", entry.name);
        fs::create_dir_all(work)?;
        let img = work.join("cd.img");
        fs::write(&img, archive.extract(&entry.data)?)?;
        img
    };
    image_from_diskcopy(&image, Some(&rsrc_fork), work)
}

/// Expand a DiskCopy 6 image to a raw HFS volume. Its chunk map is in the
/// image's resource fork: given, or a `.rsrc` file or the StuffIt archive
/// beside the image.
fn image_from_diskcopy(image: &Path, rsrc_fork: Option<&[u8]>, work: &Path) -> Result<PathBuf> {
    let fork = match rsrc_fork {
        Some(f) => f.to_vec(),
        None => {
            let mut beside = image.as_os_str().to_owned();
            beside.push(".rsrc");
            if let Ok(f) = fs::read(&beside) {
                f
            } else if let Some(sit) = fs::read_dir(image.parent().unwrap_or(Path::new(".")))?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sit")))
            {
                return image_from_sit(&sit, work);
            } else {
                bail!(
                    "{} needs its resource fork (its chunk map), which is lost when it's copied off a Mac: \
                     pass the .sit archive it came in instead, or put the fork beside it as a .rsrc file",
                    image.display()
                );
            }
        }
    };
    let resources = rsrc::parse(&fork)?;
    let bcem = resources.iter().find(|r| &r.kind == b"bcem").context("no chunk map ('bcem') in the image's resource fork")?;
    let map = ndif::ChunkMap::parse(&bcem.data)?;
    println!("  expanding the disc image ({} MB)", map.sectors as usize * ndif::SECTOR / 1_000_000);
    let data = fs::read(image)?;
    fs::create_dir_all(work)?;
    let volume = work.join("cd.hfs");
    let mut w = BufWriter::new(fs::File::create(&volume)?);
    ndif::expand(&map, &data, |chunk| {
        w.write_all(chunk)?;
        Ok(())
    })?;
    w.flush()?;
    Ok(volume)
}

/// From the CD's HFS volume: the movie folder and the installed game.
fn mac_hfs(volume: &Path, out: &Path) -> Result<()> {
    let mut vol = hfs::Volume::open(fs::File::open(volume)?).with_context(|| format!("{} isn't an HFS volume", volume.display()))?;
    println!("  reading the CD '{}'", vol.name);
    let files_dir = out.join(dp::MAC_FILES);
    let (mut count, mut installer) = (0, None);
    for file in vol.files.clone() {
        let folders = vol.folder_path(file.parent);
        if let Some((top, rest)) = folders.split_first()
            && is_files_folder(top)
        {
            let dest = rest.iter().fold(files_dir.clone(), |p, f| p.join(safe_name(f))).join(safe_name(&file.name));
            fs::create_dir_all(dest.parent().unwrap())?;
            fs::write(dest, vol.read_data(&file)?)?;
            count += 1;
        } else if folders.is_empty() && is_installer(&file.name) {
            installer = Some(vol.read_data(&file)?);
        }
    }
    if count == 0 {
        bail!("no 'Iron Helix™ Files' folder on '{}'", vol.name);
    }
    println!("  {count} movies and clips");
    mac_installed(&installer.context("no 'Iron Helix™ Installer' on the CD")?, out)
}

/// From a folder with the CD's files (copied from the CD, or `macx
/// hfs-extract`'s output).
fn mac_folder(dir: &Path, out: &Path) -> Result<()> {
    let entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    let name = |e: &fs::DirEntry| e.file_name().to_string_lossy().into_owned();
    let files = entries.iter().find(|e| e.path().is_dir() && is_files_folder(&name(e))).context("no 'Iron Helix™ Files' folder there")?;
    let count = copy_tree(&files.path(), &out.join(dp::MAC_FILES), true)?;
    println!("  {count} movies and clips");
    let installer = entries.iter().find(|e| e.path().is_file() && is_installer(&name(e))).context("no 'Iron Helix™ Installer' there")?;
    mac_installed(&fs::read(installer.path())?, out)
}

/// The installer's archive: the game program's resource fork, its sounds
/// and its movies (`Misc Files`).
fn mac_installed(installer: &[u8], out: &Path) -> Result<()> {
    let archive = sit1::Archive::parse(installer).context("the installer isn't a StuffIt archive")?;
    let base = out.join(dp::MAC_INSTALLED);
    let (mut program, mut count) = (false, 0);
    for e in &archive.entries {
        match e.path.as_slice() {
            [_, _] if &e.file_type == b"APPL" => {
                fs::write(out.join(dp::MAC_PROGRAM), archive.extract(&e.rsrc)?)?;
                program = true;
            }
            [_, folder, name] if folder == "Iron Helix Sounds" || (folder == "Misc Files" && name != "Xobjects") => {
                let dest = base.join(folder).join(safe_name(name));
                fs::create_dir_all(dest.parent().unwrap())?;
                fs::write(dest, archive.extract(&e.data)?)?;
                count += 1;
            }
            _ => {}
        }
    }
    if !program {
        bail!("the installer holds no game program");
    }
    println!("  the game program, {count} sounds and movies");
    Ok(())
}

/// Copy a folder. `mac_extras` leaves out what `macx` adds beside the Mac
/// files (resource forks as `.rsrc` files, `_finder.tsv`).
fn copy_tree(from: &Path, to: &Path, mac_extras: bool) -> Result<usize> {
    fs::create_dir_all(to)?;
    let mut count = 0;
    for e in fs::read_dir(from)?.flatten() {
        let name = e.file_name();
        if mac_extras && (name.to_string_lossy().ends_with(".rsrc") || name == "_finder.tsv") {
            continue;
        }
        if e.path().is_dir() {
            count += copy_tree(&e.path(), &to.join(&name), mac_extras)?;
        } else {
            fs::copy(e.path(), to.join(&name))?;
            count += 1;
        }
    }
    Ok(count)
}

// --- PC ----------------------------------------------------------------

/// A file or folder by name, without regard to case.
fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).map(|e| e.path())
}

/// The movie names compared the way the game matches Mac and PC names
/// (see `helixdb::files`).
fn key(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// From the PC CD: `HELIX.EXE`, the credits and their tune, and the
/// navigation movies the Mac CD lacks (the hidden rooms).
fn pc(input: &Path, out: &Path) -> Result<usize> {
    let helix = if find_ci(input, "HELIX.EXE").is_some() { input.to_path_buf() } else { find_ci(input, "HELIX").context("no HELIX folder there")? };
    let dest = out.join(dp::PC);
    let mut count = 0;
    let mut copy = |rel: &[&str]| -> Result<()> {
        let src = rel.iter().try_fold(helix.clone(), |p, part| find_ci(&p, part)).with_context(|| format!("no {} on the PC CD", rel.join("/")))?;
        let to = rel.iter().fold(dest.clone(), |p, part| p.join(part));
        fs::create_dir_all(to.parent().unwrap())?;
        fs::copy(src, to)?;
        count += 1;
        Ok(())
    };
    copy(&["HELIX.EXE"])?;
    copy(&["SEGUES", "CREDITS"])?;
    copy(&["OPENING", "CREDTUNE.WAV"])?;

    // Navigation movies by deck: the PC splits each deck over NAV/L<deck><A-E>.
    let mac_nav = out.join(dp::MAC_FILES).join("Navigation");
    let nav = find_ci(&helix, "NAV").context("no NAV folder on the PC CD")?;
    for folder in fs::read_dir(&nav)?.flatten() {
        let fname = folder.file_name().to_string_lossy().to_ascii_uppercase();
        let Some(deck) = fname.strip_prefix('L').and_then(|r| r.chars().next()).filter(char::is_ascii_digit) else { continue };
        // Mac names, whole and cut to 8.3 as the PC cuts them.
        let mac: std::collections::HashSet<String> = fs::read_dir(mac_nav.join(format!("L{deck}")))
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                let cut: String = n.replace('.', "_").chars().take(8).collect();
                [key(&n), key(&cut)]
            })
            .collect();
        for movie in fs::read_dir(folder.path())?.flatten() {
            let name = movie.file_name().to_string_lossy().into_owned();
            if !mac.contains(&key(&name)) {
                let to = dest.join("NAV").join(&fname).join(&name);
                fs::create_dir_all(to.parent().unwrap())?;
                fs::copy(movie.path(), to)?;
                count += 1;
            }
        }
    }
    println!("  {count} files (the hidden rooms, the credits and HELIX.EXE)");
    Ok(count)
}

// --- Manual --------------------------------------------------------------

/// The manual's pages, each a JPEG taken out of the scanned PDF as it is,
/// as `manual/01.jpg` and on.
fn manual(pdf: &Path, out: &Path) -> Result<usize> {
    let data = fs::read(pdf)?;
    let pages = helix_tools::pdf::Pdf::parse(&data)?.page_jpegs().context("this PDF isn't a scan with one image per page")?;
    let dir = out.join(dp::MANUAL);
    fs::create_dir_all(&dir)?;
    for (i, jpeg) in pages.iter().enumerate() {
        fs::write(dir.join(format!("{:02}.jpg", i + 1)), jpeg)?;
    }
    println!("  {} pages", pages.len());
    Ok(pages.len())
}

// --- Sega CD -------------------------------------------------------------

fn sega(cue: &Path, out: &Path) -> Result<()> {
    let pcm = cdimage::cue::audio_track(cue, 2).context("reading the theme (audio track 2)")?;
    let theme = out.join(dp::SEGA_THEME);
    fs::create_dir_all(theme.parent().unwrap())?;
    fs::write(&theme, wav(2, 16, cdimage::cue::CD_AUDIO_RATE, &pcm))?;
    println!("  the theme ({:.0} s)", pcm.len() as f64 / (cdimage::cue::CD_AUDIO_RATE as f64 * 4.0));

    let image = cdimage::cue::image_file(cue)?;
    let mut disc = cdimage::Disc::open(fs::File::open(&image).with_context(|| format!("opening {}", image.display()))?)?;
    let ending = disc
        .entries()?
        .into_iter()
        .find(|e| e.path.eq_ignore_ascii_case("/SEGUES/ENDING.SEG"))
        .context("no SEGUES/ENDING.SEG on the Sega CD")?;
    let dest = out.join(dp::SEGA).join("SEGUES").join("ENDING.SEG");
    fs::create_dir_all(dest.parent().unwrap())?;
    fs::write(dest, disc.read(ending.sector, ending.len)?)?;
    println!("  the extended ending");
    Ok(())
}

fn wav(channels: u16, bits: u16, rate: u32, data: &[u8]) -> Vec<u8> {
    let block = channels * bits / 8;
    let mut w = Vec::with_capacity(44 + data.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * block as u32).to_le_bytes());
    w.extend_from_slice(&block.to_le_bytes());
    w.extend_from_slice(&bits.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(data);
    w
}

// --- Windows -------------------------------------------------------------

fn windows(data: &Path, out: &Path) -> Result<()> {
    if !dp::is_data_folder(data) {
        bail!("{} isn't a data folder; make one with `pack data` first", data.display());
    }
    // The probe icon, from the player's own copy, for the program file.
    let ico = std::env::temp_dir().join("ironhelix-pack.ico");
    let icon = probe_ico(&data.join(dp::MAC_PROGRAM));
    if let Some(bytes) = &icon {
        fs::write(&ico, bytes)?;
    }
    println!("Building the game");
    let mut cargo = Process::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo.args(["build", "--release", "-p", "helix-app"]);
    if icon.is_some() {
        cargo.env("IRONHELIX_ICON", &ico);
    }
    if !cargo.status().context("running cargo")?.success() {
        bail!("the build failed");
    }
    let exe = workspace_target().join("release").join("ironhelix.exe");

    println!("Packaging into {}", out.display());
    if out.exists() {
        fs::remove_dir_all(out)?;
    }
    fs::create_dir_all(out)?;
    fs::copy(&exe, out.join("IronHelix.exe")).with_context(|| format!("copying {}", exe.display()))?;
    let count = copy_tree(data, &out.join(dp::FOLDER), false)?;
    fs::write(out.join("README.txt"), WINDOWS_README)?;
    println!("Done: {} ({count} data files, {} MB)", out.display(), folder_size(out) / 1_000_000);
    Ok(())
}

const WINDOWS_README: &str = "Iron Helix

Run IronHelix.exe. Keep the IronHelixData folder beside it.

Saved games go in %APPDATA%\\IronHelix. Keys: arrows move and turn, P pauses, Esc quits
(with the option to save), F switches pixel-perfect scaling.

Built with IronHelixRS (https://github.com/rfsheffer/IronHelixRS) from your own copy of the game.
It is for your own use; please don't share the game's files.
";

/// The workspace's target folder (where cargo puts builds).
fn workspace_target() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest.parent().and_then(Path::parent).unwrap_or(manifest).join("target")
    })
}

/// The probe icon (the program's custom icon) as a Windows .ico: 16, 32 and
/// 64 pixels, each a PNG.
fn probe_ico(rsrc_path: &Path) -> Option<Vec<u8>> {
    let resources = rsrc::parse(&fs::read(rsrc_path).ok()?).ok()?;
    let palette = paco::Palette::mac_system();
    let palette: [[u8; 3]; 256] = std::array::from_fn(|i| [palette.0[i].r, palette.0[i].g, palette.0[i].b]);
    let (large, small) = macformats::icon::family(&resources, -16455, &palette)?;
    let images = [small, large.scaled(1), large.scaled(2)];
    let pngs: Vec<(usize, Vec<u8>)> = images.iter().map(|i| (i.size, png_rgba(i.size, &i.rgba))).collect();
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len();
    for (size, png) in &pngs {
        ico.extend_from_slice(&[*size as u8, *size as u8, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in pngs {
        ico.extend_from_slice(&png);
    }
    Some(ico)
}

fn png_rgba(size: usize, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, size as u32, size as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(rgba)).expect("encoding to memory");
    out
}

// --- Android -------------------------------------------------------------

/// The app's Java package and the library the activity loads (the game,
/// `crates/helix-app` built as `libhelix_app.so`).
const ANDROID_PACKAGE: &str = "io.github.rfsheffer.ironhelix";
const ANDROID_LIBRARY: &str = "helix_app";
const ANDROID_MIN_SDK: u32 = 26;

/// What building for Android needs, found in the Android SDK.
struct AndroidSdk {
    ndk_bin: PathBuf,
    build_tools: PathBuf,
    android_jar: PathBuf,
    api_level: u32,
}

fn android_sdk(sdk: Option<&Path>) -> Result<AndroidSdk> {
    let sdk = sdk
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("ANDROID_HOME").or_else(|| std::env::var_os("ANDROID_SDK_ROOT")).map(PathBuf::from))
        .context("no Android SDK: pass --sdk, or set ANDROID_HOME")?;
    // The newest of each: NDK, build tools (with d8), platform.
    let newest = |dir: PathBuf, ok: &dyn Fn(&Path) -> bool| -> Option<PathBuf> {
        let mut all: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| ok(p)).collect();
        all.sort_by_key(|p| version_key(&p.file_name().unwrap_or_default().to_string_lossy()));
        all.pop()
    };
    let ndk = std::env::var_os("ANDROID_NDK_HOME")
        .or_else(|| std::env::var_os("NDK_HOME"))
        .map(PathBuf::from)
        .or_else(|| newest(sdk.join("ndk"), &|p| p.join("toolchains").is_dir()))
        .context("no NDK in the SDK's ndk folder (install one with the SDK manager), or set ANDROID_NDK_HOME")?;
    let prebuilt = ndk.join("toolchains").join("llvm").join("prebuilt");
    let host = fs::read_dir(&prebuilt)?.flatten().map(|e| e.path()).next().context("the NDK has no prebuilt toolchain")?;
    let build_tools = newest(sdk.join("build-tools"), &|p| p.join("lib").join("d8.jar").is_file()).context("no build tools with d8 in the SDK")?;
    let platform = newest(sdk.join("platforms"), &|p| p.join("android.jar").is_file()).context("no platform (android.jar) in the SDK")?;
    let api_level = platform.file_name().unwrap().to_string_lossy().trim_start_matches("android-").parse().unwrap_or(ANDROID_MIN_SDK);
    Ok(AndroidSdk { ndk_bin: host.join("bin"), build_tools, android_jar: platform.join("android.jar"), api_level })
}

/// "25.1.8937393" or "android-33" as numbers, for sorting.
fn version_key(name: &str) -> Vec<u32> {
    name.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect()
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

fn run(cmd: &mut Process, what: &str) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {what}"))?;
    if !status.success() {
        bail!("{what} failed");
    }
    Ok(())
}

/// A JDK tool (javac, keytool): from JAVA_HOME if it has it, else the PATH.
fn java_tool(name: &str) -> PathBuf {
    std::env::var_os("JAVA_HOME")
        .map(|h| PathBuf::from(h).join("bin").join(exe(name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn android(data: &Path, out: &Path, sdk: Option<&Path>, install: bool) -> Result<()> {
    if !dp::is_data_folder(data) {
        bail!("{} isn't a data folder; make one with `pack data` first", data.display());
    }
    let sdk = android_sdk(sdk)?;
    let target = workspace_target();
    let work = target.join("android");
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }

    // 1. The game as a shared library, linked with the NDK's clang.
    println!("Building the game for Android");
    let mut cargo = Process::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo
        .args(["rustc", "--release", "--target", "aarch64-linux-android", "-p", "helix-app", "--lib", "--crate-type", "cdylib"])
        .env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", sdk.ndk_bin.join(exe("clang")))
        // 16 KB pages: newer Android devices may use them.
        .env(
            "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
            format!("-C link-arg=--target=aarch64-linux-android{ANDROID_MIN_SDK} -C link-arg=-Wl,-z,max-page-size=16384"),
        );
    run(&mut cargo, "the Android build (is the target installed? `rustup target add aarch64-linux-android`)")?;
    let so = target.join("aarch64-linux-android").join("release").join(format!("lib{ANDROID_LIBRARY}.so"));
    let lib_dir = work.join("lib").join("arm64-v8a");
    fs::create_dir_all(&lib_dir)?;
    fs::copy(&so, lib_dir.join(format!("lib{ANDROID_LIBRARY}.so")))?;

    // 2. The data, as the app's assets, with an index of its files. The
    // packaging tools take only plain names (no "™"), so each file is
    // stored under its number and the index gives its real path (hard
    // links where the file system allows: fast, and no extra space).
    println!("Adding the game data");
    let assets = work.join("assets").join(dp::FOLDER);
    fs::create_dir_all(&assets)?;
    let mut index = String::new();
    for (n, rel) in list_files(data)?.iter().enumerate() {
        let (from, to) = (data.join(rel), assets.join(n.to_string()));
        if fs::hard_link(&from, &to).is_err() {
            fs::copy(&from, &to)?;
        }
        index.push_str(&format!("{n}\t{rel}\n"));
    }
    fs::write(assets.join("index.txt"), index)?;

    // 3. The launcher icon (the probe, from your copy), the manifest, and
    // miniquad's activity.
    let res = work.join("res");
    let icon = android_icons(&data.join(dp::MAC_PROGRAM), &res)?;
    fs::write(work.join("AndroidManifest.xml"), android_manifest(sdk.api_level, icon))?;
    let java = miniquad_java()?;
    let package_dir = ANDROID_PACKAGE.split('.').fold(work.join("src"), |p, part| p.join(part));
    fs::create_dir_all(&package_dir)?;
    fs::write(package_dir.join("MainActivity.java"), main_activity(&fs::read_to_string(java.join("MainActivity.java"))?))?;
    fs::create_dir_all(work.join("src").join("quad_native"))?;
    fs::copy(java.join("QuadNative.java"), work.join("src").join("quad_native").join("QuadNative.java"))?;

    // 4. Compile the activity and convert it for Android.
    println!("Compiling the activity");
    let classes = work.join("classes");
    fs::create_dir_all(&classes)?;
    run(
        Process::new(java_tool("javac"))
            .args(["-source", "8", "-target", "8", "-nowarn", "-Xlint:-options"])
            .arg("-bootclasspath")
            .arg(&sdk.android_jar)
            .arg("-d")
            .arg(&classes)
            .arg(package_dir.join("MainActivity.java"))
            .arg(work.join("src").join("quad_native").join("QuadNative.java")),
        "javac",
    )?;
    let class_files = list_files(&classes)?.into_iter().map(|rel| classes.join(rel));
    let dex = work.join("dex");
    fs::create_dir_all(&dex)?;
    // (The build tools' d8 and apksigner scripts pass Java an option newer
    // JDKs refuse, so their jars are run directly.)
    run(
        Process::new(java_tool("java"))
            .arg("-cp")
            .arg(sdk.build_tools.join("lib").join("d8.jar"))
            .arg("com.android.tools.r8.D8")
            .args(["--release", "--min-api", &ANDROID_MIN_SDK.to_string(), "--lib"])
            .arg(&sdk.android_jar)
            .arg("--output")
            .arg(&dex)
            .args(class_files),
        "d8",
    )?;

    // 5. Package: resources, assets (stored, not compressed: the movies
    // are compressed already and the game reads them faster this way),
    // code and library; then align and sign.
    println!("Packaging the APK");
    let aapt = sdk.build_tools.join(exe("aapt"));
    let unaligned = work.join("unaligned.apk");
    let mut package = Process::new(&aapt);
    package.args(["package", "-f", "-0", "", "-F"]).arg(&unaligned).arg("-M").arg(work.join("AndroidManifest.xml")).arg("-I").arg(&sdk.android_jar).arg("-A").arg(work.join("assets"));
    if icon {
        package.arg("-S").arg(&res);
    }
    run(&mut package, "aapt package")?;
    run(Process::new(&aapt).current_dir(&dex).args(["add", "-0", ""]).arg(&unaligned).arg("classes.dex"), "aapt add classes.dex")?;
    run(
        Process::new(&aapt).current_dir(&work).args(["add", "-0", ""]).arg(&unaligned).arg(format!("lib/arm64-v8a/lib{ANDROID_LIBRARY}.so")),
        "aapt add the library",
    )?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(out);
    run(Process::new(sdk.build_tools.join(exe("zipalign"))).args(["-f", "-p", "4"]).arg(&unaligned).arg(out), "zipalign")?;
    let keystore = debug_keystore()?;
    run(
        Process::new(java_tool("java")).arg("-jar").arg(sdk.build_tools.join("lib").join("apksigner.jar")).args(["sign", "--ks"]).arg(&keystore).args(["--ks-pass", "pass:android", "--key-pass", "pass:android"]).arg(out),
        "apksigner",
    )?;
    let _ = fs::remove_dir_all(work.join("assets"));
    println!("Done: {} ({} MB)", out.display(), fs::metadata(out)?.len() / 1_000_000);

    if install {
        println!("Installing on the connected device");
        let adb = std::env::var_os("ANDROID_HOME")
            .map(|h| PathBuf::from(h).join("platform-tools").join(exe("adb")))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("adb"));
        run(Process::new(adb).args(["install", "-r"]).arg(out), "adb install")?;
    }
    Ok(())
}

/// Every file under a folder, relative to it.
fn list_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.path().is_dir() {
            out.extend(list_files(&e.path())?.into_iter().map(|r| format!("{name}/{r}")));
        } else {
            out.push(name);
        }
    }
    Ok(out)
}

/// Launcher icons from the probe icon: 48 dp at a few densities, each a
/// whole multiple of the 32-pixel original.
fn android_icons(rsrc_path: &Path, res: &Path) -> Result<bool> {
    let Some(resources) = fs::read(rsrc_path).ok().and_then(|f| rsrc::parse(&f).ok()) else { return Ok(false) };
    let palette = paco::Palette::mac_system();
    let palette: [[u8; 3]; 256] = std::array::from_fn(|i| [palette.0[i].r, palette.0[i].g, palette.0[i].b]);
    let Some((large, _)) = macformats::icon::family(&resources, -16455, &palette) else { return Ok(false) };
    for (density, scale) in [("mdpi", 2), ("xhdpi", 3), ("xxxhdpi", 6)] {
        let dir = res.join(format!("mipmap-{density}"));
        fs::create_dir_all(&dir)?;
        let icon = large.scaled(scale);
        fs::write(dir.join("ic_launcher.png"), png_rgba(icon.size, &icon.rgba))?;
    }
    Ok(true)
}

fn android_manifest(api_level: u32, icon: bool) -> String {
    let icon = if icon { r#" android:icon="@mipmap/ic_launcher""# } else { "" };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
        package="{ANDROID_PACKAGE}"
        android:versionCode="1"
        android:versionName="{version}">
    <uses-sdk android:minSdkVersion="{ANDROID_MIN_SDK}" android:targetSdkVersion="{api_level}" />
    <uses-feature android:glEsVersion="0x00020000" android:required="true" />
    <application android:label="Iron Helix" android:hasCode="true"{icon}
            android:theme="@android:style/Theme.DeviceDefault.NoActionBar.Fullscreen">
        <activity android:name=".MainActivity" android:label="Iron Helix" android:exported="true"
                android:configChanges="orientation|keyboardHidden|screenSize"
                android:screenOrientation="sensorLandscape">
            <meta-data android:name="android.app.lib_name" android:value="{ANDROID_LIBRARY}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// miniquad's Java activity, from the miniquad the game is built with.
fn miniquad_java() -> Result<PathBuf> {
    let meta = Process::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1"])
        .output()
        .context("running cargo metadata")?;
    let json = String::from_utf8_lossy(&meta.stdout);
    // The "manifest_path" of the miniquad package (cargo metadata is JSON:
    // backslashes come doubled).
    let manifest = json
        .split("\"manifest_path\":\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap_or("").replace("\\\\", "\\"))
        .find(|p| Path::new(p).parent().and_then(Path::file_name).is_some_and(|d| d.to_string_lossy().starts_with("miniquad-")))
        .context("can't find miniquad in cargo metadata")?;
    Ok(Path::new(&manifest).parent().unwrap().join("java"))
}

/// miniquad's activity for this app: its package and library, with no
/// plugins, and the Back button passed to the game (which opens the quit
/// dialog) instead of closing the app.
fn main_activity(src: &str) -> String {
    let src = src.replace("TARGET_PACKAGE_NAME", ANDROID_PACKAGE).replace("LIBRARY_NAME", ANDROID_LIBRARY);
    let back = "super.onBackPressed();";
    if !src.contains(back) {
        println!("  (miniquad's activity has changed: Back will close the app)");
    }
    let src = src.replace(back, "QuadNative.surfaceOnKeyDown(KeyEvent.KEYCODE_BACK);\n        QuadNative.surfaceOnKeyUp(KeyEvent.KEYCODE_BACK);");
    // Full screen on Android 11+: miniquad only lays the game out behind
    // the system bars; hide them too (a swipe shows them for a moment).
    let behind = "getWindow().setDecorFitsSystemWindows(false);";
    let src = src.replace(
        behind,
        "getWindow().setDecorFitsSystemWindows(false);
                            android.view.WindowInsetsController bars = getWindow().getInsetsController();
                            if (bars != null) {
                                bars.hide(android.view.WindowInsets.Type.systemBars());
                                bars.setSystemBarsBehavior(android.view.WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                            }",
    );
    // The game keeps the screen on while it plays (android::keep_screen_on).
    src.replacen(
        "//% MAIN_ACTIVITY_BODY",
        "public void setKeepScreenOn(final boolean on) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (on) {
                    getWindow().addFlags(LayoutParams.FLAG_KEEP_SCREEN_ON);
                } else {
                    getWindow().clearFlags(LayoutParams.FLAG_KEEP_SCREEN_ON);
                }
            }
        });
    }",
        1,
    )
}

/// Android's debug signing key (the one Android Studio makes), made with
/// keytool if it isn't there yet.
fn debug_keystore() -> Result<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).context("no home folder")?;
    let dir = PathBuf::from(home).join(".android");
    let ks = dir.join("debug.keystore");
    if !ks.is_file() {
        fs::create_dir_all(&dir)?;
        run(
            Process::new(java_tool("keytool"))
                .args(["-genkeypair", "-keystore"])
                .arg(&ks)
                .args(["-storepass", "android", "-keypass", "android", "-alias", "androiddebugkey"])
                .args(["-dname", "CN=Android Debug,O=Android,C=US", "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000"]),
            "keytool",
        )?;
    }
    Ok(ks)
}
