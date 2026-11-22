//! The window's icon: the Mac release's own, the probe (icon family -16455,
//! the custom icon in the game program's resource fork; the installed
//! folder's `Icon_` file carries the same one). It's read from the player's
//! files when the window opens, so nothing of the game ships with this
//! program; without them the window keeps the default icon.

use std::path::Path;

use macroquad::miniquad::conf::Icon;

/// The custom icon's resource ID (Finder's `kCustomIconResource`).
pub const CUSTOM_ICON: i16 = -16455;

/// The Mac's 8-bit system palette as RGB by index.
pub fn mac_palette() -> [[u8; 3]; 256] {
    let palette = paco::Palette::mac_system();
    std::array::from_fn(|i| [palette.0[i].r, palette.0[i].g, palette.0[i].b])
}

/// The probe icon from the game program's resource fork, at 16, 32 and 64
/// pixels (the 64 is the 32 doubled; the Mac has no larger one).
pub fn mac_icon(rsrc: &Path) -> Option<Icon> {
    let resources = macformats::rsrc::parse(&std::fs::read(rsrc).ok()?).ok()?;
    let (large, small) = macformats::icon::family(&resources, CUSTOM_ICON, &mac_palette())?;
    let mut icon = Icon { small: [0; 16 * 16 * 4], medium: [0; 32 * 32 * 4], big: [0; 64 * 64 * 4] };
    icon.small.copy_from_slice(&small.rgba);
    icon.medium.copy_from_slice(&large.rgba);
    icon.big.copy_from_slice(&large.scaled(2).rgba);
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
