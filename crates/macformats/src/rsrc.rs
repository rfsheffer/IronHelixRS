//! Resource forks.
//!
//! Layout: a 16-byte header (data offset, map offset, data length, map
//! length), the resource data area (each resource prefixed by a u32 length),
//! and the resource map, which lists types and, per type, the resources.

use crate::{BeBytes, Result};

#[derive(Debug, Clone)]
pub struct Resource {
    pub kind: [u8; 4],
    pub id: i16,
    pub data: Vec<u8>,
}

impl Resource {
    pub fn kind_str(&self) -> String {
        crate::macroman::decode(&self.kind)
    }
}

pub fn parse(fork: &[u8]) -> Result<Vec<Resource>> {
    let data_off = fork.be_u32(0)? as usize;
    let map_off = fork.be_u32(4)? as usize;
    let map = &fork[map_off..];
    let type_list = map.be_u16(24)? as usize;
    let tl = &map[type_list..];

    let num_types = tl.be_u16(0)?.wrapping_add(1) as usize;
    let mut out = Vec::new();
    for t in 0..num_types {
        let rec = 2 + t * 8;
        let kind: [u8; 4] = tl.be_slice(rec, 4)?.try_into().unwrap();
        let count = tl.be_u16(rec + 4)? as usize + 1;
        let refs = tl.be_u16(rec + 6)? as usize;
        for r in 0..count {
            let entry = refs + r * 12;
            let id = tl.be_u16(entry)? as i16;
            let offset = (tl.be_u32(entry + 4)? & 0x00FF_FFFF) as usize;
            let at = data_off + offset;
            let len = fork.be_u32(at)? as usize;
            out.push(Resource {
                kind,
                id,
                data: fork.be_slice(at + 4, len)?.to_vec(),
            });
        }
    }
    Ok(out)
}
