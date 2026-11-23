//! A big keypad beside the game (not in the original): while a computer
//! asks for a code, its keypad is repeated in the space to the right of the
//! stage, large enough to tap easily on a phone. It's the same keypad: its
//! presses go through the game's and light its keys too.

use helix_core::ComputerButton;
use macroquad::prelude::*;

use crate::layout::Layout;

/// The keys, row by row, as the original keypad has them.
const ROWS: [[(&str, ComputerButton); 3]; 4] = [
    [("1", ComputerButton::Key(1)), ("2", ComputerButton::Key(2)), ("3", ComputerButton::Key(3))],
    [("4", ComputerButton::Key(4)), ("5", ComputerButton::Key(5)), ("6", ComputerButton::Key(6))],
    [("7", ComputerButton::Key(7)), ("8", ComputerButton::Key(8)), ("9", ComputerButton::Key(9))],
    [("CLEAR", ComputerButton::Clear), ("0", ComputerButton::Key(0)), ("CANCEL", ComputerButton::Cancel)],
];
/// Narrower spaces than this (in pixels) get no keypad.
const MIN_WIDTH: f32 = 110.0;
/// How long a tapped key stays lit, in seconds.
const LIT_TIME: f64 = 0.15;

const GREEN: Color = Color::new(0.0, 1.0, 0.25, 1.0);
const DIM: Color = Color::new(0.0, 0.55, 0.15, 1.0);
const PANEL: Color = Color::new(0.02, 0.09, 0.03, 0.92);

#[derive(Default)]
pub struct Keypad {
    /// The last key tapped and when, to light it.
    lit: Option<(usize, f64)>,
}

/// Each key's rectangle (on screen) and button; the Enter key spans the
/// bottom row. None when there's no room beside the stage.
fn keys(l: &Layout) -> Option<Vec<(Rect, &'static str, ComputerButton)>> {
    let left = l.stage.x + l.stage.w;
    let width = screen_width() - left;
    if width < MIN_WIDTH {
        return None;
    }
    let gap = (width * 0.04).max(4.0);
    // Five rows of keys under a display of the code (0.7 of a key tall).
    let size = ((width - gap * 4.0) / 3.0).min((screen_height() * 0.92 - gap * 6.0) / 5.7);
    let pad_w = size * 3.0 + gap * 2.0;
    let pad_h = size * 5.7 + gap * 5.0;
    let (x0, y0) = (left + (width - pad_w) / 2.0, (screen_height() - pad_h) / 2.0 + size * 0.7 + gap);
    let mut out = Vec::new();
    for (r, row) in ROWS.iter().enumerate() {
        for (c, &(label, button)) in row.iter().enumerate() {
            let rect = Rect::new(x0 + c as f32 * (size + gap), y0 + r as f32 * (size + gap), size, size);
            out.push((rect, label, button));
        }
    }
    out.push((Rect::new(x0, y0 + 4.0 * (size + gap), pad_w, size), "ENTER", ComputerButton::Enter));
    Some(out)
}

impl Keypad {
    /// The key a press at (x, y) on screen hits.
    pub fn press(&mut self, l: &Layout, x: f32, y: f32) -> Option<ComputerButton> {
        let keys = keys(l)?;
        let i = keys.iter().position(|(r, _, _)| r.contains(vec2(x, y)))?;
        self.lit = Some((i, get_time()));
        Some(keys[i].2)
    }

    /// Draw it, with the code entered so far above the keys.
    pub fn draw(&self, l: &Layout, code: &str) {
        let Some(keys) = keys(l) else { return };
        let lit = self.lit.filter(|(_, t)| get_time() - t < LIT_TIME).map(|(i, _)| i);
        let size = keys[0].0.h;
        let gap = keys[1].0.x - keys[0].0.x - size;
        // A panel behind the display and keys.
        let (first, last) = (keys[0].0, keys[keys.len() - 1].0);
        let m = size * 0.15;
        let top = first.y - gap - size * 0.7;
        draw_rectangle(first.x - m, top - m, last.x + last.w - first.x + m * 2.0, last.y + last.h - top + m * 2.0, PANEL);
        // The display.
        let d = Rect::new(first.x, top, last.w, size * 0.7);
        draw_rectangle(d.x, d.y, d.w, d.h, BLACK);
        draw_rectangle_lines(d.x, d.y, d.w, d.h, (size * 0.04).max(1.5), DIM);
        let font = size * 0.5;
        let t = measure_text(code, None, font as u16, 1.0);
        draw_text(code, d.x + (d.w - t.width) / 2.0, d.y + (d.h + t.height) / 2.0, font, GREEN);
        for (i, (r, label, _)) in keys.iter().enumerate() {
            let on = lit == Some(i);
            draw_rectangle(r.x, r.y, r.w, r.h, if on { GREEN } else { Color::new(0.0, 0.0, 0.0, 0.6) });
            draw_rectangle_lines(r.x, r.y, r.w, r.h, (size * 0.04).max(1.5), if on { GREEN } else { DIM });
            // Digits big; words as big as fits.
            let mut font = if label.len() > 1 { size * 0.4 } else { size * 0.55 };
            let mut t = measure_text(label, None, font as u16, 1.0);
            if t.width > r.w * 0.86 {
                font *= r.w * 0.86 / t.width;
                t = measure_text(label, None, font as u16, 1.0);
            }
            let color = if on { BLACK } else { GREEN };
            draw_text(label, r.x + (r.w - t.width) / 2.0, r.y + (r.h + t.height) / 2.0, font, color);
        }
    }
}
