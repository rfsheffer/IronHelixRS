//! Where the game's data is read from: ordinary files, unless the program
//! installs another [`Source`] at startup (on Android, the data packed
//! inside the app, which isn't a folder of files). Game data goes through
//! these functions; saved games and other files the player writes don't.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A place to read the game's files from.
pub trait Source: Send + Sync {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// The names in a folder (files and folders), in no particular order.
    fn list(&self, dir: &Path) -> Vec<String>;
}

/// Ordinary files.
pub struct Disk;

impl Source for Disk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn list(&self, dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
    }
}

static SOURCE: OnceLock<Box<dyn Source>> = OnceLock::new();

/// Read the game's data from `source` from now on (once, at startup).
pub fn set_source(source: Box<dyn Source>) {
    let _ = SOURCE.set(source);
}

fn source() -> &'static dyn Source {
    SOURCE.get().map_or(&Disk, |s| s.as_ref())
}

pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    source().read(path)
}

pub fn is_file(path: &Path) -> bool {
    source().is_file(path)
}

pub fn is_dir(path: &Path) -> bool {
    source().is_dir(path)
}

pub fn exists(path: &Path) -> bool {
    is_file(path) || is_dir(path)
}

pub fn list(dir: &Path) -> Vec<String> {
    source().list(dir)
}

/// A source built from a list of its files' paths (`/`-separated, relative
/// to `root`), reading each file through `read`: for packed data that can't
/// be listed itself, like an Android app's assets. A line may also give the
/// name the file is stored under, then a tab, then its path, for packages
/// that can't hold the real names.
pub struct Indexed<F> {
    root: PathBuf,
    /// Each file's path and the path it's read from.
    files: HashMap<String, String>,
    dirs: HashMap<String, Vec<String>>,
    read: F,
}

impl<F: Fn(&str) -> io::Result<Vec<u8>> + Send + Sync> Indexed<F> {
    /// `index`: one file per line, its path relative to `root` (after the
    /// name it's stored under and a tab, if that differs).
    pub fn new(root: impl Into<PathBuf>, index: &str, read: F) -> Self {
        let root = root.into();
        let root_key = key(&root);
        let under_root = |rel: &str| if root_key.is_empty() { rel.to_string() } else { format!("{root_key}/{rel}") };
        let mut files = HashMap::new();
        let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
        for line in index.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty()) {
            let (stored, rel) = line.split_once('\t').unwrap_or((line, line));
            let full = under_root(rel);
            // Every folder on the way lists the next part.
            let mut parent = String::new();
            for part in full.split('/') {
                let children = dirs.entry(parent.clone()).or_default();
                if !children.iter().any(|c| c == part) {
                    children.push(part.to_string());
                }
                parent = if parent.is_empty() { part.to_string() } else { format!("{parent}/{part}") };
            }
            files.insert(full, under_root(stored));
        }
        Self { root, files, dirs, read }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// A path as the index writes it: `/`-separated, without `.` parts.
fn key(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

impl<F: Fn(&str) -> io::Result<Vec<u8>> + Send + Sync> Source for Indexed<F> {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let k = key(path);
        match self.files.get(&k) {
            Some(stored) => (self.read)(stored),
            None => Err(io::Error::new(io::ErrorKind::NotFound, format!("{k} isn't in the packed data"))),
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(&key(path))
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.contains_key(&key(path))
    }

    fn list(&self, dir: &Path) -> Vec<String> {
        self.dirs.get(&key(dir)).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_lists_and_reads() {
        let index = "mac/game.rsrc\nmac/files/Navigation/L1/1.01.FN\nmac/files/Navigation/L1/1.01.T.0\n";
        let s = Indexed::new("IronHelixData", index, |p: &str| Ok(p.as_bytes().to_vec()));
        assert!(s.is_file(Path::new("IronHelixData/mac/game.rsrc")));
        assert!(s.is_dir(Path::new("IronHelixData/mac/files/Navigation")));
        assert!(!s.is_file(Path::new("IronHelixData/mac")));
        let mut names = s.list(Path::new("IronHelixData/mac/files/Navigation/L1"));
        names.sort();
        assert_eq!(names, ["1.01.FN", "1.01.T.0"]);
        assert_eq!(s.list(Path::new("IronHelixData")), ["mac"]);
        assert_eq!(s.read(Path::new("./IronHelixData/mac/game.rsrc")).unwrap(), b"IronHelixData/mac/game.rsrc");
        assert!(s.read(Path::new("IronHelixData/mac/nope")).is_err());
    }

    #[test]
    fn indexed_reads_stored_names() {
        let index = "0\tmac/files/Opening/Iron Helix\u{2122} Tune\r\n1\tmac/game.rsrc\r\n";
        let s = Indexed::new("IronHelixData", index, |p: &str| Ok(p.as_bytes().to_vec()));
        assert_eq!(s.list(Path::new("IronHelixData/mac/files/Opening")), ["Iron Helix\u{2122} Tune"]);
        assert_eq!(s.read(Path::new("IronHelixData/mac/files/Opening/Iron Helix\u{2122} Tune")).unwrap(), b"IronHelixData/0");
        assert_eq!(s.read(Path::new("IronHelixData/mac/game.rsrc")).unwrap(), b"IronHelixData/1");
    }
}
