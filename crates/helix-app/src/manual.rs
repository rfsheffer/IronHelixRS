//! The game's printed manual, from a scan: its pages one after another down
//! the screen, to scroll through (drag, the wheel, the arrow and page keys)
//! and zoom (pinch, Ctrl+wheel, + and -). Esc, Back or the close button
//! leaves it.
//!
//! The pages are JPEGs in the data folder (`pack data` takes them out of the
//! PDF). They're decoded on a background thread as they come near the
//! screen and dropped again when far away, so only a few are in memory.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use macroquad::prelude::*;

/// Space between pages and around them, in page widths.
const GAP: f32 = 0.03;
/// Pages kept decoded on either side of those on screen.
const KEEP: usize = 2;
/// How far a fling carries on, per second of speed lost.
const FRICTION: f32 = 4.0;
const ZOOM_RANGE: (f32, f32) = (0.4, 3.0);

/// A decoded page: its size and RGBA pixels.
type Decoded = (usize, u16, u16, Vec<u8>);

pub struct Manual {
    pages: Vec<PathBuf>,
    /// Each page's height over its width.
    aspects: Vec<f32>,
    textures: HashMap<usize, Texture2D>,
    requested: Vec<bool>,
    to_decoder: Sender<usize>,
    decoded: Receiver<Decoded>,
    /// Scroll position from the top, and sideways, in page widths.
    top: f32,
    left: f32,
    zoom: f32,
    velocity: f32,
    /// The last pointer position while dragging, and when.
    dragging: Option<(Vec2, f64)>,
    /// Two fingers' distance when a pinch began, and the zoom then.
    pinch: Option<(f32, f32)>,
}

impl Manual {
    /// The pages in the data folder's `manual` folder, if any.
    pub fn open(dir: &std::path::Path) -> Option<Self> {
        let mut names: Vec<String> = helixdb::vfs::list(dir).into_iter().filter(|n| n.to_ascii_lowercase().ends_with(".jpg")).collect();
        names.sort();
        if names.is_empty() {
            return None;
        }
        let pages: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
        // The pages' shapes, from their headers, to lay them out at once.
        let aspects = pages
            .iter()
            .map(|p| {
                let data = helixdb::vfs::read(p).ok()?;
                let mut d = jpeg_decoder::Decoder::new(data.as_slice());
                d.read_info().ok()?;
                let info = d.info()?;
                Some(info.height as f32 / info.width.max(1) as f32)
            })
            .map(|a| a.unwrap_or(1.5))
            .collect();

        let (to_decoder, requests) = channel::<usize>();
        let (send_page, decoded) = channel::<Decoded>();
        let paths = pages.clone();
        std::thread::spawn(move || {
            while let Ok(i) = requests.recv() {
                if let Some(page) = decode(&paths[i]) {
                    let (w, h, rgba) = page;
                    if send_page.send((i, w, h, rgba)).is_err() {
                        break;
                    }
                }
            }
        });
        let n = pages.len();
        Some(Self {
            pages,
            aspects,
            textures: HashMap::new(),
            requested: vec![false; n],
            to_decoder,
            decoded,
            top: 0.0,
            left: 0.0,
            zoom: 1.0,
            velocity: 0.0,
            dragging: None,
            pinch: None,
        })
    }

    /// A page's width on screen at this zoom: about two screens tall for a
    /// letter-sized page, and never wider than the screen at zoom 1.
    fn page_width(&self) -> f32 {
        (screen_width() * 0.92).min(screen_height() * 1.3) * self.zoom
    }

    /// Each page's top, in page widths, and the end of the last.
    fn tops(&self) -> (Vec<f32>, f32) {
        let mut y = GAP;
        let tops = self
            .aspects
            .iter()
            .map(|a| {
                let t = y;
                y += a + GAP;
                t
            })
            .collect();
        (tops, y)
    }

    /// Handle input and load pages; false once it's closed.
    pub fn update(&mut self, dt: f32) -> bool {
        if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Back) || is_key_pressed(KeyCode::Q) {
            return false;
        }
        let pw = self.page_width();
        let (tops, end) = self.tops();
        let view = screen_height() / pw;

        // The close button, top right.
        if is_mouse_button_pressed(MouseButton::Left) && self.close_button().contains(mouse_position().into()) {
            return false;
        }

        // Keys.
        let line = 0.08;
        if is_key_pressed(KeyCode::Down) {
            self.top += line;
        }
        if is_key_pressed(KeyCode::Up) {
            self.top -= line;
        }
        if is_key_pressed(KeyCode::PageDown) || is_key_pressed(KeyCode::Space) {
            self.top += view * 0.9;
        }
        if is_key_pressed(KeyCode::PageUp) {
            self.top -= view * 0.9;
        }
        if is_key_pressed(KeyCode::Home) {
            self.top = 0.0;
        }
        if is_key_pressed(KeyCode::End) {
            self.top = end;
        }
        if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
            self.zoom_by(1.25);
        }
        if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
            self.zoom_by(0.8);
        }

        // The wheel scrolls, or zooms with Ctrl.
        let (_, wheel) = mouse_wheel();
        if wheel != 0.0 {
            if is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl) {
                self.zoom_by(if wheel > 0.0 { 1.1 } else { 0.9 });
            } else {
                self.top -= wheel.signum() * line * 1.5;
            }
        }

        // Two fingers pinch to zoom; one (as the mouse) drags and flings.
        let fingers = touches();
        if fingers.len() >= 2 {
            let d = fingers[0].position.distance(fingers[1].position).max(1.0);
            match self.pinch {
                Some((start, zoom)) => self.zoom = (zoom * d / start).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1),
                None => self.pinch = Some((d, self.zoom)),
            }
            self.dragging = None;
        } else {
            self.pinch = None;
            let at = Vec2::from(mouse_position());
            if is_mouse_button_down(MouseButton::Left) {
                if let Some((last, when)) = self.dragging {
                    let moved = (at - last) / pw;
                    self.top -= moved.y;
                    self.left -= moved.x;
                    let elapsed = (get_time() - when).max(1e-3) as f32;
                    self.velocity = -moved.y / elapsed;
                }
                self.dragging = Some((at, get_time()));
                if is_mouse_button_pressed(MouseButton::Left) {
                    self.velocity = 0.0;
                }
            } else {
                self.dragging = None;
                self.top += self.velocity * dt;
                self.velocity *= (-FRICTION * dt).exp();
                if self.velocity.abs() < 0.01 {
                    self.velocity = 0.0;
                }
            }
        }

        // Keep within the pages.
        let pw = self.page_width();
        let view = screen_height() / pw;
        self.top = self.top.clamp(0.0, (end - view).max(0.0));
        let spare = ((pw - screen_width()) / pw / 2.0).max(0.0);
        self.left = self.left.clamp(-spare, spare);

        // Pages near the screen are decoded; far ones dropped.
        let visible = |i: usize| tops[i] + self.aspects[i] >= self.top && tops[i] <= self.top + view;
        let first = (0..self.pages.len()).find(|&i| visible(i)).unwrap_or(0);
        let last = (0..self.pages.len()).rev().find(|&i| visible(i)).unwrap_or(first);
        let keep = first.saturating_sub(KEEP)..=(last + KEEP).min(self.pages.len() - 1);
        for i in keep.clone() {
            if !self.requested[i] {
                self.requested[i] = true;
                let _ = self.to_decoder.send(i);
            }
        }
        while let Ok((i, w, h, rgba)) = self.decoded.try_recv() {
            if keep.contains(&i) {
                let tex = Texture2D::from_rgba8(w, h, &rgba);
                tex.set_filter(FilterMode::Linear);
                self.textures.insert(i, tex);
            } else {
                self.requested[i] = false;
            }
        }
        let far: Vec<usize> = self.textures.keys().copied().filter(|i| !keep.contains(i)).collect();
        for i in far {
            self.textures.remove(&i);
            self.requested[i] = false;
        }
        true
    }

    fn zoom_by(&mut self, factor: f32) {
        // Keep the middle of the screen where it is.
        let before = self.page_width();
        let middle = self.top + screen_height() / before / 2.0;
        self.zoom = (self.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        self.top = middle - screen_height() / self.page_width() / 2.0;
    }

    fn close_button(&self) -> Rect {
        let s = (screen_height() * 0.07).max(36.0);
        Rect::new(screen_width() - s * 1.4, s * 0.4, s, s)
    }

    pub fn draw(&self) {
        clear_background(Color::from_rgba(24, 24, 26, 255));
        let pw = self.page_width();
        let x = (screen_width() - pw) / 2.0 - self.left * pw;
        let (tops, _) = self.tops();
        let mut current = 0;
        for (i, top) in tops.iter().enumerate() {
            let y = (top - self.top) * pw;
            let h = self.aspects[i] * pw;
            if y + h < 0.0 || y > screen_height() {
                continue;
            }
            if y < screen_height() / 2.0 {
                current = i;
            }
            match self.textures.get(&i) {
                Some(tex) => draw_texture_ex(tex, x, y, WHITE, DrawTextureParams { dest_size: Some(vec2(pw, h)), ..Default::default() }),
                // Not decoded yet: a blank page.
                None => draw_rectangle(x, y, pw, h, Color::from_rgba(225, 225, 220, 255)),
            }
        }

        // The page number and the close button.
        let size = (screen_height() * 0.035).max(18.0);
        let label = format!("Page {} of {}", current + 1, self.pages.len());
        let m = measure_text(&label, None, size as u16, 1.0);
        let (lx, ly) = ((screen_width() - m.width) / 2.0, screen_height() - size * 0.8);
        draw_rectangle(lx - size * 0.5, ly - m.height - size * 0.3, m.width + size, m.height + size * 0.6, Color::new(0.0, 0.0, 0.0, 0.6));
        draw_text(&label, lx, ly, size, WHITE);
        let b = self.close_button();
        draw_rectangle(b.x, b.y, b.w, b.h, Color::new(0.0, 0.0, 0.0, 0.6));
        let (p, t) = (b.w * 0.28, (b.w * 0.08).max(2.0));
        draw_line(b.x + p, b.y + p, b.x + b.w - p, b.y + b.h - p, t, WHITE);
        draw_line(b.x + b.w - p, b.y + p, b.x + p, b.y + b.h - p, t, WHITE);
    }
}

/// A page as RGBA (the scans are grayscale; any JPEG works).
fn decode(path: &std::path::Path) -> Option<(u16, u16, Vec<u8>)> {
    let data = helixdb::vfs::read(path).ok()?;
    let mut d = jpeg_decoder::Decoder::new(data.as_slice());
    let pixels = d.decode().ok()?;
    let info = d.info()?;
    let rgba = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        jpeg_decoder::PixelFormat::RGB24 => pixels.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        // 16-bit gray: the high byte.
        jpeg_decoder::PixelFormat::L16 => pixels.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], 255]).collect(),
        jpeg_decoder::PixelFormat::CMYK32 => {
            pixels.as_chunks::<4>().0.iter().flat_map(|p| [255 - p[0].max(p[3]), 255 - p[1].max(p[3]), 255 - p[2].max(p[3]), 255]).collect()
        }
    };
    Some((info.width, info.height, rgba))
}
