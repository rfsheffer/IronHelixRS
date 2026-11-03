//! Loading the player's own copy of the game.
//!
//! Nothing from the game ships with this program. On desktop the paths
//! come from the command line (defaults match the `out/` folders the
//! extraction tools write); mobile builds will need to bundle or import the
//! data instead.

use std::fs;
use std::path::{Path, PathBuf};

use director::Movie;
use helixdb::GameData;

pub struct Paths {
    /// Resource fork of the Mac game program (`Iron Helix™.rsrc`).
    pub mac_app: PathBuf,
    /// The `Iron Helix™ Files` folder from the Mac CD.
    pub mac_files: PathBuf,
    /// The installed `Iron Helix Sounds` folder (next to the game program).
    pub mac_sounds: PathBuf,
    /// Optional artwork for the space beside the stage.
    pub side_panel: PathBuf,
    /// Save a screenshot after a few seconds and quit (for checking layouts).
    pub screenshot: Option<PathBuf>,
}

impl Paths {
    /// `--mac-app <file>`, `--mac-files <folder>`, `--mac-sounds <folder>`,
    /// `--side-panel <png>`, `--screenshot <png>`.
    pub fn from_args() -> Self {
        let mut p = Self {
            mac_app: PathBuf::from("out/mac/app/Iron Helix™/Iron Helix™.rsrc"),
            mac_files: PathBuf::from("out/mac/files/Iron Helix™ Files"),
            mac_sounds: PathBuf::new(),
            side_panel: PathBuf::from("assets/side_panel.png"),
            screenshot: None,
        };
        let args: Vec<String> = std::env::args().collect();
        let mut sounds = None;
        for pair in args.windows(2) {
            match pair[0].as_str() {
                "--mac-app" => p.mac_app = PathBuf::from(&pair[1]),
                "--mac-files" => p.mac_files = PathBuf::from(&pair[1]),
                "--mac-sounds" => sounds = Some(PathBuf::from(&pair[1])),
                "--side-panel" => p.side_panel = PathBuf::from(&pair[1]),
                "--screenshot" => p.screenshot = Some(PathBuf::from(&pair[1])),
                _ => {}
            }
        }
        p.mac_sounds = sounds.unwrap_or_else(|| {
            p.mac_app.parent().map(|d| d.join("Iron Helix Sounds")).unwrap_or_default()
        });
        p
    }
}

pub struct GameAssets {
    pub movie: Movie,
    pub data: GameData,
    pub stage_w: usize,
    pub stage_h: usize,
    pub palette: paco::Palette,
    pub mac_files: PathBuf,
    pub mac_sounds: PathBuf,
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))
}

/// Palette indices to RGBA (fully opaque).
pub fn indexed_to_rgba(pixels: &[u8], palette: &paco::Palette) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|&i| {
            let c = palette.0[i as usize];
            [c.r, c.g, c.b, 255]
        })
        .collect()
}

impl GameAssets {
    pub fn load(paths: &Paths) -> Result<Self, String> {
        let fork = read(&paths.mac_app)?;
        let movie = Movie::from_resource_fork(&fork).map_err(|e| e.to_string())?;
        let config = movie.config().map_err(|e| e.to_string())?;
        let (stage_w, stage_h) = config.stage_size();
        let data = GameData::from_movie(&movie).map_err(|e| e.to_string())?;
        Ok(Self {
            movie,
            data,
            stage_w,
            stage_h,
            palette: paco::Palette::mac_system(),
            mac_files: paths.mac_files.clone(),
            mac_sounds: paths.mac_sounds.clone(),
        })
    }

    /// Path of a file in the Mac CD's files, from a `/`-separated folder.
    pub fn file(&self, folder: &str, name: &str) -> PathBuf {
        folder.split('/').fold(self.mac_files.clone(), |p, part| p.join(part)).join(name)
    }
}
