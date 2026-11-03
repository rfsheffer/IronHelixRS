//! The gameplay screen: the original interface laid out from the Director
//! score, driven by the rules in helix-core.
//!
//! Sprite channel numbers and cast numbers below are the ones the original
//! Lingo uses (see docs/design.md §2); positions come from the score's
//! gameplay frame, so nothing about the layout is guessed.

use std::collections::VecDeque;

use director::score::Frame;
use helix_core::{Effect, NavAction, Navigator, Panel};
use helixdb::Direction;

use crate::assets::GameAssets;
use crate::sound::Sounds;
use crate::sprites::{Canvas, Cast};
use crate::video::{Clip, Player};

// Sprite channels.
const CH_VIEW: usize = 1;
const CH_ARM: usize = 2;
const CH_MAP: usize = 6;
const CH_PROBE_ICON: usize = 7;
const CH_DEFENDER_ICON: usize = 8;
const CH_SCREEN: usize = 9;
const CH_ENERGY: usize = 10;
const CH_SCAN_INDICATORS: std::ops::RangeInclusive<usize> = 11..=14;
const CH_MODE: usize = 16;
const CH_NAV_FIRST: usize = 17; // up, down, forward, back, left, right
const CH_PANEL: usize = 24;

// Cast members.
const CAST_NAV_BUTTONS: u16 = 9; // 3 per button: single, double, pressed
const CAST_PROBE_ICON: u16 = 48; // + facing
const CAST_DEFENDER_ICON: u16 = 52;
const CAST_PROBE_OVERVIEW: u16 = 53;
const CAST_DEFENDER_OVERVIEW: u16 = 54;
const CAST_LEVEL_MAPS: u16 = 55; // + level - 1; + 6 = whole ship
const CAST_MODE_BUTTON: u16 = 44; // + map mode
const CAST_NODE_SCREEN: u16 = 105;
const CAST_DOOR: u16 = 112; // closed; +2 open
const CAST_ELEVATOR: u16 = 116; // closed; +1 open; +1+level floor lamp

/// Defender's start position (design doc §3.1); it stands still until the
/// defender rules arrive.
const DEFENDER_LINE: usize = 5;

/// Movies are 240x128; their inner 233x123 fills the view window.
const VIEW_INNER: (usize, usize, usize, usize) = (4, 3, 233, 123);
/// Info strip clips play with their top-left here.
const INFO_STRIP: (i32, i32) = (342, 19);
/// The energy bar: x from 399 to 399 + energy, y 125-130 (full = 74).
const ENERGY_BAR: (i32, i32, i32) = (399, 125, 130);
const ENERGY_FULL: i32 = 74;

const NAV_ACTIONS: [NavAction; 6] =
    [NavAction::Up, NavAction::Down, NavAction::Forward, NavAction::Back, NavAction::TurnLeft, NavAction::TurnRight];

pub struct GameScreen {
    frame: Frame,
    cast: Cast,
    nav: Navigator,
    queue: VecDeque<Effect>,
    view: Option<Player>,
    view_still: Option<(Vec<u8>, usize)>,
    info: Option<Player>,
    info_still: Option<(Vec<u8>, usize)>,
    wait: f32,
    map_mode: u8,
    pressed: Option<usize>,
    sounds: Sounds,
    pub status: String,
}

fn loc(frame: &Frame, channel: usize) -> (i32, i32) {
    frame.sprite(channel).map_or((0, 0), |s| (s.loc_h as i32, s.loc_v as i32))
}

impl GameScreen {
    pub fn new(assets: &GameAssets) -> Result<Self, String> {
        let score = assets.movie.score().map_err(|e| e.to_string())?;
        // The "Game" label's frame initializes; play loops on the next one.
        let game = assets.movie.label_frame("Game").map_err(|e| e.to_string())?.ok_or("no \"Game\" frame in the score")?;
        let frame = score.get(game as usize).cloned().ok_or("score is shorter than expected")?;
        let cast = Cast::load(&assets.movie, &assets.palette)?;
        let nav = Navigator::new(assets.data.nav.clone());
        let mut screen = Self {
            frame,
            cast,
            queue: VecDeque::new(),
            view: None,
            view_still: None,
            info: None,
            info_still: None,
            wait: 0.0,
            map_mode: 1,
            pressed: None,
            sounds: Sounds::new(assets.mac_sounds.clone()),
            status: String::new(),
            nav,
        };
        screen.queue.extend(screen.nav.redraw());
        Ok(screen)
    }

    pub fn busy(&self) -> bool {
        self.view.is_some() || self.info.is_some() || self.wait > 0.0 || !self.queue.is_empty()
    }

    pub fn navigator(&self) -> &Navigator {
        &self.nav
    }

    fn act(&mut self, action: NavAction, pressed: Option<usize>) {
        if self.busy() {
            return;
        }
        let fx = self.nav.act(action);
        if !fx.is_empty() {
            self.pressed = pressed;
        }
        self.queue.extend(fx);
    }

    /// A click or tap at stage coordinates.
    pub fn tap(&mut self, x: f32, y: f32) {
        if self.busy() {
            return;
        }
        // Navigation buttons.
        let buttons = self.button_casts();
        for (i, cast) in buttons.iter().enumerate() {
            let ch = CH_NAV_FIRST + i;
            if let Some(img) = cast.and_then(|c| self.cast.get(c))
                && img.hit(loc(&self.frame, ch), x, y)
            {
                self.act(NAV_ACTIONS[i], Some(ch));
                return;
            }
        }
        // Map mode button: three positions left to right.
        if let Some(img) = self.cast.get(CAST_MODE_BUTTON + self.map_mode as u16)
            && img.hit(loc(&self.frame, CH_MODE), x, y)
        {
            self.map_mode = if x < 394.0 { 1 } else if x < 435.0 { 2 } else { 3 };
            return;
        }
        // Door and elevator panels on the control screen.
        match self.nav.panel() {
            Panel::Door { .. } | Panel::Elevator { .. } if (352.0..436.0).contains(&x) && (62.0..75.0).contains(&y) => {
                self.act(NavAction::Door, None);
            }
            Panel::Elevator { .. } if (352.0..480.0).contains(&x) && (76.0..91.0).contains(&y) => {
                let offset = x as i32 - 352;
                if offset % 22 < 18 {
                    self.act(NavAction::Elevator((offset / 22 + 1) as u8), None);
                }
            }
            _ => {}
        }
    }

    /// Keyboard controls (design doc §4.8).
    pub fn key(&mut self, key: macroquad::input::KeyCode) {
        use macroquad::input::KeyCode::*;
        if key == M {
            // Not in the original: cycles the map mode button.
            self.map_mode = self.map_mode % 3 + 1;
            return;
        }
        if key == P {
            // Testing aid until phases are played: cycles the game phase.
            self.nav.set_phase(self.nav.phase() % 4 + 1);
            return;
        }
        let action = match key {
            Up => NavAction::Forward,
            Down => NavAction::Back,
            Left => NavAction::TurnLeft,
            Right => NavAction::TurnRight,
            U => NavAction::Up,
            D => NavAction::Down,
            Tab => NavAction::ToggleRepeat,
            Enter | KpEnter => NavAction::Door,
            Key1 => NavAction::Elevator(1),
            Key2 => NavAction::Elevator(2),
            Key3 => NavAction::Elevator(3),
            Key4 => NavAction::Elevator(4),
            Key5 => NavAction::Elevator(5),
            Key6 => NavAction::Elevator(6),
            Key7 => NavAction::Elevator(7),
            _ => return,
        };
        let pressed = NAV_ACTIONS.iter().position(|a| *a == action).map(|i| CH_NAV_FIRST + i);
        self.act(action, pressed);
    }

    /// Run queued effects: each movie, clip and wait finishes before the next.
    pub async fn update(&mut self, assets: &GameAssets, dt: f32) {
        loop {
            if self.wait > 0.0 {
                self.wait -= dt;
                return;
            }
            if let Some(p) = &mut self.view {
                if !p.update(dt) {
                    return;
                }
                self.view_still = p.current().map(|f| (f.to_vec(), p.clip.width));
                self.view = None;
                continue;
            }
            if let Some(p) = &mut self.info {
                if !p.update(dt) {
                    return;
                }
                self.info_still = p.current().map(|f| (f.to_vec(), p.clip.width));
                self.info = None;
                continue;
            }
            let Some(effect) = self.queue.pop_front() else {
                self.pressed = None;
                return;
            };
            match effect {
                Effect::Video { folder, name, sound } => {
                    if let Some(s) = sound {
                        self.sounds.play(&s).await;
                    }
                    match Clip::load(&assets.file(&folder, &name)) {
                        Ok(clip) => {
                            self.status = format!("{folder}/{name}");
                            self.view = Some(Player::new(clip));
                        }
                        Err(e) => self.status = e,
                    }
                }
                Effect::InfoClip { folder, name } => {
                    if let Ok(clip) = crate::patches::info_clip(assets, &folder, &name) {
                        self.info = Some(Player::new(clip));
                    }
                }
                Effect::Sound(s) => self.sounds.play(&s).await,
                Effect::Wait(seconds) => self.wait = seconds,
            }
        }
    }

    /// Cast to show on each navigation button (None = hidden).
    fn button_casts(&self) -> [Option<u16>; 6] {
        let b = self.nav.buttons();
        let shown = [b.up, b.down, b.forward, b.back, b.left, b.right];
        let double = self.nav.repeat() as u16;
        std::array::from_fn(|i| {
            let base = CAST_NAV_BUTTONS + i as u16 * 3;
            if self.pressed == Some(CH_NAV_FIRST + i) {
                Some(base + 2)
            } else {
                shown[i].then_some(base + double)
            }
        })
    }

    /// Map position of a node line: the coordinate pair at `item` (2 =
    /// level map, 4 = whole ship, 6 = in a ladder), as stored in the Map table.
    fn map_point(&self, line: usize, item: usize) -> Option<(i32, i32)> {
        let n = self.nav.graph().at_line(line)?;
        let i = item.checked_sub(2)?;
        Some((n.map.get(i).copied().flatten()?, n.map.get(i + 1).copied().flatten()?))
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.pixels.fill(0);
        let managed = |ch: usize| {
            matches!(ch, CH_VIEW | CH_ARM | CH_MAP | CH_PROBE_ICON | CH_DEFENDER_ICON | CH_SCREEN | CH_ENERGY | CH_MODE | CH_PANEL)
                || CH_SCAN_INDICATORS.contains(&ch)
                || (CH_NAV_FIRST..CH_NAV_FIRST + 6).contains(&ch)
        };

        // The static parts of the interface, straight from the score.
        for (i, s) in self.frame.sprites.iter().enumerate() {
            let ch = i + 1;
            if s.kind == 1
                && !managed(ch)
                && let Some(img) = self.cast.get(s.cast)
                && img.width > 1
            {
                canvas.draw(img, (s.loc_h as i32, s.loc_v as i32), s.ink != 0);
            }
        }

        // The view window.
        if let (Some(img), Some((pixels, width))) =
            (self.cast.get(self.frame.sprite(CH_VIEW).map_or(0, |s| s.cast)), self.view_frame())
        {
            let (left, top) = img.top_left(loc(&self.frame, CH_VIEW));
            let (ix, iy, _, _) = VIEW_INNER;
            canvas.blit(pixels, width, VIEW_INNER, left + ix as i32, top + iy as i32, false);
        }

        // Control screen: node screen, or the door/elevator panel.
        let screen_loc = loc(&self.frame, CH_SCREEN);
        let (screen_cast, lamp) = match self.nav.panel() {
            Panel::Node => (CAST_NODE_SCREEN, None),
            Panel::Door { open } => (CAST_DOOR + if open { 2 } else { 0 }, None),
            Panel::Elevator { open, level } => (CAST_ELEVATOR + open as u16, Some(CAST_ELEVATOR + 1 + level as u16)),
        };
        if let Some(img) = self.cast.get(screen_cast) {
            canvas.draw(img, screen_loc, false);
        }
        if let Some(img) = lamp.and_then(|c| self.cast.get(c)) {
            canvas.draw(img, loc(&self.frame, CH_PANEL), true);
        }
        if let Some((pixels, width)) = self.info_frame() {
            let h = pixels.len() / 4 / width.max(1);
            canvas.blit(pixels, width, (0, 0, width, h), INFO_STRIP.0, INFO_STRIP.1, false);
        }
        // The power bar sits on top of whichever panel is showing.
        let (x, y0, y1) = ENERGY_BAR;
        canvas.fill(x, y0, x + ENERGY_FULL, y1, [0x00, 0xDD, 0x00]);

        // Map, probe and defender.
        let probe_level = self.nav.level();
        let defender_level = self.nav.graph().at_line(DEFENDER_LINE).map_or(0, |n| n.level());
        let probe_item = if self.nav.in_ladder() { 6 } else { 2 };
        let (map_cast, probe, defender) = match self.map_mode {
            1 => (
                CAST_LEVEL_MAPS + probe_level as u16 - 1,
                self.map_point(self.nav.line(), probe_item).map(|p| (CAST_PROBE_ICON + self.nav.facing().index() as u16, p)),
                (defender_level == probe_level)
                    .then(|| self.map_point(DEFENDER_LINE, 2).map(|p| (CAST_DEFENDER_ICON, p)))
                    .flatten(),
            ),
            2 => (
                CAST_LEVEL_MAPS + defender_level as u16 - 1,
                (defender_level == probe_level)
                    .then(|| self.map_point(self.nav.line(), probe_item).map(|p| (CAST_PROBE_ICON + self.nav.facing().index() as u16, p)))
                    .flatten(),
                self.map_point(DEFENDER_LINE, 2).map(|p| (CAST_DEFENDER_ICON, p)),
            ),
            _ => (
                CAST_LEVEL_MAPS + 6,
                self.map_point(self.nav.line(), 4).map(|p| (CAST_PROBE_OVERVIEW, p)),
                self.map_point(DEFENDER_LINE, 4).map(|p| (CAST_DEFENDER_OVERVIEW, p)),
            ),
        };
        if let Some(img) = self.cast.get(map_cast) {
            canvas.draw(img, loc(&self.frame, CH_MAP), false);
        }
        for (cast, point) in [probe, defender].into_iter().flatten() {
            if let Some(img) = self.cast.get(cast) {
                canvas.draw(img, point, true);
            }
        }
        if let Some(img) = self.cast.get(CAST_MODE_BUTTON + self.map_mode as u16) {
            canvas.draw(img, loc(&self.frame, CH_MODE), false);
        }

        // Navigation buttons.
        for (i, cast) in self.button_casts().iter().enumerate() {
            if let Some(img) = cast.and_then(|c| self.cast.get(c)) {
                canvas.draw(img, loc(&self.frame, CH_NAV_FIRST + i), true);
            }
        }
    }

    fn view_frame(&self) -> Option<(&[u8], usize)> {
        match &self.view {
            Some(p) => p.current().map(|f| (f, p.clip.width)),
            None => self.view_still.as_ref().map(|(f, w)| (f.as_slice(), *w)),
        }
    }

    fn info_frame(&self) -> Option<(&[u8], usize)> {
        match &self.info {
            Some(p) => p.current().map(|f| (f, p.clip.width)),
            None => self.info_still.as_ref().map(|(f, w)| (f.as_slice(), *w)),
        }
    }

    pub fn facing(&self) -> Direction {
        self.nav.facing()
    }
}
