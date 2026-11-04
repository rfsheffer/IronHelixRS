//! The volume slider (Lingo's VolumeSet): the knob (cast 256, sprite
//! channel 5) slides along the bottom bar, on the title screen and in the
//! game alike, between loc 347 and 439; letting go plays "SoundCheck".
//!
//! The original set the Mac's speaker volume (0-7) from where the knob
//! stopped, so its starting place (394) meant nothing until it moved. Here
//! the knob sets the game's own volume, silent at the left and full at the
//! right, and starts at the right (full volume, as with no slider). One
//! setting for the whole program.

use std::sync::atomic::{AtomicI32, Ordering};

use crate::sprites::{Canvas, Cast};

/// The knob's cast and its sprite's vertical loc (from the score).
const CAST_KNOB: u16 = 256;
const KNOB_V: i32 = 80;
/// The knob's horizontal loc range.
const MIN: i32 = 347;
const MAX: i32 = 439;

static KNOB: AtomicI32 = AtomicI32::new(MAX);

/// The volume, 0 (silent) to 1 (full).
pub fn volume() -> f32 {
    (KNOB.load(Ordering::Relaxed) - MIN) as f32 / (MAX - MIN) as f32
}

/// Dragging the knob.
#[derive(Default)]
pub struct Slider {
    /// While dragging: where on the knob it was grabbed.
    grab: Option<f32>,
}

impl Slider {
    /// A press on the knob starts a drag; true if it took the press.
    pub fn press(&mut self, cast: &Cast, x: f32, y: f32) -> bool {
        let knob = KNOB.load(Ordering::Relaxed);
        if cast.get(CAST_KNOB).is_some_and(|img| img.hit((knob, KNOB_V), x, y)) {
            self.grab = Some(x - knob as f32);
            return true;
        }
        false
    }

    /// Move the knob with the pointer; true while dragging (the volume
    /// changed, so playing sounds should follow).
    pub fn drag(&mut self, x: f32) -> bool {
        let Some(grab) = self.grab else { return false };
        KNOB.store(((x - grab).round() as i32).clamp(MIN, MAX), Ordering::Relaxed);
        true
    }

    /// Let go; true if a drag ended (time for the sound check).
    pub fn release(&mut self) -> bool {
        self.grab.take().is_some()
    }

    /// The knob where it is.
    pub fn draw(cast: &Cast, canvas: &mut Canvas) {
        if let Some(img) = cast.get(CAST_KNOB) {
            canvas.draw(img, (KNOB.load(Ordering::Relaxed), KNOB_V), true);
        }
    }
}
