//! Loading the player's own copy of the game.
//!
//! Nothing from the game ships with this program. On desktop the paths
//! come from the command line (defaults match the `out/` folders the
//! extraction tools write); mobile builds will need to bundle or import the
//! data instead.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use director::Movie;
use helixdb::{GameData, GameFiles};

pub struct Paths {
    /// Resource fork of the Mac game program (`Iron Helix™.rsrc`).
    pub mac_app: PathBuf,
    /// The `Iron Helix™ Files` folder from the Mac CD.
    pub mac_files: PathBuf,
    /// The installed `Iron Helix Sounds` folder (next to the game program).
    pub mac_sounds: PathBuf,
    /// The PC CD's `HELIX` folder, for its hidden rooms (optional).
    pub pc_files: Option<PathBuf>,
    /// Optional artwork for the space beside the stage.
    pub side_panel: PathBuf,
    /// Save a screenshot after a few seconds and quit (for checking layouts).
    pub screenshot: Option<PathBuf>,
    /// Skill level 1-5 (until the title screen picks it).
    pub skill: u8,
    /// Random seed; from the clock unless given.
    pub seed: u64,
}

impl Paths {
    /// `--mac-app <file>`, `--mac-files <folder>`, `--mac-sounds <folder>`,
    /// `--pc-files <folder>`, `--side-panel <png>`, `--screenshot <png>`,
    /// `--skill <1-5>`, `--seed <n>`.
    pub fn from_args() -> Self {
        let mut p = Self {
            mac_app: PathBuf::from("out/mac/app/Iron Helix™/Iron Helix™.rsrc"),
            mac_files: PathBuf::from("out/mac/files/Iron Helix™ Files"),
            mac_sounds: PathBuf::new(),
            pc_files: Some(PathBuf::from("../Reference/Game/Windows/HELIX")).filter(|p| p.is_dir()),
            side_panel: PathBuf::from("assets/side_panel.png"),
            screenshot: None,
            skill: 1,
            seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64),
        };
        let args: Vec<String> = std::env::args().collect();
        let mut sounds = None;
        for pair in args.windows(2) {
            match pair[0].as_str() {
                "--mac-app" => p.mac_app = PathBuf::from(&pair[1]),
                "--mac-files" => p.mac_files = PathBuf::from(&pair[1]),
                "--mac-sounds" => sounds = Some(PathBuf::from(&pair[1])),
                "--pc-files" => p.pc_files = Some(PathBuf::from(&pair[1])),
                "--side-panel" => p.side_panel = PathBuf::from(&pair[1]),
                "--screenshot" => p.screenshot = Some(PathBuf::from(&pair[1])),
                "--skill" => p.skill = pair[1].parse().unwrap_or(1),
                "--seed" => p.seed = pair[1].parse().unwrap_or(1),
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
    pub files: GameFiles,
    pub mac_sounds: PathBuf,
    /// Nodes added from the PC release (empty without it).
    pub pc_rooms: Vec<String>,
    /// Sound members by name, as WAV files.
    pub cast_sounds: HashMap<String, Vec<u8>>,
    pub skill: u8,
    pub seed: u64,
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
        let mut data = GameData::from_movie(&movie).map_err(|e| e.to_string())?;
        let mut pc_rooms = Vec::new();
        if let Some(pc) = &paths.pc_files {
            let exe = read(&pc.join("HELIX.EXE"))?;
            pc_rooms = data.add_pc_rooms(&exe).map_err(|e| e.to_string())?;
        }
        let mut cast_sounds = HashMap::new();
        for (cast, name) in movie.cast_names().map_err(|e| e.to_string())? {
            if let Ok(Some(snd)) = movie.sound(cast) {
                cast_sounds.insert(name, crate::sound::pcm8_to_wav(snd.rate, &snd.samples));
            }
        }
        Ok(Self {
            movie,
            data,
            stage_w,
            stage_h,
            palette: paco::Palette::mac_system(),
            files: GameFiles::new(&paths.mac_files, paths.pc_files.as_deref())
                .with_installed(paths.mac_app.parent().map(Path::to_path_buf).unwrap_or_default()),
            mac_sounds: paths.mac_sounds.clone(),
            pc_rooms,
            cast_sounds,
            skill: paths.skill.clamp(1, 5),
            seed: paths.seed,
        })
    }

    /// Path of a movie or clip by its Mac folder (`/`-separated) and name,
    /// from whichever release has it.
    pub fn file(&self, folder: &str, name: &str) -> PathBuf {
        self.files.find(folder, name).unwrap_or_else(|| self.files.mac_path(folder, name))
    }
}
