//! Director 3 movies.
//!
//! Before Director 4's RIFF-based files, a Director movie (and a projector,
//! which embeds one) kept everything as Mac resources:
//!
//! | resource | contents                                                    |
//! |----------|-------------------------------------------------------------|
//! | `VWCR`   | the cast: one record per member (type, flags, bounds)       |
//! | `VWCI`   | per-member info strings: attached script, name, file name   |
//! | `STXT`   | text members (text fields), including Lingo kept in fields  |
//! | `BITD`   | bitmap members; `CLUT` palettes; `snd ` sounds              |
//! | `VWSC`   | the score (sprites per frame)                               |
//! | `VWAC`   | score scripts ("actions") referenced from the score         |
//! | `VWLB`   | frame labels                                                |
//!
//! A member's resources share one ID: 1024 + its cast number. Director 3
//! interpreted Lingo from source text, so scripts are stored as plain text.

pub mod bitmap;

use std::collections::{BTreeMap, HashMap};

use macformats::rsrc;
use thiserror::Error;

pub use bitmap::{Bitmap, BitmapInfo, Rect};

/// Resource ID of cast member 0; member `n` uses `CAST_ID_BASE + n`.
pub const CAST_ID_BASE: i16 = 1024;

#[derive(Debug, Error)]
pub enum DirectorError {
    #[error("unexpected end of {0} resource")]
    Truncated(&'static str),
    #[error("no {0} resource; is this a Director 3 movie?")]
    Missing(&'static str),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Mac(#[from] macformats::MacError),
}

pub type Result<T> = std::result::Result<T, DirectorError>;

/// What a text member holds, guessed from its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// Lingo source (`macro`/`on` handlers, `--` comments).
    Script,
    /// A data table: first line starts with `*`, e.g. `** Node`.
    Table,
    /// Anything else (on-screen text, lists of codes).
    Text,
}

#[derive(Debug, Clone)]
pub struct TextMember {
    pub cast: u16,
    /// From `VWCI`, if the member has a name.
    pub name: Option<String>,
    /// The text with Mac line endings converted to `\n`.
    pub text: String,
}

impl TextMember {
    pub fn kind(&self) -> TextKind {
        let first = self.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        if first.starts_with('*') {
            TextKind::Table
        } else if first.starts_with("--")
            || self.text.lines().any(|l| {
                let l = l.trim_start().to_ascii_lowercase();
                l.starts_with("macro ") || l.starts_with("on ")
            })
        {
            TextKind::Script
        } else {
            TextKind::Text
        }
    }

    /// A short title: the member's name, else the header of a script
    /// (`-- Game` gives `Game`) or table (`** Node` gives `Node`).
    pub fn title(&self) -> Option<String> {
        if let Some(n) = self.name.as_ref().filter(|n| !n.is_empty()) {
            return Some(n.clone());
        }
        let first_text_line = self
            .text
            .lines()
            .map(|l| l.trim_matches(|c: char| c == '-' || c == '*' || c.is_whitespace()))
            .find(|l| !l.is_empty())?;
        let word: String = first_text_line.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
        (!word.is_empty() && word.len() <= 32).then_some(word)
    }
}

/// A score script, referenced from score frames by number.
#[derive(Debug, Clone)]
pub struct Action {
    pub id: u8,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Label {
    pub frame: u16,
    pub name: String,
}

/// The movie's configuration (`VWCF`).
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// The real file version, at offset 36: Director 3.0 = 0x404,
    /// 3.1 = 0x405. (The field at offset 2 stayed 0x400 from Director 2 to 3.)
    pub version: u16,
    /// Stage rectangle on the authoring screen; its size is the stage size.
    pub stage: Rect,
    /// Cast number of the first `VWCR` record.
    pub first_cast: u16,
    pub last_cast: u16,
}

impl Config {
    pub fn stage_size(&self) -> (usize, usize) {
        (self.stage.width(), self.stage.height())
    }
}

/// Cast member types (first byte of a `VWCR` record).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastType {
    Bitmap,
    FilmLoop,
    Text,
    Palette,
    Picture,
    Sound,
    Button,
    Shape,
    Movie,
    DigitalVideo,
    Script,
    Other(u8),
}

impl CastType {
    fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Bitmap,
            2 => Self::FilmLoop,
            3 => Self::Text,
            4 => Self::Palette,
            5 => Self::Picture,
            6 => Self::Sound,
            7 => Self::Button,
            8 => Self::Shape,
            9 => Self::Movie,
            10 => Self::DigitalVideo,
            11 => Self::Script,
            n => Self::Other(n),
        }
    }
}

pub struct Movie {
    resources: HashMap<([u8; 4], i16), rsrc::Resource>,
}

fn be16(d: &[u8], at: usize, what: &'static str) -> Result<u16> {
    d.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or(DirectorError::Truncated(what))
}

fn be32(d: &[u8], at: usize, what: &'static str) -> Result<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(DirectorError::Truncated(what))
}

fn mac_text(bytes: &[u8]) -> String {
    macformats::macroman::decode(bytes).replace('\r', "\n")
}

impl Movie {
    /// Load from the resource fork of a Director 3 movie or projector.
    pub fn from_resource_fork(fork: &[u8]) -> Result<Self> {
        let resources: HashMap<_, _> = rsrc::parse(fork)?.into_iter().map(|r| ((r.kind, r.id), r)).collect();
        if !resources.keys().any(|(k, _)| k == b"VWCR") {
            return Err(DirectorError::Missing("VWCR"));
        }
        Ok(Self { resources })
    }

    pub fn resource(&self, kind: &[u8; 4], id: i16) -> Option<&[u8]> {
        self.resources.get(&(*kind, id)).map(|r| r.data.as_slice())
    }

    fn ids_of(&self, kind: &[u8; 4]) -> Vec<i16> {
        let mut ids: Vec<i16> = self.resources.keys().filter(|(k, _)| k == kind).map(|(_, id)| *id).collect();
        ids.sort();
        ids
    }

    /// The strings stored in a member's `VWCI` resource: `[0]` attached script,
    /// `[1]` name (a Pascal string), `[2]` folder, `[3]` file name, ...
    pub fn cast_info(&self, cast: u16) -> Result<Option<Vec<Vec<u8>>>> {
        let Some(d) = self.resource(b"VWCI", CAST_ID_BASE + cast as i16) else {
            return Ok(None);
        };
        let table = be32(d, 0, "VWCI")? as usize;
        let count = be16(d, table, "VWCI")? as usize;
        let offsets: Vec<usize> = (0..=count)
            .map(|i| be32(d, table + 2 + 4 * i, "VWCI").map(|o| o as usize))
            .collect::<Result<_>>()?;
        let base = table + 2 + 4 * (count + 1);
        let strings = (0..count)
            .map(|i| {
                d.get(base + offsets[i]..base + offsets[i + 1])
                    .map(|s| s.to_vec())
                    .ok_or(DirectorError::Truncated("VWCI"))
            })
            .collect::<Result<_>>()?;
        Ok(Some(strings))
    }

    /// A member's name, if it has one.
    pub fn cast_name(&self, cast: u16) -> Result<Option<String>> {
        let Some(strings) = self.cast_info(cast)? else {
            return Ok(None);
        };
        Ok(strings.get(1).and_then(|s| {
            let len = *s.first()? as usize;
            let name = macformats::macroman::decode(s.get(1..1 + len)?);
            (!name.is_empty()).then_some(name)
        }))
    }

    /// Every named member.
    pub fn cast_names(&self) -> Result<BTreeMap<u16, String>> {
        let mut out = BTreeMap::new();
        for id in self.ids_of(b"VWCI") {
            let cast = (id - CAST_ID_BASE) as u16;
            if let Some(name) = self.cast_name(cast)? {
                out.insert(cast, name);
            }
        }
        Ok(out)
    }

    /// Every text member. `STXT` layout: u32 header length, u32 text length,
    /// u32 style length, then the text.
    pub fn texts(&self) -> Result<Vec<TextMember>> {
        let mut out = Vec::new();
        for id in self.ids_of(b"STXT") {
            let d = self.resource(b"STXT", id).unwrap_or_default();
            let header = be32(d, 0, "STXT")? as usize;
            let len = be32(d, 4, "STXT")? as usize;
            let text = d.get(header..header + len).ok_or(DirectorError::Truncated("STXT"))?;
            let cast = (id - CAST_ID_BASE) as u16;
            out.push(TextMember {
                cast,
                name: self.cast_name(cast)?,
                text: mac_text(text),
            });
        }
        Ok(out)
    }

    /// Scripts attached to members (string 0 of their `VWCI`).
    pub fn member_scripts(&self) -> Result<Vec<TextMember>> {
        let mut out = Vec::new();
        for id in self.ids_of(b"VWCI") {
            let cast = (id - CAST_ID_BASE) as u16;
            if let Some(script) = self.cast_info(cast)?.and_then(|s| s.into_iter().next())
                && !script.iter().all(|b| b.is_ascii_whitespace())
            {
                out.push(TextMember {
                    cast,
                    name: self.cast_name(cast)?,
                    text: mac_text(&script),
                });
            }
        }
        Ok(out)
    }

    /// Score scripts. `VWAC` layout: u16 count, then 4-byte entries
    /// (u8 id, u8 kind, u16 offset) ending with id 0, then the texts back to
    /// back (each entry's text runs to the next entry's offset).
    pub fn actions(&self) -> Result<Vec<Action>> {
        let Some(d) = self.resource(b"VWAC", CAST_ID_BASE) else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        let mut p = 2;
        loop {
            let id = *d.get(p).ok_or(DirectorError::Truncated("VWAC"))?;
            let offset = be16(d, p + 2, "VWAC")? as usize;
            entries.push((id, offset));
            p += 4;
            if id == 0 {
                break;
            }
        }
        let base = p;
        Ok(entries
            .windows(2)
            .filter_map(|w| {
                let text = d.get(base + w[0].1..base + w[1].1)?;
                Some(Action { id: w[0].0, text: mac_text(text) })
            })
            .collect())
    }

    /// The movie configuration from `VWCF`. Layout: u16 length, u16 file
    /// version (stuck at 0x400), stage rect (top, left, bottom, right),
    /// u16 first cast, u16 last cast, ..., i16 real version at offset 36.
    pub fn config(&self) -> Result<Config> {
        let d = self.resource(b"VWCF", CAST_ID_BASE).ok_or(DirectorError::Missing("VWCF"))?;
        let stage = Rect::parse(d.get(4..12).ok_or(DirectorError::Truncated("VWCF"))?)
            .ok_or(DirectorError::Truncated("VWCF"))?;
        let version = if d.len() > 37 { be16(d, 36, "VWCF")? } else { be16(d, 2, "VWCF")? };
        Ok(Config {
            version,
            stage,
            first_cast: be16(d, 12, "VWCF")?,
            last_cast: be16(d, 14, "VWCF")?,
        })
    }

    /// Cast records from `VWCR`, keyed by cast number. Each record starts
    /// with a size byte (0 = empty slot); records are numbered from
    /// `Config::first_cast`. The returned bytes start with the type byte.
    pub fn cast_records(&self) -> Result<BTreeMap<u16, Vec<u8>>> {
        let d = self.resource(b"VWCR", CAST_ID_BASE).ok_or(DirectorError::Missing("VWCR"))?;
        let mut cast = self.config()?.first_cast;
        let mut out = BTreeMap::new();
        let mut p = 0;
        while p < d.len() {
            let size = d[p] as usize;
            if size > 0 {
                let rec = d.get(p + 1..p + 1 + size).ok_or(DirectorError::Truncated("VWCR"))?;
                out.insert(cast, rec.to_vec());
            }
            p += 1 + size;
            cast += 1;
        }
        Ok(out)
    }

    pub fn cast_type(&self, cast: u16) -> Result<Option<CastType>> {
        Ok(self.cast_records()?.get(&cast).and_then(|r| r.first()).map(|&b| CastType::from_byte(b)))
    }

    /// Decode one bitmap member.
    pub fn bitmap(&self, cast: u16) -> Result<Option<Bitmap>> {
        let records = self.cast_records()?;
        self.bitmap_with(&records, cast)
    }

    fn bitmap_with(&self, records: &BTreeMap<u16, Vec<u8>>, cast: u16) -> Result<Option<Bitmap>> {
        let Some(rec) = records.get(&cast).filter(|r| r.first() == Some(&1)) else {
            return Ok(None);
        };
        let Some(data) = self.resource(b"BITD", CAST_ID_BASE + cast as i16) else {
            return Ok(None);
        };
        Ok(Some(bitmap::decode(cast, BitmapInfo::parse(rec)?, data)?))
    }

    /// Decode every bitmap member.
    pub fn bitmaps(&self) -> Result<Vec<Bitmap>> {
        let records = self.cast_records()?;
        let mut out = Vec::new();
        for &cast in records.keys() {
            if let Some(b) = self.bitmap_with(&records, cast)? {
                out.push(b);
            }
        }
        Ok(out)
    }

    /// Find a member by name.
    pub fn cast_by_name(&self, name: &str) -> Result<Option<u16>> {
        Ok(self.cast_names()?.into_iter().find(|(_, n)| n == name).map(|(c, _)| c))
    }

    /// Frame labels. `VWLB` layout: u16 count, then count + 1 entries of
    /// (u16 frame, u16 offset), then the names back to back.
    pub fn labels(&self) -> Result<Vec<Label>> {
        let Some(d) = self.resource(b"VWLB", CAST_ID_BASE) else {
            return Ok(Vec::new());
        };
        let count = be16(d, 0, "VWLB")? as usize;
        let entries: Vec<(u16, usize)> = (0..=count)
            .map(|i| Ok((be16(d, 2 + 4 * i, "VWLB")?, be16(d, 4 + 4 * i, "VWLB")? as usize)))
            .collect::<Result<_>>()?;
        let base = 2 + 4 * (count + 1);
        Ok(entries
            .windows(2)
            .filter_map(|w| {
                let name = d.get(base + w[0].1..base + w[1].1)?;
                Some(Label { frame: w[0].0, name: mac_text(name) })
            })
            .collect())
    }
}
