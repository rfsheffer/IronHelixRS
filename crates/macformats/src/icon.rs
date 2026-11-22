//! Finder icons: the 8-bit color icons (`icl8` 32x32, `ics8` 16x16) and the
//! 1-bit masks that go with them (`ICN#`, `ics#`: an image, then its mask).

use crate::rsrc::Resource;

/// An icon as RGBA pixels, `size` x `size`.
pub struct Icon {
    pub size: usize,
    pub rgba: Vec<u8>,
}

impl Icon {
    /// The same icon `scale` times larger (nearest neighbor).
    pub fn scaled(&self, scale: usize) -> Icon {
        let size = self.size * scale;
        let mut rgba = vec![0; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let s = ((y / scale) * self.size + x / scale) * 4;
                rgba[(y * size + x) * 4..][..4].copy_from_slice(&self.rgba[s..s + 4]);
            }
        }
        Icon { size, rgba }
    }
}

/// The large (32) and small (16) color icons of family `id`, colored with
/// `palette` (the Mac's 8-bit system palette, as RGB by index).
pub fn family(resources: &[Resource], id: i16, palette: &[[u8; 3]; 256]) -> Option<(Icon, Icon)> {
    let get = |kind: &[u8; 4]| resources.iter().find(|r| &r.kind == kind && r.id == id).map(|r| r.data.as_slice());
    let large = decode(get(b"icl8")?, get(b"ICN#")?.get(128..256)?, 32, palette)?;
    let small = decode(get(b"ics8")?, get(b"ics#")?.get(32..64)?, 16, palette)?;
    Some((large, small))
}

fn decode(pixels: &[u8], mask: &[u8], size: usize, palette: &[[u8; 3]; 256]) -> Option<Icon> {
    if pixels.len() < size * size || mask.len() < size * size / 8 {
        return None;
    }
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let opaque = mask[y * size / 8 + x / 8] >> (7 - x % 8) & 1 == 1;
            let [r, g, b] = palette[pixels[y * size + x] as usize];
            rgba.extend_from_slice(&[r, g, b, if opaque { 255 } else { 0 }]);
        }
    }
    Some(Icon { size, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_and_scales() {
        let mut palette = [[0u8; 3]; 256];
        palette[5] = [10, 20, 30];
        let res = |kind: &[u8; 4], data: Vec<u8>| Resource { kind: *kind, id: 1, data };
        // Everything pixel 5; the masks open only the first row.
        let mut icn = vec![0u8; 256];
        icn[128..132].fill(0xFF);
        let mut ics = vec![0u8; 64];
        ics[32..34].fill(0xFF);
        let r = [res(b"icl8", vec![5; 1024]), res(b"ICN#", icn), res(b"ics8", vec![5; 256]), res(b"ics#", ics)];
        let (large, small) = family(&r, 1, &palette).unwrap();
        assert_eq!(&large.rgba[..4], &[10, 20, 30, 255]);
        assert_eq!(large.rgba[32 * 4 + 3], 0);
        assert_eq!(small.size, 16);
        let big = large.scaled(2);
        assert_eq!(big.size, 64);
        assert_eq!(&big.rgba[4..8], &[10, 20, 30, 255]);
        assert_eq!(big.rgba[2 * 64 * 4 + 3], 0);
    }
}
