//! ISO9660 file systems on CD images.
//!
//! Disc rips come in two sector layouts:
//!
//! - **cooked**: 2048 bytes of user data per sector (`.iso`)
//! - **raw**: full 2352-byte sectors (`.img`/`.bin` with a `.cue`). For Mode 1
//!   data, each sector is a 12-byte sync pattern, 4-byte header, 2048 bytes of
//!   user data, then error correction codes.
//!
//! The Primary Volume Descriptor lives in sector 16 and points at the root
//! directory. Directory records are little-endian (ISO9660 stores both
//! byte orders; we read the LE copy).

use std::io::{Read, Seek, SeekFrom};

use thiserror::Error;

pub const USER_DATA: usize = 2048;
const RAW_SECTOR: usize = 2352;
const SYNC: [u8; 12] = [0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0];

#[derive(Debug, Error)]
pub enum CdError {
    #[error("no ISO9660 volume descriptor found")]
    NotIso,
    #[error("invalid directory record: {0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CdError>;

#[derive(Debug, Clone)]
pub struct Entry {
    /// Path from the root, `/`-separated, without the `;1` version suffix.
    pub path: String,
    pub sector: u32,
    pub len: u32,
    pub is_dir: bool,
}

pub struct Disc<R> {
    reader: R,
    raw: bool,
    pub volume_id: String,
    pub system_id: String,
}

impl<R: Read + Seek> Disc<R> {
    /// Open an image, detecting raw vs cooked sectors from the sync pattern.
    pub fn open(mut reader: R) -> Result<Self> {
        let mut sync = [0u8; 12];
        reader.seek(SeekFrom::Start(0))?;
        reader.read_exact(&mut sync)?;
        let raw = sync == SYNC;

        let mut disc = Self {
            reader,
            raw,
            volume_id: String::new(),
            system_id: String::new(),
        };
        let pvd = disc.read_sector(16)?;
        if &pvd[1..6] != b"CD001" || pvd[0] != 1 {
            return Err(CdError::NotIso);
        }
        disc.system_id = ascii(&pvd[8..40]);
        disc.volume_id = ascii(&pvd[40..72]);
        Ok(disc)
    }

    /// Read one sector's 2048 bytes of user data.
    pub fn read_sector(&mut self, lba: u32) -> Result<[u8; USER_DATA]> {
        let pos = if self.raw {
            lba as u64 * RAW_SECTOR as u64 + 16
        } else {
            lba as u64 * USER_DATA as u64
        };
        let mut buf = [0u8; USER_DATA];
        self.reader.seek(SeekFrom::Start(pos))?;
        self.reader.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Read `len` bytes starting at sector `lba`.
    pub fn read(&mut self, lba: u32, len: u32) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(len as usize);
        let mut sector = lba;
        while out.len() < len as usize {
            let s = self.read_sector(sector)?;
            let take = (len as usize - out.len()).min(USER_DATA);
            out.extend_from_slice(&s[..take]);
            sector += 1;
        }
        Ok(out)
    }

    /// Every file and directory on the disc, depth first.
    pub fn entries(&mut self) -> Result<Vec<Entry>> {
        // The root's record is embedded in the PVD; its name is "\0" like ".".
        let pvd = self.read_sector(16)?;
        let root = &pvd[156..190];
        let lba = u32::from_le_bytes(root[2..6].try_into().unwrap());
        let len = u32::from_le_bytes(root[10..14].try_into().unwrap());
        let mut out = Vec::new();
        self.walk(lba, len, "", &mut out, 0)?;
        Ok(out)
    }

    fn walk(&mut self, lba: u32, len: u32, prefix: &str, out: &mut Vec<Entry>, depth: u32) -> Result<()> {
        if depth > 16 {
            return Err(CdError::Invalid("directory tree too deep".into()));
        }
        let data = self.read(lba, len)?;
        for sector in data.chunks(USER_DATA) {
            let mut p = 0;
            while p < sector.len() && sector[p] != 0 {
                let rec_len = sector[p] as usize;
                let Some((name, sub_lba, sub_len, is_dir)) = parse_record(&sector[p..p + rec_len])? else {
                    p += rec_len;
                    continue;
                };
                p += rec_len;
                let path = format!("{prefix}/{name}");
                out.push(Entry {
                    path: path.clone(),
                    sector: sub_lba,
                    len: sub_len,
                    is_dir,
                });
                if is_dir {
                    self.walk(sub_lba, sub_len, &path, out, depth + 1)?;
                }
            }
        }
        Ok(())
    }
}

/// Parse a directory record. Returns `None` for the `.` and `..` entries.
fn parse_record(r: &[u8]) -> Result<Option<(String, u32, u32, bool)>> {
    if r.len() < 34 {
        return Err(CdError::Invalid("record too short".into()));
    }
    let lba = u32::from_le_bytes(r[2..6].try_into().unwrap());
    let len = u32::from_le_bytes(r[10..14].try_into().unwrap());
    let is_dir = r[25] & 2 != 0;
    let name_len = r[32] as usize;
    let name = r
        .get(33..33 + name_len)
        .ok_or_else(|| CdError::Invalid("name past end of record".into()))?;
    if name == [0] || name == [1] {
        return Ok(None);
    }
    let name = ascii(name);
    let name = name.split(';').next().unwrap_or(&name).to_string();
    Ok(Some((name, lba, len, is_dir)))
}

fn ascii(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim_end().to_string()
}
