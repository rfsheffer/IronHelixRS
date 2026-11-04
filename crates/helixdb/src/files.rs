//! Finding the game's movies across the releases the player has.
//!
//! The Mac CD's `Iron Helix™ Files` folder comes first, then the installed
//! game's folder (`Misc Files`). For navigation movies the PC CD's `HELIX`
//! folder is the fallback: its movies decode to the same pixels, and it alone
//! has the hidden rooms' movies (design doc §14.4). Names are matched without
//! regard to case where the exact name is missing, as on the Mac (the CD has
//! `crew01` next to `Crew03`).
//! The PC names are 8.3: `Navigation/L2/2.42.LN` is `NAV/L2<A-E>/2_42_LN`,
//! `3.03.E.CW` is cut to `3_03_E_C`, and ladder ids drop the dots around the
//! `L` (`2.1.1.L.D` is `2_1_1LD`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct GameFiles {
    mac: PathBuf,
    installed: Option<PathBuf>,
    /// The PC CD's `HELIX` folder.
    pc: Option<PathBuf>,
    /// PC navigation movies by deck, keyed by [`key`].
    pc_nav: HashMap<u8, HashMap<String, PathBuf>>,
}

/// Names compared without separators or case.
fn key(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

impl GameFiles {
    /// `mac` is the Mac CD's `Iron Helix™ Files` folder; `pc` the PC CD's
    /// `HELIX` folder, if the player has it.
    pub fn new(mac: impl Into<PathBuf>, pc: Option<&Path>) -> Self {
        let mut pc_nav: HashMap<u8, HashMap<String, PathBuf>> = HashMap::new();
        let folders = pc.and_then(|pc| fs::read_dir(pc.join("NAV")).ok()).into_iter().flatten().flatten();
        for folder in folders {
            let name = folder.file_name().to_string_lossy().to_ascii_uppercase();
            // L<deck><part>: the movies of one deck are split over folders A-E.
            let Some(deck) = name.strip_prefix('L').and_then(|r| r.chars().next()).and_then(|c| c.to_digit(10)) else {
                continue;
            };
            let movies = pc_nav.entry(deck as u8).or_default();
            for file in fs::read_dir(folder.path()).into_iter().flatten().flatten() {
                movies.insert(key(&file.file_name().to_string_lossy()), file.path());
            }
        }
        Self { mac: mac.into(), installed: None, pc: pc.map(Path::to_path_buf), pc_nav }
    }

    /// Also look in the installed game's folder (the one holding `Misc Files`).
    pub fn with_installed(mut self, folder: impl Into<PathBuf>) -> Self {
        self.installed = Some(folder.into());
        self
    }

    /// Whether any PC movies were found.
    pub fn has_pc(&self) -> bool {
        !self.pc_nav.is_empty()
    }

    /// The Mac path of a file, from a `/`-separated folder (whether or not
    /// it exists).
    pub fn mac_path(&self, folder: &str, name: &str) -> PathBuf {
        folder.split('/').fold(self.mac.clone(), |p, part| p.join(part)).join(name)
    }

    /// A file on the PC CD by its own folder and name (`SEGUES`, `CREDITS`),
    /// if the player has it.
    pub fn pc_file(&self, folder: &str, name: &str) -> Option<PathBuf> {
        find_in(self.pc.as_ref()?, folder, name)
    }

    /// A movie or clip by its Mac folder and name, from whichever release has it.
    pub fn find(&self, folder: &str, name: &str) -> Option<PathBuf> {
        for root in std::iter::once(&self.mac).chain(&self.installed) {
            if let Some(found) = find_in(root, folder, name) {
                return Some(found);
            }
        }
        let deck = folder.strip_prefix("Navigation/L")?.parse::<u8>().ok()?;
        let movies = self.pc_nav.get(&deck)?;
        let cut: String = name.replace('.', "_").chars().take(8).collect();
        movies.get(&key(name)).or_else(|| movies.get(&key(&cut))).cloned()
    }
}

/// `root/folder/name`, matching each part without case if needed.
fn find_in(root: &Path, folder: &str, name: &str) -> Option<PathBuf> {
    let exact = folder.split('/').fold(root.to_path_buf(), |p, part| p.join(part)).join(name);
    if exact.is_file() {
        return Some(exact);
    }
    let mut path = root.to_path_buf();
    for part in folder.split('/').chain(std::iter::once(name)) {
        let entry = fs::read_dir(&path).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        path = entry.path();
    }
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pc_names() {
        let dir = std::env::temp_dir().join(format!("helixdb-files-{}", std::process::id()));
        let l2 = dir.join("pc/NAV/L2E");
        fs::create_dir_all(&l2).unwrap();
        fs::create_dir_all(dir.join("mac/Navigation/L2")).unwrap();
        for f in ["2_42_LN", "3_03_E_C", "2_1_1LD"] {
            fs::write(l2.join(f), b"").unwrap();
        }
        fs::write(dir.join("mac/Navigation/L2/2.08.LN"), b"").unwrap();

        let files = GameFiles::new(dir.join("mac"), Some(&dir.join("pc")));
        assert!(files.has_pc());
        assert_eq!(files.find("Navigation/L2", "2.08.LN"), Some(dir.join("mac/Navigation/L2/2.08.LN")));
        assert_eq!(files.find("Navigation/L2", "2.42.LN"), Some(l2.join("2_42_LN")));
        assert_eq!(files.find("Navigation/L2", "3.03.E.CW"), Some(l2.join("3_03_E_C")));
        assert_eq!(files.find("Navigation/L2", "2.1.1.L.D"), Some(l2.join("2_1_1LD")));
        assert_eq!(files.find("Navigation/L2", "2.43.LN"), None);
        assert_eq!(files.find("Messages", "LockedDoorsTxt"), None);
        assert!(files.find("navigation/l2", "2.08.ln").is_some(), "case doesn't matter");
        fs::remove_dir_all(dir).ok();
    }
}
