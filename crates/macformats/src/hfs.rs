//! Read-only HFS (Mac OS Standard) volumes.
//!
//! The Master Directory Block (MDB) sits 1024 bytes into the volume. It
//! locates two B-trees, both stored as files:
//!
//! - the **catalog**, keyed by (parent directory ID, name), holding every
//!   folder and file record, and
//! - the **extents overflow** file, holding extra extents for fragmented
//!   files (each file record only has room for three).
//!
//! Space is handed out in allocation blocks; block N starts at
//! `first_alloc_sector * 512 + N * alloc_block_size`.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

use crate::{BeBytes, MacError, Result, macroman};

const ROOT_DIR_ID: u32 = 2;
const CATALOG_FILE_ID: u32 = 4;
const NODE_SIZE: usize = 512;

#[derive(Debug, Clone, Copy)]
pub struct Extent {
    pub start: u16,
    pub count: u16,
}

fn parse_extents(b: &[u8]) -> Result<[Extent; 3]> {
    let mut out = [Extent { start: 0, count: 0 }; 3];
    for (i, e) in out.iter_mut().enumerate() {
        e.start = b.be_u16(i * 4)?;
        e.count = b.be_u16(i * 4 + 2)?;
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct ForkInfo {
    pub len: u64,
    pub extents: [Extent; 3],
}

#[derive(Debug, Clone)]
pub struct File {
    pub id: u32,
    pub parent: u32,
    pub name: String,
    pub file_type: [u8; 4],
    pub creator: [u8; 4],
    pub finder_flags: u16,
    pub data: ForkInfo,
    pub rsrc: ForkInfo,
}

#[derive(Debug, Clone)]
pub struct Folder {
    pub id: u32,
    pub parent: u32,
    pub name: String,
}

pub struct Volume<R> {
    reader: R,
    pub name: String,
    alloc_block_size: u64,
    first_alloc_byte: u64,
    pub folders: HashMap<u32, Folder>,
    pub files: Vec<File>,
    /// Extra extents from the overflow file, keyed by (file ID, is_rsrc).
    overflow: HashMap<(u32, bool), Vec<Extent>>,
}

impl<R: Read + Seek> Volume<R> {
    pub fn open(mut reader: R) -> Result<Self> {
        let mut mdb = [0u8; 162];
        reader.seek(SeekFrom::Start(1024))?;
        reader.read_exact(&mut mdb)?;
        let mdb = &mdb[..];
        if mdb.be_u16(0)? != 0x4244 {
            return Err(MacError::Invalid("no HFS signature ('BD') at offset 1024".into()));
        }
        let alloc_block_size = mdb.be_u32(20)? as u64;
        let first_alloc_byte = mdb.be_u16(28)? as u64 * 512;
        let name_len = mdb.be_u8(36)? as usize;
        let name = macroman::decode(mdb.be_slice(37, name_len.min(27))?);
        let extents_file = ForkInfo {
            len: mdb.be_u32(130)? as u64,
            extents: parse_extents(mdb.be_slice(134, 12)?)?,
        };
        let catalog_file = ForkInfo {
            len: mdb.be_u32(146)? as u64,
            extents: parse_extents(mdb.be_slice(150, 12)?)?,
        };

        let mut vol = Self {
            reader,
            name,
            alloc_block_size,
            first_alloc_byte,
            folders: HashMap::new(),
            files: Vec::new(),
            overflow: HashMap::new(),
        };

        // The extents B-tree's own extents must fit in the MDB.
        let xt = vol.read_fork_from(&extents_file, &[])?;
        for (key, data) in leaf_records(&xt)? {
            // key: len, fork type (0 data / 0xFF rsrc), file ID, first block
            let is_rsrc = key.be_u8(1)? == 0xFF;
            let file_id = key.be_u32(2)?;
            let list = vol.overflow.entry((file_id, is_rsrc)).or_default();
            list.extend(parse_extents(data)?.into_iter().filter(|e| e.count > 0));
        }

        let cat_extra = vol.overflow.get(&(CATALOG_FILE_ID, false)).cloned().unwrap_or_default();
        let catalog = vol.read_fork_from(&catalog_file, &cat_extra)?;
        vol.load_catalog(&catalog)?;
        Ok(vol)
    }

    fn load_catalog(&mut self, catalog: &[u8]) -> Result<()> {
        for (key, data) in leaf_records(catalog)? {
            // key: len, reserved, parent ID, Str31 name
            let parent = key.be_u32(2)?;
            let name_len = key.be_u8(6)? as usize;
            let name = macroman::decode(key.be_slice(7, name_len)?);
            match data.be_u8(0)? {
                1 => {
                    let id = data.be_u32(6)?;
                    self.folders.insert(id, Folder { id, parent, name });
                }
                2 => self.files.push(File {
                    id: data.be_u32(20)?,
                    parent,
                    name,
                    file_type: data.be_slice(4, 4)?.try_into().unwrap(),
                    creator: data.be_slice(8, 4)?.try_into().unwrap(),
                    finder_flags: data.be_u16(12)?,
                    data: ForkInfo {
                        len: data.be_u32(26)? as u64,
                        extents: parse_extents(data.be_slice(74, 12)?)?,
                    },
                    rsrc: ForkInfo {
                        len: data.be_u32(36)? as u64,
                        extents: parse_extents(data.be_slice(86, 12)?)?,
                    },
                }),
                _ => {} // thread records
            }
        }
        Ok(())
    }

    /// Path of a folder from the volume root, as name components.
    pub fn folder_path(&self, mut id: u32) -> Vec<String> {
        let mut parts = Vec::new();
        while id != ROOT_DIR_ID {
            match self.folders.get(&id) {
                Some(f) => {
                    parts.push(f.name.clone());
                    id = f.parent;
                }
                None => break,
            }
        }
        parts.reverse();
        parts
    }

    pub fn read_data(&mut self, file: &File) -> Result<Vec<u8>> {
        let extra = self.overflow.get(&(file.id, false)).cloned().unwrap_or_default();
        self.read_fork_from(&file.data, &extra)
    }

    pub fn read_rsrc(&mut self, file: &File) -> Result<Vec<u8>> {
        let extra = self.overflow.get(&(file.id, true)).cloned().unwrap_or_default();
        self.read_fork_from(&file.rsrc, &extra)
    }

    fn read_fork_from(&mut self, fork: &ForkInfo, extra: &[Extent]) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(fork.len as usize);
        for e in fork.extents.iter().chain(extra) {
            if out.len() as u64 >= fork.len {
                break;
            }
            if e.count == 0 {
                continue;
            }
            let start = self.first_alloc_byte + e.start as u64 * self.alloc_block_size;
            let want = (e.count as u64 * self.alloc_block_size).min(fork.len - out.len() as u64);
            self.reader.seek(SeekFrom::Start(start))?;
            let at = out.len();
            out.resize(at + want as usize, 0);
            self.reader.read_exact(&mut out[at..])?;
        }
        if (out.len() as u64) < fork.len {
            return Err(MacError::Invalid(format!(
                "fork is {} bytes but its extents only cover {}",
                fork.len,
                out.len()
            )));
        }
        Ok(out)
    }
}

/// Walk a B-tree's leaf nodes in order and return every (key, data) record.
fn leaf_records(tree: &[u8]) -> Result<Vec<(&[u8], &[u8])>> {
    // Node 0 is the header node; its first record is the B-tree header.
    let first_leaf = tree.be_u32(14 + 10)? as usize;
    let mut records = Vec::new();
    let mut node_num = first_leaf;
    let mut visited = 0;
    while node_num != 0 {
        visited += 1;
        if visited > tree.len() / NODE_SIZE {
            return Err(MacError::Invalid("B-tree leaf chain loops".into()));
        }
        let node = tree.be_slice(node_num * NODE_SIZE, NODE_SIZE)?;
        let next = node.be_u32(0)? as usize;
        let kind = node.be_u8(8)? as i8;
        if kind != -1 {
            return Err(MacError::Invalid(format!("expected a leaf node, got type {kind}")));
        }
        let count = node.be_u16(10)? as usize;
        for i in 0..count {
            let start = node.be_u16(NODE_SIZE - 2 * (i + 1))? as usize;
            let end = node.be_u16(NODE_SIZE - 2 * (i + 2))? as usize;
            let rec = node.be_slice(start, end.saturating_sub(start))?;
            // Key length byte excludes itself; data starts at an even offset.
            let key_len = rec.be_u8(0)? as usize + 1;
            let data_at = (key_len + 1) & !1;
            records.push((&rec[..key_len], &rec[data_at.min(rec.len())..]));
        }
        node_num = next;
    }
    Ok(records)
}
