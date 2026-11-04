//! The window's icon: the Mac release's own, the probe (icon family -16455,
//! the custom icon in the game program's resource fork; the installed
//! folder's `Icon_` file carries the same one). It's read from the player's
//! files when the window opens, so nothing of the game ships with this
//! program; without them the window keeps the default icon.

use std::path::Path;

use macroquad::miniquad::conf::Icon;

/// The custom icon's resource ID (Finder's `kCustomIconResource`).
const CUSTOM_ICON: i16 = -16455;

/// The probe icon from the game program's resource fork, at 16, 32 and 64
/// pixels (the 64 is the 32 doubled; the Mac has no larger one).
pub fn mac_icon(rsrc: &Path) -> Option<Icon> {
    let fork = std::fs::read(rsrc).ok()?;
    let resources = macformats::rsrc::parse(&fork).ok()?;
    let get = |kind: &[u8; 4]| resources.iter().find(|r| &r.kind == kind && r.id == CUSTOM_ICON).map(|r| r.data.as_slice());
    // 8-bit pixels, and a 1-bit image followed by its 1-bit mask.
    let (large, large_mask) = (get(b"icl8")?, get(b"ICN#")?.get(128..256)?);
    let (small, small_mask) = (get(b"ics8")?, get(b"ics#")?.get(32..64)?);
    let palette = paco::Palette::mac_system();
    let rgba = |pixels: &[u8], mask: &[u8], size: usize, scale: usize, out: &mut [u8]| {
        let row_bytes = size / 8;
        let out_size = size * scale;
        for y in 0..out_size {
            for x in 0..out_size {
                let (sx, sy) = (x / scale, y / scale);
                let opaque = mask.get(sy * row_bytes + sx / 8).is_some_and(|b| b >> (7 - sx % 8) & 1 == 1);
                let c = palette.0[pixels.get(sy * size + sx).copied().unwrap_or(0) as usize];
                let o = (y * out_size + x) * 4;
                out[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, if opaque { 255 } else { 0 }]);
            }
        }
    };
    let mut icon = Icon { small: [0; 16 * 16 * 4], medium: [0; 32 * 32 * 4], big: [0; 64 * 64 * 4] };
    rgba(small, small_mask, 16, 1, &mut icon.small);
    rgba(large, large_mask, 32, 1, &mut icon.medium);
    rgba(large, large_mask, 32, 2, &mut icon.big);
    Some(icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the extracted Mac files; skipped without.
    #[test]
    fn reads_the_probe_icon() {
        let path = Path::new("../../out/mac/app/Iron Helix™/Iron Helix™.rsrc");
        if !path.exists() {
            return;
        }
        let icon = mac_icon(path).expect("the program has the probe icon");
        // The corners are outside the mask; the middle is drawn.
        assert_eq!(icon.medium[3], 0);
        assert_eq!(icon.medium[(16 * 32 + 16) * 4 + 3], 255);
        assert_eq!(icon.big[(32 * 64 + 32) * 4 + 3], 255);
    }
}
