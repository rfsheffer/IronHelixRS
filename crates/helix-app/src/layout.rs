//! Fitting the 4:3 stage onto any screen.
//!
//! The stage is scaled to fill the screen's height (or width, on screens
//! narrower than 4:3) and centered. The space left over becomes side panels
//! (wide screens) or top/bottom bars (tall screens), drawn with decorative
//! artwork. Kept free of macroquad so it can be unit tested.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// Screen pixels per stage pixel.
    pub scale: f32,
    /// Where the stage is drawn.
    pub stage: Rect,
    /// Leftover areas; zero-sized when unused.
    pub left: Rect,
    pub right: Rect,
    pub top: Rect,
    pub bottom: Rect,
}

impl Layout {
    /// `pixel_perfect` rounds the scale down to a whole number (when the
    /// screen is at least as big as the stage) for perfectly crisp pixels.
    pub fn fit(screen_w: f32, screen_h: f32, stage_w: f32, stage_h: f32, pixel_perfect: bool) -> Self {
        let mut scale = (screen_w / stage_w).min(screen_h / stage_h);
        if pixel_perfect && scale >= 1.0 {
            scale = scale.floor();
        }
        let (w, h) = (stage_w * scale, stage_h * scale);
        let (x, y) = (((screen_w - w) / 2.0).floor(), ((screen_h - h) / 2.0).floor());
        Self {
            scale,
            stage: Rect { x, y, w, h },
            left: Rect { x: 0.0, y: 0.0, w: x, h: screen_h },
            right: Rect { x: x + w, y: 0.0, w: screen_w - (x + w), h: screen_h },
            top: Rect { x, y: 0.0, w, h: y },
            bottom: Rect { x, y: y + h, w, h: screen_h - (y + h) },
        }
    }

    /// Convert a screen position (mouse or touch) to stage coordinates.
    pub fn screen_to_stage(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        self.stage
            .contains(x, y)
            .then(|| ((x - self.stage.x) / self.scale, (y - self.stage.y) / self.scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widescreen_gets_side_panels() {
        let l = Layout::fit(1920.0, 1080.0, 512.0, 384.0, false);
        assert_eq!(l.stage, Rect { x: 240.0, y: 0.0, w: 1440.0, h: 1080.0 });
        assert_eq!(l.left.w, 240.0);
        assert_eq!(l.right.w, 240.0);
        assert_eq!(l.top.h, 0.0);
        assert_eq!(l.screen_to_stage(240.0 + 1440.0 / 2.0, 540.0), Some((256.0, 192.0)));
        assert_eq!(l.screen_to_stage(100.0, 100.0), None);
    }

    #[test]
    fn tall_screen_gets_bars_and_pixel_perfect_rounds() {
        let l = Layout::fit(1080.0, 1920.0, 512.0, 384.0, false);
        assert!(l.top.h > 0.0 && l.left.w == 0.0);
        let p = Layout::fit(1920.0, 1080.0, 512.0, 384.0, true);
        assert_eq!(p.scale, 2.0);
        assert_eq!(p.stage.w, 1024.0);
    }
}
