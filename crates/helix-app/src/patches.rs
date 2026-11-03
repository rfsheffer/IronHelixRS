//! Fixes for flaws in the original files, applied as they load.
//!
//! - The PC release's English location strips for the stasis room (4.08,
//!   4.09) are its German ones ("DECK 4 / ZUGANG BEGRENZT", "Zugang:
//!   Datenport"). English ones are rebuilt from two English strips in the
//!   same font: "DECK 4 /" from 4.04's ("DECK 4 / CORRIDOR") and the rest from
//!   the deck 2 hidden room's ("DECK 2/ Restricted Access", with "Access: Data
//!   Port" where the German has its second line). The typing animation is
//!   kept: each frame shows as much of "DECK 4 /" as the deck 2 strip has
//!   typed of "DECK 2/".

use crate::assets::GameAssets;
use crate::video::{Clip, decode};

/// German strip -> the deck 2 strip with the matching English lines.
const GERMAN_STRIPS: [(&str, &str); 8] = [
    ("4.08.T.0", "2.42.T.1"),
    ("4.08.T.1", "2.42.T.0"),
    ("4.08.T.2", "2.42.T.0"),
    ("4.08.T.3", "2.42.T.1"),
    ("4.09.T.0", "2.42.T.1"),
    ("4.09.T.1", "2.42.T.1"),
    ("4.09.T.2", "2.42.T.1"),
    ("4.09.T.3", "2.42.T.1"),
];

/// The strip with "DECK 4 /" in the same font and position.
const DECK_4_STRIP: &str = "4.04.T.0";
/// Rows of the first text line.
const LINE: std::ops::Range<usize> = 2..12;
/// "DECK 4 /" spans columns 0-41 of its strip; "DECK 2/" ends at column 39,
/// and the deck 2 text after it starts at 41 and moves right by 2.
const DECK_4_WIDTH: usize = 42;
const DECK_2_END: usize = 39;
const SHIFT: usize = 2;

/// An info-strip clip, patched if it's one of the known flawed ones.
pub fn info_clip(assets: &GameAssets, folder: &str, name: &str) -> Result<Clip, String> {
    if folder == "Navigation/L4"
        && let Some(&(_, english)) = GERMAN_STRIPS.iter().find(|(german, _)| *german == name)
        && let Ok(clip) = english_deck_4(assets, english)
    {
        return Ok(clip);
    }
    Clip::load(&assets.file(folder, name))
}

fn english_deck_4(assets: &GameAssets, deck_2_strip: &str) -> Result<Clip, String> {
    let mut frames = decode(&assets.file("Navigation/L2", deck_2_strip))?;
    let deck_4 = decode(&assets.file("Navigation/L4", DECK_4_STRIP))?.pop().ok_or("empty strip")?;
    let blank = frames.first().ok_or("empty strip")?.pixels.clone();
    let w = deck_4.width;
    if frames.iter().any(|f| f.width != w || f.pixels.len() != deck_4.pixels.len()) {
        return Err("strip sizes differ".into());
    }
    for f in &mut frames {
        f.pixels = rebuild_line(&f.pixels, &blank, &deck_4.pixels, w);
    }
    Ok(Clip::from_frames(&frames))
}

/// One frame of the deck 2 strip with "DECK 2/" replaced by "DECK 4 /".
fn rebuild_line(frame: &[u8], blank: &[u8], deck_4: &[u8], w: usize) -> Vec<u8> {
    let mut out = frame.to_vec();
    // How far the deck 2 strip has typed its first line.
    let typed = (0..w).filter(|&x| LINE.clone().any(|y| frame[y * w + x] != blank[y * w + x])).max();
    for y in LINE {
        let row = y * w;
        for x in 0..w {
            out[row + x] = if x < DECK_4_WIDTH {
                if typed.is_some_and(|t| t >= x.min(DECK_2_END)) { deck_4[row + x] } else { blank[row + x] }
            } else {
                frame[row + x - SHIFT]
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_the_deck_number_and_keeps_the_typing() {
        let w = 50;
        let blank = vec![0u8; w * 32];
        let deck_4 = vec![4u8; w * 32];
        // Deck 2 strip: typed through column 45 ("DECK 2/" plus a little).
        let mut frame = blank.clone();
        for y in LINE {
            for x in 0..=45 {
                frame[y * w + x] = 2;
            }
        }
        let out = rebuild_line(&frame, &blank, &deck_4, w);
        let y = LINE.start * w;
        assert_eq!(out[y], 4, "DECK 4 / replaces DECK 2/");
        assert_eq!(out[y + DECK_4_WIDTH - 1], 4);
        assert_eq!(out[y + 47], 2, "the rest moves right by 2");
        assert_eq!(out[y + 48], 0);
        assert_eq!(out[0], 0, "rows outside the line are untouched");

        // Nothing typed yet: nothing shown.
        assert_eq!(rebuild_line(&blank, &blank, &deck_4, w), blank);
        // Typed only "DE": only those columns of "DECK 4 /" show.
        let mut start = blank.clone();
        start[y + 10] = 2;
        let out = rebuild_line(&start, &blank, &deck_4, w);
        assert_eq!((out[y + 10], out[y + 11]), (4, 0));
    }
}
