//! Classic StuffIt archives (`SIT!` signature, StuffIt 1.5 - 4.x).
//!
//! A 22-byte archive header, then entries. Each entry is a 112-byte header
//! followed by the resource fork's packed bytes, then the data fork's:
//!
//! ```text
//! 0    u8     resource fork method      (32 = folder start, 33 = folder end)
//! 1    u8     data fork method
//! 2    Str63  name
//! 66   OSType file type, 70 OSType creator, 74 u16 Finder flags
//! 76   u32    created, 80 u32 modified
//! 84   u32    resource length, 88 data length
//! 92   u32    resource packed length, 96 data packed length
//! 100  u16    resource CRC, 102 data CRC, 104..109 reserved, 110 u16 header CRC
//! ```
//!
//! Only method 0 (stored) is supported, which is what the Iron Helix Mac
//! installers use.

use crate::{BeBytes, MacError, Result, macroman};

const FOLDER_START: u8 = 32;
const FOLDER_END: u8 = 33;
const HEADER: usize = 112;

#[derive(Debug, Clone, Copy)]
pub struct Fork {
    pub offset: usize,
    pub packed_len: usize,
    pub len: usize,
    pub method: u8,
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Folder names and the file name, outermost first.
    pub path: Vec<String>,
    pub file_type: [u8; 4],
    pub creator: [u8; 4],
    pub finder_flags: u16,
    pub data: Fork,
    pub rsrc: Fork,
}

pub struct Archive<'a> {
    data: &'a [u8],
    /// Files only; folders appear in each file's path.
    pub entries: Vec<Entry>,
}

pub fn is_sit1(data: &[u8]) -> bool {
    data.starts_with(b"SIT!") && data.get(10..14) == Some(b"rLau")
}

impl<'a> Archive<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if !is_sit1(data) {
            return Err(MacError::Invalid("not a classic StuffIt archive".into()));
        }
        let end = (data.be_u32(6)? as usize).min(data.len());
        let mut entries = Vec::new();
        let mut folders: Vec<String> = Vec::new();
        let mut p = 22;

        while p + HEADER <= end {
            let h = data.be_slice(p, HEADER)?;
            let (rm, dm) = (h[0], h[1]);
            let name_len = (h[2] as usize).min(63);
            let name = macroman::decode(&h[3..3 + name_len]);

            if rm == FOLDER_START || dm == FOLDER_START {
                folders.push(name);
                p += HEADER;
                continue;
            }
            if rm == FOLDER_END || dm == FOLDER_END {
                folders.pop();
                p += HEADER;
                continue;
            }

            let rsrc_len = h.be_u32(84)? as usize;
            let data_len = h.be_u32(88)? as usize;
            let rsrc_packed = h.be_u32(92)? as usize;
            let data_packed = h.be_u32(96)? as usize;
            let rsrc = Fork {
                offset: p + HEADER,
                packed_len: rsrc_packed,
                len: rsrc_len,
                method: rm & 0x0F,
            };
            let data_fork = Fork {
                offset: p + HEADER + rsrc_packed,
                packed_len: data_packed,
                len: data_len,
                method: dm & 0x0F,
            };
            let mut path = folders.clone();
            path.push(name);
            entries.push(Entry {
                path,
                file_type: h[66..70].try_into().unwrap(),
                creator: h[70..74].try_into().unwrap(),
                finder_flags: h.be_u16(74)?,
                data: data_fork,
                rsrc,
            });
            p += HEADER + rsrc_packed + data_packed;
        }
        Ok(Self { data, entries })
    }

    /// Unpack one fork.
    pub fn extract(&self, fork: &Fork) -> Result<Vec<u8>> {
        let packed = self.data.be_slice(fork.offset, fork.packed_len)?;
        match fork.method {
            0 => Ok(packed.to_vec()),
            m => Err(MacError::Unsupported(format!("classic StuffIt compression method {m}"))),
        }
    }
}
