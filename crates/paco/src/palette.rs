//! 256-color palettes.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette(pub [Rgb; 256]);

impl Palette {
    /// The classic Macintosh 8-bit system palette (also used by QuickTime).
    ///
    /// It is generated rather than tabled:
    /// - indices 0..215: a 6x6x6 color cube with levels FF, CC, 99, 66, 33, 00
    ///   (red slowest), minus its final entry (black)
    /// - then 10-step ramps of pure red, green, blue and gray using the
    ///   levels EE, DD, BB, AA, 88, 77, 55, 44, 22, 11
    /// - index 255 is black.
    ///
    /// Note that index 0 is *white* on the Mac, unlike Windows.
    pub fn mac_system() -> Self {
        const CUBE: [u8; 6] = [0xFF, 0xCC, 0x99, 0x66, 0x33, 0x00];
        const RAMP: [u8; 10] = [0xEE, 0xDD, 0xBB, 0xAA, 0x88, 0x77, 0x55, 0x44, 0x22, 0x11];

        let mut colors = Vec::with_capacity(256);
        for r in CUBE {
            for g in CUBE {
                for b in CUBE {
                    colors.push(Rgb::new(r, g, b));
                }
            }
        }
        colors.pop(); // drop the cube's black; it goes at the very end
        colors.extend(RAMP.map(|v| Rgb::new(v, 0, 0)));
        colors.extend(RAMP.map(|v| Rgb::new(0, v, 0)));
        colors.extend(RAMP.map(|v| Rgb::new(0, 0, v)));
        colors.extend(RAMP.map(|v| Rgb::new(v, v, v)));
        colors.push(Rgb::new(0, 0, 0));

        Self(colors.try_into().expect("Mac palette has exactly 256 entries"))
    }

    /// Every entry set to one color (used by SOLID palette opcodes).
    pub fn solid(color: Rgb) -> Self {
        Self([color; 256])
    }

    /// Build a palette from up to 256 colors; missing entries are black.
    pub fn from_colors(colors: &[Rgb]) -> Self {
        let mut out = [Rgb::default(); 256];
        for (dst, src) in out.iter_mut().zip(colors) {
            *dst = *src;
        }
        Self(out)
    }

    /// Flatten to `[r, g, b, r, g, b, ...]`, the layout PNG and GIF want.
    pub fn to_rgb_bytes(&self) -> Vec<u8> {
        self.0.iter().flat_map(|c| [c.r, c.g, c.b]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_palette_landmarks() {
        let p = Palette::mac_system();
        assert_eq!(p.0[0], Rgb::new(0xFF, 0xFF, 0xFF));
        assert_eq!(p.0[1], Rgb::new(0xFF, 0xFF, 0xCC));
        assert_eq!(p.0[214], Rgb::new(0x00, 0x00, 0x33));
        assert_eq!(p.0[215], Rgb::new(0xEE, 0x00, 0x00));
        assert_eq!(p.0[254], Rgb::new(0x11, 0x11, 0x11));
        assert_eq!(p.0[255], Rgb::new(0x00, 0x00, 0x00));
    }
}
