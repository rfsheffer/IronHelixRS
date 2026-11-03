//! The gameplay screen: the original interface laid out from the Director
//! score, driven by the rules in helix-core.
//!
//! Sprite channel numbers and cast numbers below are the ones the original
//! Lingo uses (see docs/design.md §2); positions come from the score's
//! gameplay frame, so nothing about the layout is guessed.

use std::collections::{HashMap, VecDeque};

use director::Picture;
use director::score::Frame;
use helix_core::game::ENERGY_FULL;
use helix_core::scan::TICK;
use helix_core::{
    Access, Action, BoxLook, Button, ComputerButton, ComputerData, ControlsPage, Effect, Function, Game, Interface, NavAction,
    Navigator, Panel, ScanText,
};
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
const CH_FUNCTION: usize = 15;
const CH_MODE: usize = 16;
const CH_NAV_FIRST: usize = 17; // up, down, forward, back, left, right
const CH_PANEL: usize = 24;

// Cast members.
const CAST_NAV_BUTTONS: u16 = 9; // 3 per button: single, double, pressed
const CAST_FUNCTION: u16 = 33; // none lit; +2 scan, +5 probe controls
const CAST_MODE_BUTTON: u16 = 44; // + map mode
const CAST_PROBE_ICON: u16 = 48; // + facing
const CAST_DEFENDER_ICON: u16 = 52;
const CAST_PROBE_OVERVIEW: u16 = 53;
const CAST_DEFENDER_OVERVIEW: u16 = 54;
const CAST_LEVEL_MAPS: u16 = 55; // + level - 1; + 6 = whole ship
const CAST_SCAN_BOX: u16 = 89; // plain, flash, idle, locked
const CAST_SCAN_MARKER: u16 = 93; // + frame 0-5; 99 = hit
const CAST_SCAN_HIT: u16 = 99;
const CAST_NODE_SCREEN: u16 = 105;
const CAST_SCAN_BARS: u16 = 107; // + frame 0-4
const CAST_DOOR: u16 = 112; // closed; +2 open
const CAST_ELEVATOR: u16 = 116; // closed; +1 open; +1+level floor lamp
const CAST_CONTROLS_MENU: u16 = 130;
const CAST_AUTO_SCAN_LAMP: u16 = 133; // off; +2 on
const CAST_CONTROLS_INVENTORY: u16 = 143;
const CAST_CREW: u16 = 146; // + (DNA id - 1) * 2, + 1 selected
const CAST_EMPTY_SCREEN: u16 = 168;
const CAST_SCAN_INTERFACE: u16 = 169;
const CAST_COMPUTER_HILITE: u16 = 177; // square row; +1 oval row; +item-3 bottom bar
const CAST_ETA_SHIP: u16 = 214;
const CAST_MESSAGE: u16 = 219;
const CAST_SCAN_TEXT: u16 = 220; // active, contact, ready, seeking
const CAST_SEGUE_SCREEN: u16 = 225;
const CAST_KEYPAD: u16 = 233; // + key: 1-9, 10 = 0, 11 clear, 12 enter, 13 cancel

/// Defender's start position (design doc §3.1); it stands still until the
/// defender rules arrive.
const DEFENDER_LINE: usize = 5;

/// Movies are 240x128; their inner 233x123 fills the view window.
const VIEW_INNER: (usize, usize, usize, usize) = (4, 3, 233, 123);
/// Info strip clips play with their top-left here.
const INFO_STRIP: (i32, i32) = (342, 19);
/// The energy bar: x from 399 to 399 + energy, y 125-130.
const ENERGY_BAR: (i32, i32, i32) = (399, 125, 130);
/// Scan indicators sit at this height (x depends on direction).
const SCAN_BAR_V: i32 = 120;
/// DNA samples on the inventory page.
const INVENTORY: [(i32, i32); 3] = [(436, 61), (436, 77), (436, 93)];
/// Function buttons' vertical bands: scan, probe controls.
const FUNCTION_SCAN: std::ops::Range<f32> = 22.0..39.0;
const FUNCTION_ARM: std::ops::Range<f32> = 73.0..89.0;
const FUNCTION_CONTROLS: std::ops::Range<f32> = 124.0..138.0;
/// Computer clips and video messages play over the control screen here.
const PANEL_AT: (i32, i32) = (335, 15);
/// Segues play over the interface here.
const SEGUE_AT: (i32, i32) = (38, 40);
/// The keypad's digits.
const DIGITS_AT: (i32, i32) = (353, 55);
/// Screen text color (the computer screens' green).
const TEXT_COLOR: [u8; 3] = [0x00, 0xFF, 0x00];
/// How long a pressed control-screen button stays lit.
const HILITE_TIME: f32 = 0.15;

/// A lit button: its cast and where.
type Hilite = (u16, (i32, i32));

const NAV_ACTIONS: [NavAction; 6] =
    [NavAction::Up, NavAction::Down, NavAction::Forward, NavAction::Back, NavAction::TurnLeft, NavAction::TurnRight];

/// The bridge screen (score frame `BridgeBG`) where phase movies play.
struct Bridge {
    picture: Option<(Picture, (i32, i32))>,
    movie_at: (i32, i32),
}

pub struct GameScreen {
    frame: Frame,
    cast: Cast,
    game: Game,
    bridge: Bridge,
    queue: VecDeque<Effect>,
    view: Option<Player>,
    view_is_nav: bool,
    view_still: Option<(Vec<u8>, usize)>,
    /// The last navigation picture, which the view returns to after scanning.
    nav_still: Option<(Vec<u8>, usize)>,
    info: Option<Player>,
    info_still: Option<(Vec<u8>, usize)>,
    bridge_movie: Option<Player>,
    /// Computer clips and video messages over the control screen; `keep`
    /// leaves the last frame up.
    panel: Option<(Player, bool)>,
    panel_still: Option<(Vec<u8>, usize)>,
    segue: Option<Player>,
    /// Computer screens by name (the first member with that name).
    screens: HashMap<String, u16>,
    wait: f32,
    tick_time: f32,
    map_mode: u8,
    pressed: Option<usize>,
    hilite: Option<(Hilite, f32)>,
    dragging: bool,
    sounds: Sounds,
    pub status: String,
}

fn loc(frame: &Frame, channel: usize) -> (i32, i32) {
    frame.sprite(channel).map_or((0, 0), |s| (s.loc_h as i32, s.loc_v as i32))
}

impl GameScreen {
    pub fn new(assets: &GameAssets) -> Result<Self, String> {
        let score = assets.movie.score().map_err(|e| e.to_string())?;
        let frame_of = |label: &str| -> Result<Frame, String> {
            let n = assets.movie.label_frame(label).map_err(|e| e.to_string())?.ok_or(format!("no \"{label}\" frame in the score"))?;
            score.get(n as usize - 1).cloned().ok_or_else(|| "score is shorter than expected".to_string())
        };
        // The "Game" label's frame initializes; play loops on the next one.
        let game_label = assets.movie.label_frame("Game").map_err(|e| e.to_string())?.ok_or("no \"Game\" frame in the score")?;
        let frame = score.get(game_label as usize).cloned().ok_or("score is shorter than expected")?;
        let bridge_frame = frame_of("BridgeBG")?;
        let picture = bridge_frame.sprites.iter().find_map(|s| {
            let p = assets.movie.picture(s.cast).ok().flatten()?;
            Some((p, (s.loc_h as i32, s.loc_v as i32)))
        });
        let bridge = Bridge { picture, movie_at: loc(&bridge_frame, CH_PANEL) };

        let cast = Cast::load(&assets.movie, &assets.palette)?;
        let mut nav = Navigator::new(assets.data.nav.clone());
        *nav.access_mut() = Access::new(assets.data.clearance.clone());
        let data = ComputerData {
            cmp: assets.data.cmp_lines.clone(),
            no_arm: assets.data.no_arm.clone(),
            clue_lists: assets.data.video_clue_lists.clone(),
            clue_notes: assets.data.clue_notes.clone(),
        };
        let game = Game::new(nav, assets.skill, assets.seed).with_computers(data);
        let mut screens = HashMap::new();
        for (cast, name) in assets.movie.cast_names().map_err(|e| e.to_string())? {
            screens.entry(name).or_insert(cast);
        }
        let mut screen = Self {
            frame,
            cast,
            bridge,
            queue: VecDeque::new(),
            view: None,
            view_is_nav: false,
            view_still: None,
            nav_still: None,
            info: None,
            info_still: None,
            bridge_movie: None,
            panel: None,
            panel_still: None,
            segue: None,
            screens,
            wait: 0.0,
            tick_time: 0.0,
            map_mode: 1,
            pressed: None,
            hilite: None,
            dragging: false,
            sounds: Sounds::new(assets.mac_sounds.clone(), assets.cast_sounds.clone()),
            status: String::new(),
            game,
        };
        screen.queue.extend(screen.game.redraw());
        Ok(screen)
    }

    pub fn busy(&self) -> bool {
        self.playing() || self.game.scan_busy()
    }

    /// A movie, clip, wait or queued effect is in progress.
    fn playing(&self) -> bool {
        self.view.is_some()
            || self.info.is_some()
            || self.bridge_movie.is_some()
            || self.panel.is_some()
            || self.segue.is_some()
            || self.wait > 0.0
            || !self.queue.is_empty()
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn navigator(&self) -> &Navigator {
        self.game.nav()
    }

    fn act(&mut self, action: Action, pressed: Option<usize>) {
        let fx = self.game.act(action);
        if !fx.is_empty() {
            self.pressed = pressed;
        }
        self.queue.extend(fx);
    }

    /// Mouse down or a touch starting, at stage coordinates.
    pub fn press(&mut self, x: f32, y: f32) {
        if self.playing() {
            return;
        }
        // While the manual scan box glides, only the view takes clicks.
        if !self.game.scan_busy() && self.press_interface(x, y) {
            return;
        }
        // The view window drives the scanner.
        if let Some(img) = self.cast.get(self.frame.sprite(CH_VIEW).map_or(0, |s| s.cast))
            && img.hit(loc(&self.frame, CH_VIEW), x, y)
        {
            self.dragging = true;
            self.act(Action::ViewPress(x as i32, y as i32), None);
        }
    }

    /// A press on the interface around the view; true if something took it.
    fn press_interface(&mut self, x: f32, y: f32) -> bool {
        // Navigation buttons.
        let buttons = self.button_casts();
        for (i, cast) in buttons.iter().enumerate() {
            let ch = CH_NAV_FIRST + i;
            if let Some(img) = cast.and_then(|c| self.cast.get(c))
                && img.hit(loc(&self.frame, ch), x, y)
            {
                self.act(Action::Nav(NAV_ACTIONS[i]), Some(ch));
                return true;
            }
        }
        // Function buttons (Lingo's FunctionButtons).
        if let Some(img) = self.cast.get(CAST_FUNCTION)
            && img.hit(loc(&self.frame, CH_FUNCTION), x, y)
        {
            if FUNCTION_SCAN.contains(&y) {
                self.act(Action::Scan, None);
            } else if FUNCTION_ARM.contains(&y) {
                self.act(Action::Arm, None);
            } else if FUNCTION_CONTROLS.contains(&y) {
                self.act(Action::Controls, None);
            }
            return true;
        }
        // Map mode button: three positions left to right.
        if let Some(img) = self.cast.get(CAST_MODE_BUTTON + self.map_mode as u16)
            && img.hit(loc(&self.frame, CH_MODE), x, y)
        {
            self.map_mode = if x < 394.0 { 1 } else if x < 435.0 { 2 } else { 3 };
            return true;
        }
        // The computer screen.
        if self.game.interface() == Interface::Computer {
            if let Some(b) = self.game.computer_button_at(x as i32, y as i32) {
                self.hilite = self.computer_hilite(b).map(|c| (c, HILITE_TIME));
                self.panel_still = None;
                self.act(Action::Computer(b), None);
            }
            return true;
        }
        // Scan and probe-control buttons on the control screen.
        if let Some(b) = self.game.button_at(x as i32, y as i32) {
            self.hilite = self.hilite_cast(b).map(|c| ((c, loc(&self.frame, CH_SCREEN)), HILITE_TIME));
            self.act(Action::Press(b), None);
            return true;
        }
        // Door and elevator panels on the control screen.
        if let Interface::Nav(panel) = self.game.interface() {
            match panel {
                Panel::Door { .. } | Panel::Elevator { .. } if (352.0..436.0).contains(&x) && (62.0..75.0).contains(&y) => {
                    self.act(Action::Nav(NavAction::Door), None);
                    return true;
                }
                Panel::Elevator { .. } if (352.0..480.0).contains(&x) && (76.0..91.0).contains(&y) => {
                    let offset = x as i32 - 352;
                    if offset % 22 < 18 {
                        self.act(Action::Nav(NavAction::Elevator((offset / 22 + 1) as u8)), None);
                    }
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// The mouse or touch moving while down.
    pub fn drag(&mut self, x: f32, y: f32) {
        if self.dragging {
            self.act(Action::ViewDrag(x as i32, y as i32), None);
        }
    }

    pub fn release(&mut self) {
        if std::mem::take(&mut self.dragging) {
            self.act(Action::ViewRelease, None);
        }
    }

    /// The cast lit while a computer button is pressed, and where.
    fn computer_hilite(&self, b: ComputerButton) -> Option<Hilite> {
        let screen = loc(&self.frame, CH_SCREEN);
        Some(match b {
            ComputerButton::Item(n @ 1..=4) => {
                let menu = self.game.computer_view().is_some_and(|v| v.menu);
                (CAST_COMPUTER_HILITE + !menu as u16, (screen.0, n as i32 * 14 + 78))
            }
            ComputerButton::Item(n) => (CAST_COMPUTER_HILITE + n as u16 - 3, screen),
            ComputerButton::Key(0) => (CAST_KEYPAD + 10, screen),
            ComputerButton::Key(d) => (CAST_KEYPAD + d as u16, screen),
            ComputerButton::Clear => (CAST_KEYPAD + 11, screen),
            ComputerButton::Enter => (CAST_KEYPAD + 12, screen),
            ComputerButton::Cancel => (CAST_KEYPAD + 13, screen),
        })
    }

    /// The cast lit while a control-screen button is pressed.
    fn hilite_cast(&self, b: Button) -> Option<u16> {
        let auto = self.game.auto_scan() as u16;
        Some(match b {
            Button::Acquire => CAST_SCAN_INTERFACE + 2,
            Button::ExitScanner => CAST_SCAN_INTERFACE + 4,
            Button::Inventory => CAST_CONTROLS_MENU + 2,
            Button::AutoScan => CAST_CONTROLS_MENU + 4 + auto * 2,
            Button::Reboot => CAST_CONTROLS_MENU + 8,
            Button::ExitControls => CAST_CONTROLS_MENU + 10,
            Button::Previous => CAST_CONTROLS_MENU + 12,
            Button::Delete => CAST_CONTROLS_MENU + 15,
            Button::Sample(_) => return None,
        })
    }

    /// Keyboard controls (design doc §4.8), plus testing aids.
    pub fn key(&mut self, key: macroquad::input::KeyCode) {
        use macroquad::input::KeyCode::*;
        match key {
            // Not in the original: map mode, phase (testing), scan and controls.
            M => self.map_mode = self.map_mode % 3 + 1,
            P => self.game.set_phase(self.game.phase() % 4 + 1),
            S if !self.busy() => self.act(Action::Scan, None),
            C if !self.busy() => self.act(Action::Controls, None),
            X if !self.busy() => self.act(Action::Arm, None),
            // The keypad (Backspace cancels, as in the original; Enter is ours).
            _ if self.game.computer_view().is_some_and(|v| v.digits.is_some()) => {
                let button = match key {
                    Key0 | Kp0 => ComputerButton::Key(0),
                    Key1 | Kp1 => ComputerButton::Key(1),
                    Key2 | Kp2 => ComputerButton::Key(2),
                    Key3 | Kp3 => ComputerButton::Key(3),
                    Key4 | Kp4 => ComputerButton::Key(4),
                    Key5 | Kp5 => ComputerButton::Key(5),
                    Key6 | Kp6 => ComputerButton::Key(6),
                    Key7 | Kp7 => ComputerButton::Key(7),
                    Key8 | Kp8 => ComputerButton::Key(8),
                    Key9 | Kp9 => ComputerButton::Key(9),
                    Backspace => ComputerButton::Cancel,
                    Enter | KpEnter => ComputerButton::Enter,
                    _ => return,
                };
                if !self.busy() {
                    self.hilite = self.computer_hilite(button).map(|c| (c, HILITE_TIME));
                    self.panel_still = None;
                    self.act(Action::Computer(button), None);
                }
            }
            // Testing aids for autoplay: click the scan point (or, with the
            // arm out, the port), press Acquire.
            K if !self.busy() => {
                let nav = self.game.nav();
                let target = if self.game.arm_view().is_some() {
                    nav.node().ports[nav.facing().index()].map(|(h, v)| (h as i32, v as i32 + 40))
                } else {
                    let point = nav.node().scan.get(self.game.skill() as usize - 1).and_then(|s| s[nav.facing().index()]);
                    point.map(|p| (p.h as i32 + 2, p.v as i32))
                };
                if let Some((x, y)) = target {
                    self.act(Action::ViewPress(x, y), None);
                    self.act(Action::ViewRelease, None);
                }
            }
            A if !self.busy() => self.act(Action::Press(Button::Acquire), None),
            G if !self.busy() => {
                let fx = self.game.debug_phase_two();
                self.queue.extend(fx);
            }
            _ => {
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
                if !self.busy() {
                    let pressed = NAV_ACTIONS.iter().position(|a| *a == action).map(|i| CH_NAV_FIRST + i);
                    self.act(Action::Nav(action), pressed);
                }
            }
        }
    }

    /// Run queued effects (each movie, clip and wait finishes before the
    /// next) and, while nothing plays, the game's animation ticks.
    pub async fn update(&mut self, assets: &GameAssets, dt: f32) {
        if let Some((_, t)) = &mut self.hilite {
            *t -= dt;
            if *t <= 0.0 {
                self.hilite = None;
            }
        }
        if !self.playing() || self.game.scan_busy() {
            self.tick_time += dt;
            while self.tick_time >= TICK {
                self.tick_time -= TICK;
                for e in self.game.tick() {
                    match e {
                        // Blips play at once rather than waiting their turn.
                        Effect::Sound(s) => self.sounds.play(&s).await,
                        e => self.queue.push_back(e),
                    }
                }
            }
        }
        loop {
            if self.wait > 0.0 {
                self.wait -= dt;
                return;
            }
            if let Some(p) = &mut self.bridge_movie {
                if !p.update(dt) {
                    return;
                }
                self.bridge_movie = None;
                self.map_mode = 1;
                continue;
            }
            if let Some(p) = &mut self.segue {
                if !p.update(dt) {
                    return;
                }
                self.segue = None;
                continue;
            }
            if let Some((p, keep)) = &mut self.panel {
                if !p.update(dt) {
                    return;
                }
                self.panel_still = if *keep { p.current().map(|f| (f.to_vec(), p.clip.width)) } else { None };
                self.panel = None;
                continue;
            }
            if let Some(p) = &mut self.view {
                if !p.update(dt) {
                    return;
                }
                self.view_still = p.current().map(|f| (f.to_vec(), p.clip.width));
                if self.view_is_nav {
                    self.nav_still = self.view_still.clone();
                }
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
                            self.view_is_nav = folder.starts_with("Navigation");
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
                Effect::RestoreView => self.view_still = self.nav_still.clone(),
                Effect::Panel { folder, name, keep } => match Clip::load(&assets.file(&folder, &name)) {
                    Ok(clip) => {
                        self.status = format!("{folder}/{name}");
                        self.panel_still = None;
                        self.panel = Some((Player::new(clip), keep));
                    }
                    Err(e) => self.status = e,
                },
                Effect::ErasePanel => self.panel_still = None,
                Effect::Segue { folder, name } => match Clip::load(&assets.file(&folder, &name)) {
                    Ok(clip) => {
                        self.status = format!("{folder}/{name}");
                        self.segue = Some(Player::new(clip));
                    }
                    Err(e) => self.status = e,
                },
                Effect::Bridge { folder, name } => match Clip::load(&assets.file(&folder, &name)) {
                    Ok(clip) => {
                        self.status = format!("{folder}/{name}");
                        self.bridge_movie = Some(Player::new(clip));
                    }
                    Err(e) => self.status = e,
                },
            }
        }
    }

    /// Cast to show on each navigation button (None = hidden).
    fn button_casts(&self) -> [Option<u16>; 6] {
        let b = self.game.nav().buttons();
        let shown = [b.up, b.down, b.forward, b.back, b.left, b.right];
        let double = self.game.nav().repeat() as u16;
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
        let n = self.game.graph().at_line(line)?;
        let i = item.checked_sub(2)?;
        Some((n.map.get(i).copied().flatten()?, n.map.get(i + 1).copied().flatten()?))
    }

    fn draw_cast(&self, canvas: &mut Canvas, cast: u16, at: (i32, i32), transparent: bool) {
        if let Some(img) = self.cast.get(cast) {
            canvas.draw(img, at, transparent);
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.pixels.fill(0);
        if let Some(movie) = &self.bridge_movie {
            self.draw_bridge(canvas, movie);
            return;
        }
        let managed = |ch: usize| {
            matches!(
                ch,
                CH_VIEW | CH_ARM | CH_MAP | CH_PROBE_ICON | CH_DEFENDER_ICON | CH_SCREEN | CH_ENERGY | CH_FUNCTION | CH_MODE | CH_PANEL
            ) || CH_SCAN_INDICATORS.contains(&ch)
                || (CH_NAV_FIRST..CH_NAV_FIRST + 6).contains(&ch)
        };

        // The view window, with the scan box and the arm (channel 2) over it;
        // the interface frame (channel 3) covers what reaches outside.
        if let (Some(img), Some((pixels, width))) =
            (self.cast.get(self.frame.sprite(CH_VIEW).map_or(0, |s| s.cast)), self.view_frame())
        {
            let (left, top) = img.top_left(loc(&self.frame, CH_VIEW));
            let (ix, iy, _, _) = VIEW_INNER;
            canvas.blit(pixels, width, VIEW_INNER, left + ix as i32, top + iy as i32, false);
        }
        let scan = self.game.scan_view();
        if self.view.is_none()
            && let Some((look, at)) = scan.scan_box
        {
            let offset = match look {
                BoxLook::Plain => 0,
                BoxLook::Flash => 1,
                BoxLook::Idle => 2,
                BoxLook::Locked => 3,
            };
            self.draw_cast(canvas, CAST_SCAN_BOX + offset, at, true);
        }
        if let Some((cast, at)) = self.game.arm_view() {
            self.draw_cast(canvas, cast, at, true);
        }

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
        if self.view.is_none()
            && let Some((frame, at)) = scan.marker
        {
            let cast = if frame >= 6 { CAST_SCAN_HIT } else { CAST_SCAN_MARKER + frame as u16 };
            self.draw_cast(canvas, cast, at, true);
        }

        // Control screen: node screen, door/elevator panel, scan interface
        // or probe controls.
        let screen_loc = loc(&self.frame, CH_SCREEN);
        let interface = self.game.interface();
        match interface {
            Interface::Nav(panel) => {
                let (screen_cast, lamp) = match panel {
                    Panel::Node => (CAST_NODE_SCREEN, None),
                    Panel::Door { open } => (CAST_DOOR + if open { 2 } else { 0 }, None),
                    Panel::Elevator { open, level } => (CAST_ELEVATOR + open as u16, Some(CAST_ELEVATOR + 1 + level as u16)),
                };
                self.draw_cast(canvas, screen_cast, screen_loc, false);
                if let Some(c) = lamp {
                    self.draw_cast(canvas, c, loc(&self.frame, CH_PANEL), true);
                }
            }
            Interface::Scan => self.draw_cast(canvas, CAST_SCAN_INTERFACE, screen_loc, false),
            Interface::Controls(ControlsPage::Menu) => {
                self.draw_cast(canvas, CAST_CONTROLS_MENU, screen_loc, false);
                self.draw_cast(canvas, CAST_AUTO_SCAN_LAMP + self.game.auto_scan() as u16 * 2, screen_loc, true);
            }
            Interface::Controls(ControlsPage::Inventory { selected }) => {
                self.draw_cast(canvas, CAST_CONTROLS_INVENTORY, screen_loc, false);
                for (i, &dna) in self.game.kit().iter().enumerate().take(INVENTORY.len()) {
                    let cast = CAST_CREW + (dna.max(1) as u16 - 1) * 2 + selected[i] as u16;
                    self.draw_cast(canvas, cast, INVENTORY[i], true);
                }
            }
            Interface::Computer => {
                if let Some(view) = self.game.computer_view() {
                    let cast = view
                        .screen
                        .as_ref()
                        .and_then(|(name, page)| self.screens.get(name).map(|&c| (c as i32 + page) as u16))
                        .unwrap_or(CAST_EMPTY_SCREEN);
                    self.draw_cast(canvas, cast, screen_loc, false);
                    if view.message {
                        self.draw_cast(canvas, CAST_MESSAGE, screen_loc, true);
                    }
                    for (text, at) in &view.texts {
                        draw_digits(canvas, text, *at);
                    }
                    if let Some(digits) = &view.digits {
                        draw_digits(canvas, digits, DIGITS_AT);
                    }
                }
            }
        }
        if let Some(((cast, at), _)) = self.hilite {
            self.draw_cast(canvas, cast, at, true);
        }
        // The location strip, except over the computer screen (which covers it).
        if interface != Interface::Computer
            && let Some((pixels, width)) = self.info_frame()
        {
            let h = pixels.len() / 4 / width.max(1);
            canvas.blit(pixels, width, (0, 0, width, h), INFO_STRIP.0, INFO_STRIP.1, false);
        }
        // Computer clips over the control screen; the ETA ship sits on its clip.
        if let Some((pixels, width)) = self.panel_frame() {
            let h = pixels.len() / 4 / width.max(1);
            canvas.blit(pixels, width, (0, 0, width, h), PANEL_AT.0, PANEL_AT.1, false);
        }
        if self.panel.is_none()
            && let Some(at) = self.game.computer_view().and_then(|v| v.eta)
        {
            self.draw_cast(canvas, CAST_ETA_SHIP, at, true);
        }
        if self.view.is_none()
            && self.info.is_none()
            && let Some(text) = self.game.scan_view().text
        {
            let offset = match text {
                ScanText::Active => 0,
                ScanText::Contact => 1,
                ScanText::Ready => 2,
                ScanText::Seeking => 3,
            };
            self.draw_cast(canvas, CAST_SCAN_TEXT + offset, screen_loc, false);
        }
        // Power bar and scan indicators, except under the probe controls
        // and the computer.
        if !matches!(interface, Interface::Controls(_) | Interface::Computer) {
            let (x, y0, y1) = ENERGY_BAR;
            canvas.fill(x, y0, x + self.game.energy().clamp(0, ENERGY_FULL), y1, [0x00, 0xDD, 0x00]);
        }
        for (frame, x) in self.game.scan_bars().into_iter().flatten() {
            self.draw_cast(canvas, CAST_SCAN_BARS + frame as u16, (x, SCAN_BAR_V), true);
        }
        let function = match self.game.function() {
            None => CAST_FUNCTION,
            Some(Function::Scan) => CAST_FUNCTION + 2,
            Some(Function::Arm) => CAST_FUNCTION + 4,
            Some(Function::Controls) => CAST_FUNCTION + 5,
        };
        self.draw_cast(canvas, function, loc(&self.frame, CH_FUNCTION), true);

        // Map, probe and defender.
        let nav = self.game.nav();
        let probe_level = nav.level();
        let defender_level = self.game.graph().at_line(DEFENDER_LINE).map_or(0, |n| n.level());
        let probe_item = if nav.in_ladder() { 6 } else { 2 };
        let (map_cast, probe, defender) = match self.map_mode {
            1 => (
                CAST_LEVEL_MAPS + probe_level as u16 - 1,
                self.map_point(nav.line(), probe_item).map(|p| (CAST_PROBE_ICON + nav.facing().index() as u16, p)),
                (defender_level == probe_level)
                    .then(|| self.map_point(DEFENDER_LINE, 2).map(|p| (CAST_DEFENDER_ICON, p)))
                    .flatten(),
            ),
            2 => (
                CAST_LEVEL_MAPS + defender_level as u16 - 1,
                (defender_level == probe_level)
                    .then(|| self.map_point(nav.line(), probe_item).map(|p| (CAST_PROBE_ICON + nav.facing().index() as u16, p)))
                    .flatten(),
                self.map_point(DEFENDER_LINE, 2).map(|p| (CAST_DEFENDER_ICON, p)),
            ),
            _ => (
                CAST_LEVEL_MAPS + 6,
                self.map_point(nav.line(), 4).map(|p| (CAST_PROBE_OVERVIEW, p)),
                self.map_point(DEFENDER_LINE, 4).map(|p| (CAST_DEFENDER_OVERVIEW, p)),
            ),
        };
        self.draw_cast(canvas, map_cast, loc(&self.frame, CH_MAP), false);
        for (cast, point) in [probe, defender].into_iter().flatten() {
            self.draw_cast(canvas, cast, point, true);
        }
        self.draw_cast(canvas, CAST_MODE_BUTTON + self.map_mode as u16, loc(&self.frame, CH_MODE), false);

        // Navigation buttons.
        for (i, cast) in self.button_casts().iter().enumerate() {
            if let Some(c) = cast {
                self.draw_cast(canvas, *c, loc(&self.frame, CH_NAV_FIRST + i), true);
            }
        }
        if let Some(movie) = &self.segue {
            self.draw_segue(canvas, movie);
        }
    }

    /// A segue playing over the whole interface.
    fn draw_segue(&self, canvas: &mut Canvas, movie: &Player) {
        self.draw_cast(canvas, CAST_SEGUE_SCREEN, loc(&self.frame, CH_SCREEN), false);
        if let Some(frame) = movie.current() {
            let w = movie.clip.width;
            let h = frame.len() / 4 / w.max(1);
            canvas.blit(frame, w, (0, 0, w, h), SEGUE_AT.0, SEGUE_AT.1, false);
        }
    }

    fn panel_frame(&self) -> Option<(&[u8], usize)> {
        match &self.panel {
            Some((p, _)) => p.current().map(|f| (f, p.clip.width)),
            None => self.panel_still.as_ref().map(|(f, w)| (f.as_slice(), *w)),
        }
    }

    /// The bridge screen with a phase movie playing on its monitor.
    fn draw_bridge(&self, canvas: &mut Canvas, movie: &Player) {
        if let Some((p, (x, y))) = &self.bridge.picture {
            canvas.blit(&p.rgba, p.width, (0, 0, p.width, p.height), *x, *y, true);
        }
        if let Some(frame) = movie.current() {
            let w = movie.clip.width;
            let h = frame.len() / 4 / w.max(1);
            canvas.blit(frame, w, (0, 0, w, h), self.bridge.movie_at.0, self.bridge.movie_at.1, false);
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
        self.game.nav().facing()
    }
}

/// Digits (and spaces) in a small 5x7 font, standing in for the screens'
/// text members (codes and keypad entry), in the screen green.
fn draw_digits(canvas: &mut Canvas, text: &str, at: (i32, i32)) {
    const GLYPHS: [[u8; 7]; 10] = [
        [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
    ];
    let mut x = at.0;
    for c in text.chars() {
        if let Some(d) = c.to_digit(10) {
            for (row, bits) in GLYPHS[d as usize].iter().enumerate() {
                for col in 0..5 {
                    if bits & (0x10 >> col) != 0 {
                        let (px, py) = (x + col, at.1 + row as i32);
                        canvas.fill(px, py, px + 1, py + 1, TEXT_COLOR);
                    }
                }
            }
        }
        x += 7;
    }
}
