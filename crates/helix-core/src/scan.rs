//! The scanner's view-window part (design doc §6, Lingo `Scan`).
//!
//! Auto scan puts a cycling marker on the facing's scan point; clicking near
//! it enhances. Manual scan shows a box that glides toward the mouse; over a
//! scan point it locks on and, after four blip cycles, enhances. Everything
//! advances in ticks of [`TICK`] seconds, like the original's idle loop.

use helixdb::nav::ScanPoint;

use crate::rng::Rng;

/// Seconds per tick.
pub const TICK: f32 = 1.0 / 30.0;

/// Where the manual box starts (the view window's registration point).
const BOX_START: (i32, i32) = (141, 80);
/// The area the box can move in: left, top, right, bottom.
const TARGET_AREA: (i32, i32, i32, i32) = (51, 37, 232, 123);
/// Fastest box movement per step, in pixels, and steps per tick. The
/// original's glide loop ran about as fast as the Mac could redraw; at two
/// steps a tick (60 a second) the box keeps up with the cursor as it did, so
/// the random "seeking" blips, which only sound while it trails behind, stay
/// rare and the lock-on blips stand out.
const MAX_SPEED: f32 = 20.0;
const STEPS_PER_TICK: usize = 2;
/// Lock-on distance from the scan point: horizontal, vertical.
const LOCK: (i32, i32) = (20, 14);
/// Auto scan: how close a click must be to the point.
const AUTO_HIT: i32 = 20;
/// Marker frames per blip cycle, and the cycles before a lock enhances.
const MARKER_FRAMES: u8 = 6;
const LOCK_CYCLES: u8 = 4;
/// Seconds between the moving box's random flashes.
const FLASH_GAP: f32 = 40.0 / 60.0;

/// How the manual scan box looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxLook {
    /// Corners only, while seeking.
    Plain,
    /// Bright corners, a moment while moving.
    Flash,
    /// Corners with a crosshair, at rest.
    Idle,
    /// A grid, locked on a scan point.
    Locked,
}

/// The scanner's status text on the control screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanText {
    Active,
    Contact,
    Ready,
    Seeking,
}

/// What to draw for the scanner in the view window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanView {
    pub scan_box: Option<(BoxLook, (i32, i32))>,
    /// Target marker frame (0-5 cycling, 6 = hit) and position.
    pub marker: Option<(u8, (i32, i32))>,
    pub text: Option<ScanText>,
}

/// Something the scanner wants to happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanEvent {
    Blip,
    Enhance(ScanPoint),
}

#[derive(Debug, Clone)]
pub(crate) enum Scan {
    /// Auto scan; `None` when the facing had nothing (the scanner stays on).
    Auto { point: Option<ScanPoint>, frame: u8 },
    Manual(Manual),
    /// After enhancing: the control screen offers Acquire / Exit.
    Enhanced(ScanPoint),
}

#[derive(Debug, Clone)]
pub(crate) struct Manual {
    point: Option<ScanPoint>,
    pos: (f32, f32),
    target: (i32, i32),
    step: (f32, f32),
    /// The mouse is down.
    held: bool,
    /// The glide loop is running (OperateScan's repeat).
    running: bool,
    look: BoxLook,
    text: ScanText,
    /// Blip cycles completed while locked (0 = not locked).
    lock: u8,
    marker: u8,
    marker_pos: (i32, i32),
    since_flash: f32,
}

impl Scan {
    pub(crate) fn auto(point: Option<ScanPoint>) -> Self {
        Scan::Auto { point, frame: 0 }
    }

    pub(crate) fn manual(point: Option<ScanPoint>) -> Self {
        Scan::Manual(Manual {
            point,
            pos: (BOX_START.0 as f32, BOX_START.1 as f32),
            target: BOX_START,
            step: (0.0, 0.0),
            held: false,
            running: false,
            look: BoxLook::Idle,
            text: ScanText::Ready,
            lock: 0,
            marker: 0,
            marker_pos: (0, 0),
            since_flash: 0.0,
        })
    }

    pub(crate) fn view(&self) -> ScanView {
        match self {
            Scan::Auto { point: Some(p), frame } => {
                ScanView { marker: Some((*frame, (p.h as i32, p.v as i32))), ..Default::default() }
            }
            Scan::Auto { point: None, .. } | Scan::Enhanced(_) => ScanView::default(),
            Scan::Manual(m) => ScanView {
                scan_box: Some((m.look, (m.pos.0.round() as i32, m.pos.1.round() as i32))),
                marker: (m.lock > 0).then_some((m.marker, m.marker_pos)),
                text: Some(m.text),
            },
        }
    }

    /// The manual box is still gliding or finishing a lock.
    pub(crate) fn busy(&self) -> bool {
        matches!(self, Scan::Manual(m) if m.running || m.lock > 0)
    }

    /// The mouse went down in the view window.
    pub(crate) fn press(&mut self, x: i32, y: i32) -> Option<ScanEvent> {
        match self {
            Scan::Auto { point: Some(p), frame } if (x - p.h as i32).abs() < AUTO_HIT && (y - p.v as i32).abs() < AUTO_HIT => {
                *frame = MARKER_FRAMES;
                Some(ScanEvent::Enhance(*p))
            }
            Scan::Manual(m) => {
                m.held = true;
                m.running = true;
                m.text = ScanText::Seeking;
                m.look = BoxLook::Plain;
                m.aim(x, y);
                None
            }
            _ => None,
        }
    }

    pub(crate) fn drag(&mut self, x: i32, y: i32) {
        if let Scan::Manual(m) = self
            && m.held
        {
            m.aim(x, y);
        }
    }

    pub(crate) fn release(&mut self) {
        if let Scan::Manual(m) = self {
            m.held = false;
        }
    }

    pub(crate) fn tick(&mut self, rng: &mut Rng) -> Vec<ScanEvent> {
        match self {
            Scan::Auto { point: Some(_), frame } if *frame < MARKER_FRAMES => {
                *frame = (*frame + 1) % MARKER_FRAMES;
                Vec::new()
            }
            Scan::Manual(m) => m.tick(rng),
            _ => Vec::new(),
        }
    }
}

impl Manual {
    /// Head for a point (clamped to the target area) at up to MAX_SPEED per
    /// tick, both axes arriving together (Lingo's GetLineSlopes).
    fn aim(&mut self, x: i32, y: i32) {
        let (l, t, r, b) = TARGET_AREA;
        self.target = (x.clamp(l, r), y.clamp(t, b));
        let (dx, dy) = (self.target.0 as f32 - self.pos.0, self.target.1 as f32 - self.pos.1);
        let longest = dx.abs().max(dy.abs());
        self.step = if longest == 0.0 { (0.0, 0.0) } else { (dx / longest * MAX_SPEED, dy / longest * MAX_SPEED) };
    }

    fn arrived(&self) -> bool {
        self.pos == (self.target.0 as f32, self.target.1 as f32)
    }

    fn tick(&mut self, rng: &mut Rng) -> Vec<ScanEvent> {
        let mut events = Vec::new();
        if !self.running {
            // Released over a point: finish the blip cycles, then enhance.
            if self.lock > 0
                && let Some(p) = self.point
            {
                events.extend(self.over_point(p));
                if self.lock >= LOCK_CYCLES {
                    events.push(ScanEvent::Enhance(p));
                    self.lock = 0;
                }
            }
            return events;
        }

        for _ in 0..STEPS_PER_TICK {
            for (pos, target, step) in [(&mut self.pos.0, self.target.0, self.step.0), (&mut self.pos.1, self.target.1, self.step.1)] {
                let target = target as f32;
                *pos = if (*pos - target).abs() > step.abs() { *pos + step } else { target };
            }
        }
        self.since_flash += TICK;
        if self.lock == 0 && self.since_flash > FLASH_GAP {
            let moving = !self.arrived();
            self.look = BoxLook::Plain;
            if rng.random(10) <= 7 && moving {
                self.look = BoxLook::Flash;
                self.since_flash = 0.0;
                events.push(ScanEvent::Blip);
            }
        } else if self.look == BoxLook::Flash {
            self.look = BoxLook::Plain;
        }

        if let Some(p) = self.point {
            let (h, v) = (self.pos.0.round() as i32, self.pos.1.round() as i32);
            if (h - p.h as i32 - 2).abs() < LOCK.0 && (v - p.v as i32).abs() < LOCK.1 {
                events.extend(self.over_point(p));
            } else if self.lock > 0 {
                self.lock = 0;
                self.text = ScanText::Seeking;
                self.look = BoxLook::Plain;
            }
        }

        if !self.held && self.arrived() {
            self.running = false;
            if self.lock == 0 {
                self.look = BoxLook::Idle;
            }
        }
        events
    }

    /// Lingo's OverScanPoint: lock on, cycle the marker, blip each cycle.
    fn over_point(&mut self, p: ScanPoint) -> Option<ScanEvent> {
        if self.lock == 0 {
            self.lock = 1;
            self.marker = 0;
            self.look = BoxLook::Locked;
            self.text = ScanText::Contact;
        }
        let mut event = None;
        if self.marker + 1 < MARKER_FRAMES {
            self.marker += 1;
        } else {
            self.marker = 0;
            self.lock += 1;
            event = Some(ScanEvent::Blip);
        }
        // The marker snaps to a 5-pixel grid around the point.
        let (h, v) = (self.pos.0.round() as i32, self.pos.1.round() as i32);
        self.marker_pos = (p.h as i32 + (h - p.h as i32) % 5 + 2, p.v as i32 + (v - p.v as i32) % 5);
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POINT: ScanPoint = ScanPoint { h: 120, v: 60, item: 7, dna: 2 };

    #[test]
    fn auto_hits_near_the_point() {
        let mut s = Scan::auto(Some(POINT));
        assert_eq!(s.press(10, 10), None);
        assert_eq!(s.press(130, 70), Some(ScanEvent::Enhance(POINT)));
        assert_eq!(s.view().marker, Some((MARKER_FRAMES, (120, 60))));
    }

    #[test]
    fn manual_locks_on_and_enhances() {
        let mut rng = Rng::new(1);
        let mut s = Scan::manual(Some(POINT));
        assert_eq!(s.view().text, Some(ScanText::Ready));
        s.press(POINT.h as i32 + 2, POINT.v as i32);
        s.release();
        let mut events = Vec::new();
        for _ in 0..200 {
            events.extend(s.tick(&mut rng));
            if events.iter().any(|e| matches!(e, ScanEvent::Enhance(_))) {
                break;
            }
        }
        assert!(events.contains(&ScanEvent::Enhance(POINT)));
        assert!(events.iter().filter(|e| **e == ScanEvent::Blip).count() >= LOCK_CYCLES as usize - 1);
    }

    /// Drag the box from `from` to `to`, `step` pixels a tick; the blips heard.
    fn blips(from: (i32, i32), to: (i32, i32), step: i32) -> usize {
        let mut rng = Rng::new(3);
        let mut s = Scan::manual(Some(POINT));
        s.press(from.0, from.1);
        for _ in 0..30 {
            s.tick(&mut rng);
        }
        let n = ((to.0 - from.0).abs().max((to.1 - from.1).abs()) / step).max(1);
        let mut count = 0;
        for i in 0..=n {
            s.drag(from.0 + (to.0 - from.0) * i / n, from.1 + (to.1 - from.1) * i / n);
            count += s.tick(&mut rng).iter().filter(|e| **e == ScanEvent::Blip).count();
        }
        count
    }

    #[test]
    fn blips_only_over_the_point() {
        // Quick drags past the point's column and row keep the box with the
        // cursor: no seeking blips.
        assert_eq!(blips((162, 40), (162, 120), 30), 0);
        assert_eq!(blips((60, 110), (230, 110), 30), 0);
        // Slowly across the point: the lock-on blips.
        assert!(blips((60, 60), (230, 60), 2) >= 2);
    }

    #[test]
    fn manual_box_stays_in_the_target_area() {
        let mut rng = Rng::new(1);
        let mut s = Scan::manual(None);
        s.press(500, 0);
        s.release();
        for _ in 0..100 {
            s.tick(&mut rng);
        }
        let v = s.view();
        assert_eq!(v.scan_box, Some((BoxLook::Idle, (232, 37))));
        assert_eq!(v.text, Some(ScanText::Seeking));
    }
}
