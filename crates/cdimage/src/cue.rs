//! CD audio tracks from a `.cue` sheet and its raw (2352-byte sector) image.
//!
//! Only single-file sheets are handled (one `FILE`, as disc rips of Sega
//! CD games usually are). A track runs from its `INDEX 01` to the next
//! track's `INDEX 00` (or `INDEX 01`), or to the end of the file.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::{CdError, RAW_SECTOR, Result};

/// Audio sample rate of CD audio (16-bit little-endian stereo).
pub const CD_AUDIO_RATE: u32 = 44_100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Track {
    number: u32,
    audio: bool,
    /// Sector of INDEX 00 (pregap), if any.
    pregap: Option<u32>,
    /// Sector of INDEX 01.
    start: Option<u32>,
}

fn msf(s: &str) -> Option<u32> {
    let mut p = s.split(':').map(|n| n.parse::<u32>().ok());
    let (m, s, f) = (p.next()??, p.next()??, p.next()??);
    Some((m * 60 + s) * 75 + f)
}

/// The image file a cue sheet names, and its tracks.
fn parse(cue: &str) -> (Option<String>, Vec<Track>) {
    let mut file = None;
    let mut tracks: Vec<Track> = Vec::new();
    for line in cue.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("FILE ") {
            // FILE "name" BINARY
            let name = match rest.strip_prefix('"') {
                Some(r) => r.split('"').next().unwrap_or(""),
                None => rest.split_whitespace().next().unwrap_or(""),
            };
            file.get_or_insert_with(|| name.to_string());
        } else if let Some(rest) = line.strip_prefix("TRACK ") {
            let mut w = rest.split_whitespace();
            if let Some(n) = w.next().and_then(|n| n.parse().ok()) {
                tracks.push(Track { number: n, audio: w.next() == Some("AUDIO"), pregap: None, start: None });
            }
        } else if let Some(rest) = line.strip_prefix("INDEX ") {
            let mut w = rest.split_whitespace();
            let (index, at) = (w.next(), w.next().and_then(msf));
            if let (Some(t), Some(at)) = (tracks.last_mut(), at) {
                match index {
                    Some("00") => t.pregap = Some(at),
                    Some("01") => t.start = Some(at),
                    _ => {}
                }
            }
        }
    }
    (file, tracks)
}

/// Read audio track `number` of a cue sheet's image: 16-bit little-endian
/// stereo samples at 44.1 kHz.
pub fn audio_track(cue_path: &Path, number: u32) -> Result<Vec<u8>> {
    let cue = std::fs::read_to_string(cue_path)?;
    let (file, tracks) = parse(&cue);
    let file = file.ok_or_else(|| CdError::Invalid("cue sheet names no file".into()))?;
    let i = tracks
        .iter()
        .position(|t| t.number == number && t.audio)
        .ok_or_else(|| CdError::Invalid(format!("no audio track {number}")))?;
    let start = tracks[i].start.ok_or_else(|| CdError::Invalid(format!("track {number} has no INDEX 01")))?;
    let end = tracks.get(i + 1).and_then(|t| t.pregap.or(t.start));

    let image = cue_path.parent().unwrap_or(Path::new(".")).join(file);
    let mut f = std::fs::File::open(image)?;
    let file_len = f.seek(SeekFrom::End(0))?;
    let from = start as u64 * RAW_SECTOR as u64;
    let to = end.map_or(file_len, |e| (e as u64 * RAW_SECTOR as u64).min(file_len));
    if from >= to {
        return Err(CdError::Invalid(format!("track {number} is empty")));
    }
    f.seek(SeekFrom::Start(from))?;
    let mut pcm = vec![0; (to - from) as usize];
    f.read_exact(&mut pcm)?;
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_sega_cd_sheet() {
        let cue = "FILE \"Game (US).img\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 00 42:56:64\n    INDEX 01 42:58:64\n";
        let (file, tracks) = parse(cue);
        assert_eq!(file.as_deref(), Some("Game (US).img"));
        assert_eq!(tracks.len(), 2);
        assert!(!tracks[0].audio && tracks[1].audio);
        assert_eq!(tracks[1].pregap, Some((42 * 60 + 56) * 75 + 64));
        assert_eq!(tracks[1].start, Some(193_414));
    }
}
