//! The game data folder, `IronHelixData`: what `pack data` builds from the
//! player's own copies of the game, and where the game looks first.
//!
//! ```text
//! IronHelixData/
//!   manifest.txt                 version and the sources it was built from
//!   mac/game.rsrc                the Mac game program's resource fork
//!   mac/files/...                the Mac CD's `Iron Helix™ Files` folder
//!   mac/Iron Helix Sounds/...    the installed sounds
//!   mac/Misc Files/...           the installed movies
//!   pc/HELIX.EXE, pc/NAV/...     from the PC CD: the hidden rooms' node data
//!   pc/SEGUES/CREDITS,           and movies, the scrolling credits and their
//!   pc/OPENING/CREDTUNE.WAV      tune (all optional)
//!   sega/theme.wav               the Sega CD's theme, from its audio track
//!   sega/SEGUES/ENDING.SEG       the Sega CD's extended ending (optional)
//! ```
//!
//! The PC and Sega CD parts keep those discs' own folder names, so the game
//! reads them the same way as the discs themselves.

use std::path::{Path, PathBuf};

/// The folder's name, looked for next to the game.
pub const FOLDER: &str = "IronHelixData";
pub const MANIFEST: &str = "manifest.txt";
/// The manifest's first line.
pub const VERSION: &str = "IronHelixData 1";

pub const MAC_PROGRAM: &str = "mac/game.rsrc";
pub const MAC_FILES: &str = "mac/files";
/// The installed game's folder: `Iron Helix Sounds` and `Misc Files`.
pub const MAC_INSTALLED: &str = "mac";
pub const PC: &str = "pc";
pub const SEGA: &str = "sega";
pub const SEGA_THEME: &str = "sega/theme.wav";

/// Whether `dir` is a data folder (it has the manifest and the program).
pub fn is_data_folder(dir: &Path) -> bool {
    crate::vfs::is_file(&dir.join(MANIFEST)) && crate::vfs::is_file(&dir.join(MAC_PROGRAM))
}

/// A part of the data folder, if it's there.
pub fn part(dir: &Path, rel: &str) -> Option<PathBuf> {
    let p = rel.split('/').fold(dir.to_path_buf(), |p, s| p.join(s));
    crate::vfs::exists(&p).then_some(p)
}
