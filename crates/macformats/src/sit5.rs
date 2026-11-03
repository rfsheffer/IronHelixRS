//! StuffIt 5 archives (`StuffIt (c)1997-...` signature).
//!
//! Each entry has a fixed header, the name, then a second block with Finder
//! info and (optionally) resource fork sizes. The resource fork's compressed
//! bytes come first, then the data fork's. Only files are supported so far,
//! not folders, and only methods 0 (stored) and 15 (Arsenic).

use crate::macroman;
use crate::{BeBytes, MacError, Result, arsenic};

const SIGNATURE: &[u8] = b"StuffIt (c)1997-";
const ENTRY_MAGIC: u32 = 0xA5A5_A5A5;
const FLAG_FOLDER: u8 = 0x40;
const FLAG_ENCRYPTED: u8 = 0x20;

#[derive(Debug, Clone)]
pub struct Fork {
    pub offset: usize,
    pub compressed_len: usize,
    pub len: usize,
    pub method: u8,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub file_type: [u8; 4],
    pub creator: [u8; 4],
    pub finder_flags: u16,
    pub data: Fork,
    pub rsrc: Option<Fork>,
}

pub struct Archive<'a> {
    data: &'a [u8],
    pub entries: Vec<Entry>,
}

impl<'a> Archive<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if !data.starts_with(SIGNATURE) {
            return Err(MacError::Invalid("not a StuffIt 5 archive".into()));
        }
        let num_entries = data.be_u16(0x5C)? as usize;
        let mut offset = data.be_u32(0x5E)? as usize;

        let mut entries = Vec::with_capacity(num_entries);
        for _ in 0..num_entries {
            let (entry, next) = parse_entry(data, offset)?;
            entries.push(entry);
            offset = next;
        }
        Ok(Self { data, entries })
    }

    /// Decompress one fork.
    pub fn extract(&self, fork: &Fork) -> Result<Vec<u8>> {
        let packed = self.data.be_slice(fork.offset, fork.compressed_len)?;
        match fork.method {
            0 => Ok(packed.to_vec()),
            15 => arsenic::decompress(packed, fork.len),
            m => Err(MacError::Unsupported(format!("StuffIt compression method {m}"))),
        }
    }
}

/// Parse the entry at `at`; returns it and the offset just past its data.
fn parse_entry(d: &[u8], at: usize) -> Result<(Entry, usize)> {
    if d.be_u32(at)? != ENTRY_MAGIC {
        return Err(MacError::Invalid(format!("no StuffIt entry at offset {at:#x}")));
    }
    let version = d.be_u8(at + 4)?;
    let header_len = d.be_u16(at + 6)? as usize;
    let flags = d.be_u8(at + 9)?;
    if flags & FLAG_FOLDER != 0 {
        return Err(MacError::Unsupported("folders in StuffIt archives".into()));
    }
    if flags & FLAG_ENCRYPTED != 0 {
        return Err(MacError::Unsupported("encrypted StuffIt entries".into()));
    }
    let name_len = d.be_u16(at + 30)? as usize;
    let data_len = d.be_u32(at + 34)? as usize;
    let data_comp = d.be_u32(at + 38)? as usize;
    let data_method = d.be_u8(at + 46)?;
    let name = macroman_or_utf8(d.be_slice(at + 48, name_len)?);

    // Second block, right after the fixed header.
    let mut p = at + header_len;
    let flags2 = d.be_u16(p)?;
    let file_type = d.be_slice(p + 4, 4)?.try_into().unwrap();
    let creator = d.be_slice(p + 8, 4)?.try_into().unwrap();
    let finder_flags = d.be_u16(p + 12)?;
    p += 14 + if version == 1 { 22 } else { 18 };

    let mut rsrc = None;
    if flags2 & 1 != 0 {
        let len = d.be_u32(p)? as usize;
        let compressed_len = d.be_u32(p + 4)? as usize;
        let method = d.be_u8(p + 12)?;
        let pass_len = d.be_u8(p + 13)? as usize;
        p += 14 + pass_len;
        rsrc = Some(Fork {
            offset: 0,
            compressed_len,
            len,
            method,
        });
    }

    // Fork data: resource first, then data.
    if let Some(r) = &mut rsrc {
        r.offset = p;
        p += r.compressed_len;
    }
    let data = Fork {
        offset: p,
        compressed_len: data_comp,
        len: data_len,
        method: data_method,
    };
    p += data_comp;

    Ok((
        Entry {
            name,
            file_type,
            creator,
            finder_flags,
            data,
            rsrc,
        },
        p,
    ))
}

/// StuffIt 5 stores names as UTF-8 in newer archives and Mac Roman in older ones.
fn macroman_or_utf8(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => macroman::decode(bytes),
    }
}
