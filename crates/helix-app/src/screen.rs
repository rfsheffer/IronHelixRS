//! The gameplay screen: the original interface laid out from the Director
//! score, driven by the rules in helix-core.
//!
//! Sprite channel numbers and cast numbers below are the ones the original
//! Lingo uses (see docs/design.md §2); positions come from the score's
//! gameplay frame, so nothing about the layout is guessed.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

use director::Picture;
use director::score::Frame;
use helix_core::game::ENERGY_FULL;
use helix_core::scan::TICK;
use helix_core::{
    Access, Action, BoxLook, Button, ComputerButton, ComputerData, ControlsPage, DefenderData, Effect, Function, Game,
    Interface, Lamp, NavAction, Navigator, Panel, ScanText,
};
use helixdb::Direction;

use crate::assets::GameAssets;
use crate::sound::Sounds;
use crate::sprites::{Canvas, Cast};
use crate::video::{Clip, Player};

// Sprite channels.
const CH_VIEW: usize = 1;
const CH_ARM: usize = 2;
const CH_VOLUME: usize = 5;
const CH_LAMP: usize = 4;
const CH_MAP: usize = 6;
const CH_PROBE_ICON: usize = 7;
const CH_DEFENDER_ICON: usize = 8;
const CH_SCREEN: usize = 9;
const CH_ENERGY: usize = 10;
const CH_SCAN_INDICATORS: std::ops::RangeInclusive<usize> = 11..=14;
const CH_FUNCTION: usize = 15;
const CH_MODE: usize = 16;
const CH_NAV_FIRST: usize = 17; // up, down, forward, back, left, right
const CH_GAME_CONTROLS: usize = 23;
const CH_PANEL: usize = 24;

// Cast members.
const CAST_NAV_BUTTONS: u16 = 9; // 3 per button: single, double, pressed
const CAST_LAMP: u16 = 30; // ambient blip; +1 warning one, +2 warning two
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
/// The view on the death, entering and win screens (score frames Segue,
/// Entering and WinFrame), where the game's view window shows cast 39.
const CAST_BLANK_MONITOR: u16 = 41;
const CAST_SCAN_INTERFACE: u16 = 169;
const CAST_COMPUTER_HILITE: u16 = 177; // square row; +1 oval row; +item-3 bottom bar
const CAST_ETA_SHIP: u16 = 214;
const CAST_MESSAGE: u16 = 219;
const CAST_SCAN_TEXT: u16 = 220; // active, contact, ready, seeking
const CAST_SEGUE_SCREEN: u16 = 225;
const CAST_KEYPAD: u16 = 233; // + key: 1-9, 10 = 0, 11 clear, 12 enter, 13 cancel
const CAST_GAME_CONTROLS: u16 = 247; // the bar; lit: +2 Quit, +4 Save, +6 Pause, +8 Notes
const CAST_GAME_PAUSED: u16 = 228;
const CAST_QUIT_DIALOG: u16 = 281; // +1 its buttons; lit: +3 Quit, +5 Cancel, +7 Save

/// The ambient lamp blip moves in steps of this many pixels.
const LAMP_STEP: i32 = 18;
/// Minutes left, on the death screen: tens and ones digits (casts 289-298).
const CAST_DIGITS: u16 = 289;
const MINUTES_AT: [(i32, i32); 2] = [(124, 262), (132, 262)];
/// The game controls bar's buttons (Lingo's GameControls).
const QUIT_BUTTON: std::ops::Range<f32> = 0.0..74.0;
const SAVE_BUTTON: std::ops::Range<f32> = 79.0..125.0;
const PAUSE_BUTTON: std::ops::Range<f32> = 132.0..185.0;
const NOTES_BUTTON: std::ops::Range<f32> = 225.0..271.0;
/// How long a message (such as "Game saved") shows over the view.
const TOAST_TIME: f32 = 2.5;
/// The reboot light show (Lingo's RebootProbe): dark for 30 ticks, four
/// 7-tick flashes, the deck maps flicking by, then 15 ticks dark.
const REBOOT_DARK: f32 = 0.5;
const REBOOT_STEP: f32 = 7.0 / 60.0;
const REBOOT_MAPS: f32 = 7.0 / 30.0;
const REBOOT_TIME: f32 = REBOOT_DARK + 4.0 * REBOOT_STEP + REBOOT_MAPS + 0.25;
/// Where the notes appear (over the view) and how wide they may run.
pub const NOTES_AT: (f32, f32) = (28.0, 21.0);
pub const NOTES_WIDTH: f32 = 227.0;
pub const NOTES_BOTTOM: f32 = 140.0;
/// The Jam function button's band.
const FUNCTION_JAM: std::ops::Range<f32> = 47.0..62.0;
/// The defender's marker slides this far per tick (Lingo's AnimateMap).
const DEFENDER_SPEED: f32 = 7.0;

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

/// The probe's map marker slides this far per tick after a move (Lingo's
/// AnimateMap speed for the probe).
const MARKER_SPEED: f32 = 2.0;

/// A lit button: its cast and where.
type Hilite = (u16, (i32, i32));

const NAV_ACTIONS: [NavAction; 6] =
    [NavAction::Up, NavAction::Down, NavAction::Forward, NavAction::Back, NavAction::TurnLeft, NavAction::TurnRight];

/// How a game starts.
pub enum Start {
    /// At the start, drawn at once (testing).
    Plain,
    /// The intro video and the probe entering the ship.
    Intro,
    /// From a saved game's text.
    Load(String),
}

/// What the reboot light show shows at a moment.
enum RebootLook {
    Dark,
    Buttons,
    Lights,
    Map(u16),
}

/// The bridge screen (score frame `BridgeBG`) where phase movies play.
struct Bridge {
    picture: Option<(Picture, (i32, i32))>,
    movie_at: (i32, i32),
}

pub struct GameScreen {
    frame: Frame,
    cast: Rc<Cast>,
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
    /// The node line the map shows during an elevator ride.
    map_line: Option<usize>,
    /// Where the probe's marker is drawn: its deck and position, which
    /// slides toward the probe's node after each move.
    marker: Option<(u8, (f32, f32))>,
    /// The same for the defender's marker.
    defender_marker: Option<(u8, (f32, f32))>,
    /// The segue's last frame stays up through the minutes-left pause.
    segue_still: Option<(Vec<u8>, usize)>,
    minutes: Option<u32>,
    /// All probes lost: the game is over.
    game_over: bool,
    /// Showing the score's Entering frame (see `Effect::Entering`).
    entering: bool,
    /// The notes are up over the view (until the next action).
    notes_up: bool,
    wait: f32,
    tick_time: f32,
    map_mode: u8,
    pressed: Option<usize>,
    hilite: Option<(Hilite, f32)>,
    dragging: bool,
    /// The volume slider's knob.
    slider: crate::volume::Slider,
    /// The knob was let go: the sound check plays.
    sound_check: bool,
    sounds: Sounds,
    pub status: String,
    /// Paused (Lingo's PauseGame): any key or click carries on.
    paused: bool,
    /// The quit dialog is up, with the lit button while pressed.
    quit_dialog: Option<Option<u16>>,
    /// The player chose to quit.
    quitting: bool,
    /// A message over the view and how long it stays.
    toast: Option<(String, f32)>,
    /// The reboot light show's time so far.
    reboot: Option<f32>,
    /// Won on skill 5: the extended ending follows.
    extended_ending: bool,
    save_file: PathBuf,
}

fn loc(frame: &Frame, channel: usize) -> (i32, i32) {
    frame.sprite(channel).map_or((0, 0), |s| (s.loc_h as i32, s.loc_v as i32))
}

impl GameScreen {
    pub fn new(assets: &GameAssets, skill: u8, seed: u64, start: Start) -> Result<Self, String> {
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

        let cast = assets.cast.clone();
        let mut nav = Navigator::new(assets.data.nav.clone());
        *nav.access_mut() = Access::new(assets.data.clearance.clone());
        let data = ComputerData {
            cmp: assets.data.cmp_lines.clone(),
            no_arm: assets.data.no_arm.clone(),
            clue_lists: assets.data.video_clue_lists.clone(),
            clue_notes: assets.data.clue_notes.clone(),
            notes_default: assets.data.notes_default.clone(),
        };
        let mut game = Game::new(nav, skill.clamp(1, 5), seed)
            .with_computers(data)
            .with_defender(DefenderData::new(&assets.data.defender, &assets.data.deaths))
            .with_failures(assets.data.probe_failures.clone());
        let opening = match start {
            Start::Plain => game.redraw(),
            Start::Intro => game.intro(),
            Start::Load(text) => {
                game.load(&text)?;
                game.redraw()
            }
        };
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
            map_line: None,
            marker: None,
            defender_marker: None,
            segue_still: None,
            minutes: None,
            game_over: false,
            entering: false,
            notes_up: false,
            wait: 0.0,
            tick_time: 0.0,
            map_mode: 1,
            pressed: None,
            hilite: None,
            dragging: false,
            slider: Default::default(),
            sound_check: false,
            sounds: Sounds::new(assets.mac_sounds.clone(), assets.cast_sounds.clone()),
            status: String::new(),
            paused: false,
            quit_dialog: None,
            quitting: false,
            toast: None,
            reboot: None,
            extended_ending: false,
            save_file: assets.save_file.clone(),
            game,
        };
        screen.queue.extend(opening);
        Ok(screen)
    }

    pub fn busy(&self) -> bool {
        self.playing() || self.game.scan_busy()
    }

    /// All probes are lost and the game-over movies have played.
    pub fn finished(&self) -> bool {
        self.game_over && !self.playing()
    }

    /// The player quit (from the quit dialog).
    pub fn quitting(&self) -> bool {
        self.quitting
    }

    /// Won on skill 5: the Sega CD's extended ending is due.
    pub fn extended_ending(&self) -> bool {
        self.extended_ending
    }

    /// A message to show over the view.
    pub fn toast(&self) -> Option<&str> {
        self.toast.as_ref().map(|(t, _)| t.as_str())
    }

    /// Lingo's PauseGame: the scanner, arm and controls close, sound stops.
    fn pause(&mut self) {
        self.notes_up = false;
        let fx = self.game.pause();
        self.queue.extend(fx.into_iter().filter(|e| !matches!(e, Effect::Sound(_))));
        self.sounds.stop();
        self.paused = true;
    }

    /// Lingo's QuitGame: the quit dialog over the control screen; the
    /// game waits.
    fn open_quit_dialog(&mut self) {
        self.notes_up = false;
        self.sounds.stop();
        self.quit_dialog = Some(None);
    }

    /// Lingo's SaveGame, to the one save slot.
    fn save(&mut self) -> bool {
        self.notes_up = false;
        let text = self.game.save();
        let result = self
            .save_file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&self.save_file, text));
        match result {
            Ok(()) => {
                self.toast = Some(("Game saved.".into(), TOAST_TIME));
                true
            }
            Err(e) => {
                self.toast = Some((format!("Couldn't save: {e}"), TOAST_TIME * 2.0));
                false
            }
        }
    }

    /// The quit dialog's button under a point (lit cast), by its bands.
    fn quit_button_at(&self, x: f32, y: f32) -> Option<u16> {
        let img = self.cast.get(CAST_QUIT_DIALOG + 1)?;
        if !img.hit(loc(&self.frame, CH_SCREEN), x, y) {
            return None;
        }
        if x < 237.0 {
            Some(CAST_QUIT_DIALOG + 3)
        } else if (246.0..312.0).contains(&x) {
            Some(CAST_QUIT_DIALOG + 5)
        } else if x > 326.0 {
            Some(CAST_QUIT_DIALOG + 7)
        } else {
            None
        }
    }

    /// Lingo's QuitHilites: Quit, Cancel or Save (then quit).
    fn quit_choice(&mut self, lit: u16) {
        self.quit_dialog = None;
        match lit - CAST_QUIT_DIALOG {
            3 => self.quitting = true,
            7 => self.quitting = self.save(),
            _ => {}
        }
    }

    /// What the reboot light show shows now.
    fn reboot_look(&self) -> Option<RebootLook> {
        let t = self.reboot?;
        let after_flashes = REBOOT_DARK + 4.0 * REBOOT_STEP;
        Some(if t < REBOOT_DARK {
            RebootLook::Dark
        } else if t < after_flashes {
            match ((t - REBOOT_DARK) / REBOOT_STEP) as u32 {
                0 => RebootLook::Buttons,
                2 => RebootLook::Lights,
                _ => RebootLook::Dark,
            }
        } else if t < after_flashes + REBOOT_MAPS {
            RebootLook::Map(((t - after_flashes) * 30.0) as u16)
        } else {
            RebootLook::Dark
        })
    }

    /// The defender's deck and map point (6 = in a ladder).
    fn defender_target(&self) -> Option<(u8, (i32, i32))> {
        let d = self.game.defender();
        let item = if d.in_ladder() { 6 } else { 2 };
        Some((d.level(), self.map_point(d.line(), item)?))
    }

    /// Slide the defender's marker one step, or put it on a new deck.
    fn step_defender_marker(&mut self) {
        let Some((level, (tx, ty))) = self.defender_target() else { return };
        let target = (tx as f32, ty as f32);
        match &mut self.defender_marker {
            Some((l, pos)) if *l == level => {
                let (dx, dy) = (target.0 - pos.0, target.1 - pos.1);
                let longest = dx.abs().max(dy.abs());
                if longest <= DEFENDER_SPEED {
                    *pos = target;
                } else {
                    pos.0 += dx / longest * DEFENDER_SPEED;
                    pos.1 += dy / longest * DEFENDER_SPEED;
                }
            }
            _ => self.defender_marker = Some((level, target)),
        }
    }

    /// The probe's deck and map point (map item 2, or 6 in a ladder), on
    /// the line the map is following.
    fn probe_target(&self) -> Option<(u8, (i32, i32))> {
        let nav = self.game.nav();
        let line = self.map_line.unwrap_or(nav.line());
        let item = if self.map_line.is_none() && nav.in_ladder() { 6 } else { 2 };
        let level = self.game.graph().at_line(line)?.level();
        Some((level, self.map_point(line, item)?))
    }

    /// The marker is still sliding to the probe's node.
    fn marker_moving(&self) -> bool {
        match (self.marker, self.probe_target()) {
            (Some((level, (x, y))), Some((tl, (tx, ty)))) => level == tl && (x != tx as f32 || y != ty as f32),
            _ => false,
        }
    }

    /// Slide the marker one step (Lingo's AnimateMap), or put it straight
    /// on a new deck.
    fn step_marker(&mut self, slide: bool) {
        let Some((level, (tx, ty))) = self.probe_target() else { return };
        let target = (tx as f32, ty as f32);
        match &mut self.marker {
            Some((l, pos)) if *l == level => {
                if !slide {
                    return;
                }
                let (dx, dy) = (target.0 - pos.0, target.1 - pos.1);
                let longest = dx.abs().max(dy.abs());
                if longest <= MARKER_SPEED {
                    *pos = target;
                } else {
                    pos.0 += dx / longest * MARKER_SPEED;
                    pos.1 += dy / longest * MARKER_SPEED;
                }
            }
            _ => self.marker = Some((level, target)),
        }
    }

    /// A movie, clip, wait or queued effect is in progress.
    fn playing(&self) -> bool {
        self.view.is_some()
            || self.info.is_some()
            || self.bridge_movie.is_some()
            || self.panel.is_some()
            || self.segue.is_some()
            || self.wait > 0.0
            || self.reboot.is_some()
            || !self.queue.is_empty()
            || self.marker_moving()
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn navigator(&self) -> &Navigator {
        self.game.nav()
    }

    fn act(&mut self, action: Action, pressed: Option<usize>) {
        // Anything else puts the notes away (a movie would draw over them).
        self.notes_up = false;
        let fx = self.game.act(action);
        if !fx.is_empty() {
            self.pressed = pressed;
        }
        self.queue.extend(fx);
    }

    /// Mouse down or a touch starting, at stage coordinates.
    pub fn press(&mut self, x: f32, y: f32) {
        // Any click carries on from a pause (Lingo's ContPause).
        if self.paused {
            self.paused = false;
            return;
        }
        if self.quit_dialog.is_some() {
            match self.quit_button_at(x, y) {
                Some(lit) => self.quit_dialog = Some(Some(lit)),
                None => self.queue.push_back(Effect::Sound("SysBeeb".into())),
            }
            return;
        }
        // The volume knob works through movies too.
        if self.slider.press(&self.cast, x, y) {
            return;
        }
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

    /// The notes to show over the view, if they're up.
    pub fn notes(&self) -> Option<Vec<String>> {
        (self.notes_up && !self.playing()).then(|| self.game.notes_text())
    }

    /// The Notes button (Lingo's DisplayNotes): not while the scanner's
    /// interface is up.
    fn toggle_notes(&mut self) {
        if self.game.interface() == Interface::Scan {
            self.queue.push_back(Effect::Sound("SysBeeb".into()));
            return;
        }
        self.notes_up = !self.notes_up;
        if self.notes_up {
            self.hilite = Some(((CAST_GAME_CONTROLS + 8, loc(&self.frame, CH_SCREEN)), HILITE_TIME));
        }
    }

    /// A press on the interface around the view; true if something took it.
    fn press_interface(&mut self, x: f32, y: f32) -> bool {
        // The game controls bar: Quit, Save, Pause and Notes.
        if let Some(img) = self.cast.get(CAST_GAME_CONTROLS)
            && img.hit(loc(&self.frame, CH_GAME_CONTROLS), x, y)
        {
            let lit = |n: u16| Some(((CAST_GAME_CONTROLS + n, loc(&self.frame, CH_SCREEN)), HILITE_TIME));
            if QUIT_BUTTON.contains(&x) {
                self.hilite = lit(2);
                self.open_quit_dialog();
            } else if SAVE_BUTTON.contains(&x) {
                self.hilite = lit(4);
                self.save();
            } else if PAUSE_BUTTON.contains(&x) {
                self.hilite = lit(6);
                self.pause();
            } else if NOTES_BUTTON.contains(&x) {
                self.toggle_notes();
            }
            return true;
        }
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
            } else if FUNCTION_JAM.contains(&y) {
                self.act(Action::Jam, None);
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
        if self.slider.drag(x) {
            self.sounds.volume_changed();
            return;
        }
        if self.dragging {
            self.act(Action::ViewDrag(x as i32, y as i32), None);
        }
    }

    pub fn release(&mut self) {
        if self.slider.release() {
            self.sound_check = true;
            return;
        }
        if let Some(Some(lit)) = self.quit_dialog {
            self.quit_choice(lit);
            return;
        }
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
        // Any key carries on from a pause.
        if self.paused {
            self.paused = false;
            return;
        }
        // The quit dialog: Q quit, S save and quit, Esc cancel.
        if self.quit_dialog.is_some() {
            match key {
                Q => self.quit_choice(CAST_QUIT_DIALOG + 3),
                S => self.quit_choice(CAST_QUIT_DIALOG + 7),
                Escape => self.quit_choice(CAST_QUIT_DIALOG + 5),
                _ => {}
            }
            return;
        }
        match key {
            Escape if !self.busy() => self.open_quit_dialog(),
            P if !self.busy() => self.pause(),
            // Not in the original: map mode, phase (testing), scan and controls.
            M => self.map_mode = self.map_mode % 3 + 1,
            F2 => self.game.set_phase(self.game.phase() % 4 + 1),
            S if !self.busy() => self.act(Action::Scan, None),
            C if !self.busy() => self.act(Action::Controls, None),
            X if !self.busy() => self.act(Action::Arm, None),
            Space if !self.busy() => self.act(Action::Jam, None),
            N if !self.busy() => self.toggle_notes(),
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
            Y if !self.busy() => {
                let fx = self.game.debug_caught();
                self.queue.extend(fx);
            }
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
        if let Some((_, t)) = &mut self.toast {
            *t -= dt;
            if *t <= 0.0 {
                self.toast = None;
            }
        }
        // The volume knob was let go (over whatever is playing).
        if std::mem::take(&mut self.sound_check) {
            self.sounds.play_over("SoundCheck").await;
        }
        // Paused, or the quit dialog up: everything waits, the clock too.
        if self.paused || self.quit_dialog.is_some() {
            return;
        }
        // The 90-minute clock runs in real time, through movies too.
        if !self.game_over {
            let (now, later) = split_sounds(self.game.pass_time(dt));
            for s in now {
                self.sounds.play(&s).await;
            }
            self.queue.extend(later);
        }
        // The marker moves to a new deck at once and slides once a move's
        // movie is over.
        self.step_marker(false);
        // The game (the defender, the scanner's animations) waits while any
        // movie or clip plays, as the original's modal playback did; the
        // map markers keep sliding over location strips and computer clips.
        let modal = self.view.is_some()
            || self.info.is_some()
            || self.panel.is_some()
            || self.segue.is_some()
            || self.bridge_movie.is_some()
            || self.wait > 0.0
            || self.reboot.is_some()
            || !self.queue.is_empty()
            || self.entering;
        let slide = self.view.is_none()
            && self.segue.is_none()
            && self.bridge_movie.is_none()
            && self.wait <= 0.0
            && self.queue.is_empty()
            && !self.entering;
        let ticking = !modal || self.game.scan_busy();
        if ticking || slide {
            self.tick_time += dt;
            while self.tick_time >= TICK {
                self.tick_time -= TICK;
                if slide {
                    self.step_marker(true);
                    self.step_defender_marker();
                }
                if !ticking {
                    continue;
                }
                let (now, later) = split_sounds(self.game.tick());
                for s in now {
                    self.sounds.play(&s).await;
                }
                self.queue.extend(later);
            }
        }
        loop {
            if self.wait > 0.0 {
                self.wait -= dt;
                return;
            }
            if let Some(t) = &mut self.reboot {
                *t += dt;
                if *t < REBOOT_TIME {
                    return;
                }
                self.reboot = None;
                continue;
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
                self.segue_still = p.current().map(|f| (f.to_vec(), p.clip.width));
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
                self.map_line = None;
                self.segue_still = None;
                self.minutes = None;
                return;
            };
            // A movie in the view ends the death screen's pause.
            if matches!(effect, Effect::Video { .. }) {
                self.segue_still = None;
                self.minutes = None;
            }
            match effect {
                Effect::Video { folder, name, sound } => {
                    if let Some(s) = sound {
                        self.sounds.play(&s).await;
                    }
                    match Clip::load(&assets.file(&folder, &name)) {
                        Ok(clip) => {
                            self.status = format!("{folder}/{name}");
                            // The entering probe's view ends on the start, like a move.
                            self.view_is_nav = folder.starts_with("Navigation") || folder == "Opening";
                            self.start_audio(&clip).await;
                            self.view = Some(Player::new(clip));
                        }
                        Err(e) => self.status = e,
                    }
                }
                Effect::InfoClip { folder, name } => {
                    if let Ok(clip) = crate::patches::info_clip(assets, &folder, &name) {
                        self.start_audio(&clip).await;
                        self.info = Some(Player::new(clip));
                    }
                }
                Effect::Sound(s) => self.sounds.play(&s).await,
                Effect::StopSound => self.sounds.stop(),
                Effect::Minutes(m) => self.minutes = Some(m),
                Effect::MapMode(m) => self.map_mode = m,
                Effect::GameOver => self.game_over = true,
                Effect::Reboot => self.reboot = Some(0.0),
                Effect::ExtendedEnding => self.extended_ending = true,
                Effect::Entering(on) => {
                    self.entering = on;
                    self.segue_still = None;
                    self.minutes = None;
                    // A new probe: the markers start where things are now.
                    self.marker = None;
                    self.defender_marker = None;
                }
                Effect::MapLine(line) => {
                    self.map_line = Some(line);
                    self.step_marker(false);
                }
                Effect::Wait(seconds) => self.wait = seconds,
                Effect::RestoreView => self.view_still = self.nav_still.clone(),
                Effect::Panel { folder, name, keep } => match Clip::load(&assets.file(&folder, &name)) {
                    Ok(clip) => {
                        self.status = format!("{folder}/{name}");
                        self.panel_still = None;
                        self.start_audio(&clip).await;
                        self.panel = Some((Player::new(clip), keep));
                    }
                    Err(e) => self.status = e,
                },
                Effect::ErasePanel => self.panel_still = None,
                Effect::Segue { folder, name } => match Clip::load(&assets.file(&folder, &name)) {
                    // The next segue covers the minutes left.
                    Ok(clip) => {
                        self.minutes = None;
                        self.status = format!("{folder}/{name}");
                        self.start_audio(&clip).await;
                        self.segue = Some(Player::new(clip));
                    }
                    Err(e) => self.status = e,
                },
                Effect::Bridge { folder, name } => match Clip::load(&assets.file(&folder, &name)) {
                    Ok(clip) => {
                        self.status = format!("{folder}/{name}");
                        self.start_audio(&clip).await;
                        self.bridge_movie = Some(Player::new(clip));
                    }
                    Err(e) => self.status = e,
                },
            }
        }
    }

    /// Start a clip's own sound track, if it has one.
    async fn start_audio(&mut self, clip: &Clip) {
        if let Some((key, wav)) = &clip.audio {
            self.sounds.play_wav(key, wav).await;
        }
    }

    /// Cast to show on each navigation button (None = hidden).
    fn button_casts(&self) -> [Option<u16>; 6] {
        let b = self.game.buttons();
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
                CH_VIEW
                    | CH_ARM
                    | CH_VOLUME
                    | CH_LAMP
                    | CH_MAP
                    | CH_PROBE_ICON
                    | CH_DEFENDER_ICON
                    | CH_SCREEN
                    | CH_ENERGY
                    | CH_FUNCTION
                    | CH_MODE
                    | CH_PANEL
            ) || CH_SCAN_INDICATORS.contains(&ch)
                || (CH_NAV_FIRST..CH_NAV_FIRST + 6).contains(&ch)
        };

        // The view window, with the scan box and the arm (channel 2) over it;
        // the interface frame (channel 3) covers what reaches outside.
        if self.entering && self.view.is_none() {
            // The death, entering and win screens blank the view (a movie
            // there, like the probe entering, plays over it).
            self.draw_cast(canvas, CAST_BLANK_MONITOR, loc(&self.frame, CH_VIEW), false);
        } else if let (Some(img), Some((pixels, width)), None) =
            (self.cast.get(self.frame.sprite(CH_VIEW).map_or(0, |s| s.cast)), self.view_frame(), self.reboot)
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
        crate::volume::Slider::draw(&self.cast, canvas);
        if self.view.is_none()
            && let Some((frame, at)) = scan.marker
        {
            let cast = if frame >= 6 { CAST_SCAN_HIT } else { CAST_SCAN_MARKER + frame as u16 };
            self.draw_cast(canvas, cast, at, true);
        }

        // The reboot light show: the interface dark but for its flashes.
        if let Some(look) = self.reboot_look() {
            match look {
                RebootLook::Dark => {}
                RebootLook::Buttons => {
                    for i in 0..6 {
                        self.draw_cast(canvas, CAST_NAV_BUTTONS + i as u16 * 3, loc(&self.frame, CH_NAV_FIRST + i), true);
                    }
                }
                RebootLook::Lights => {
                    self.draw_cast(canvas, CAST_FUNCTION + 1, loc(&self.frame, CH_FUNCTION), true);
                    self.draw_cast(canvas, CAST_MODE_BUTTON, loc(&self.frame, CH_MODE), false);
                }
                RebootLook::Map(n) => self.draw_cast(canvas, CAST_LEVEL_MAPS + n.min(6), loc(&self.frame, CH_MAP), false),
            }
            return;
        }

        // Control screen: node screen, door/elevator panel, scan interface
        // or probe controls.
        let screen_loc = loc(&self.frame, CH_SCREEN);
        if self.entering {
            // The Entering frame: an empty control screen (with the game-over
            // speech over it), and none of the map, buttons or indicators.
            self.draw_cast(canvas, CAST_EMPTY_SCREEN, screen_loc, false);
            if let Some((pixels, width)) = self.panel_frame() {
                let h = pixels.len() / 4 / width.max(1);
                canvas.blit(pixels, width, (0, 0, width, h), PANEL_AT.0, PANEL_AT.1, false);
            }
            self.draw_segues(canvas);
            return;
        }
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
        // Computer clips cover the control screen, indicators and all; the
        // ETA ship sits on its clip.
        if let Some((pixels, width)) = self.panel_frame() {
            let h = pixels.len() / 4 / width.max(1);
            canvas.blit(pixels, width, (0, 0, width, h), PANEL_AT.0, PANEL_AT.1, false);
        }
        if self.panel.is_none()
            && let Some(at) = self.game.computer_view().and_then(|v| v.eta)
        {
            self.draw_cast(canvas, CAST_ETA_SHIP, at, true);
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
        let probe_line = self.map_line.unwrap_or(nav.line());
        let probe_level = self.game.graph().at_line(probe_line).map_or(nav.level(), |n| n.level());
        // The marker as it slides; the node's point until it's placed.
        let marker = self
            .marker
            .filter(|(level, _)| *level == probe_level)
            .map(|(_, (x, y))| (x.round() as i32, y.round() as i32));
        let d = self.game.defender();
        let defender_level = d.level();
        let defender_marker = self
            .defender_marker
            .filter(|(level, _)| *level == defender_level)
            .map(|(_, (x, y))| (x.round() as i32, y.round() as i32))
            .or_else(|| self.defender_target().map(|(_, p)| p))
            .filter(|_| d.visible());
        let probe_item = if nav.in_ladder() { 6 } else { 2 };
        let (map_cast, probe, defender) = match self.map_mode {
            1 => (
                CAST_LEVEL_MAPS + probe_level as u16 - 1,
                marker.or(self.map_point(probe_line, probe_item)).map(|p| (CAST_PROBE_ICON + nav.facing().index() as u16, p)),
                (defender_level == probe_level).then(|| defender_marker.map(|p| (CAST_DEFENDER_ICON, p))).flatten(),
            ),
            2 => (
                CAST_LEVEL_MAPS + defender_level.max(1) as u16 - 1,
                (defender_level == probe_level)
                    .then(|| marker.or(self.map_point(probe_line, probe_item)).map(|p| (CAST_PROBE_ICON + nav.facing().index() as u16, p)))
                    .flatten(),
                defender_marker.map(|p| (CAST_DEFENDER_ICON, p)),
            ),
            _ => (
                CAST_LEVEL_MAPS + 6,
                self.map_point(probe_line, 4).map(|p| (CAST_PROBE_OVERVIEW, p)),
                self.map_point(d.line(), 4).filter(|_| d.visible()).map(|p| (CAST_DEFENDER_OVERVIEW, p)),
            ),
        };
        // The warning lamp under the view.
        let lamp_at = loc(&self.frame, CH_LAMP);
        match d.lamp() {
            Lamp::Off => {}
            Lamp::Ambient(n) => self.draw_cast(canvas, CAST_LAMP, (lamp_at.0 + n as i32 * LAMP_STEP, lamp_at.1), true),
            Lamp::One => self.draw_cast(canvas, CAST_LAMP + 1, lamp_at, true),
            Lamp::Two => self.draw_cast(canvas, CAST_LAMP + 2, lamp_at, true),
        }
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
        self.draw_segues(canvas);

        // Paused: "Game Paused" over the interface. The quit dialog sits on
        // the control screen.
        if self.paused {
            self.draw_cast(canvas, CAST_SEGUE_SCREEN, screen_loc, true);
            self.draw_cast(canvas, CAST_GAME_PAUSED, screen_loc, true);
        }
        if let Some(lit) = self.quit_dialog {
            self.draw_cast(canvas, CAST_QUIT_DIALOG, screen_loc, false);
            self.draw_cast(canvas, lit.unwrap_or(CAST_QUIT_DIALOG + 1), screen_loc, true);
        }
    }

    /// A segue (playing, or its last frame) and the minutes left over it.
    fn draw_segues(&self, canvas: &mut Canvas) {
        if let Some(movie) = &self.segue {
            self.draw_segue(canvas, movie.current(), movie.clip.width);
        } else if let Some((frame, width)) = &self.segue_still {
            self.draw_segue(canvas, Some(frame), *width);
        }
        if let Some(m) = self.minutes.filter(|_| self.segue_still.is_some() || self.segue.is_some()) {
            self.draw_cast(canvas, CAST_DIGITS + (m / 10 % 10) as u16, MINUTES_AT[0], false);
            self.draw_cast(canvas, CAST_DIGITS + (m % 10) as u16, MINUTES_AT[1], false);
        }
    }

    /// A segue over the whole interface (the score's Segue frame).
    fn draw_segue(&self, canvas: &mut Canvas, frame: Option<&[u8]>, w: usize) {
        self.draw_cast(canvas, CAST_SEGUE_SCREEN, loc(&self.frame, CH_SCREEN), true);
        if let Some(frame) = frame {
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

/// A tick's effects: the sounds that lead them (scan blips, warning beeps)
/// play at once; from the first other effect on, everything waits its turn
/// in order, so a sequence's sounds (a death's "Static", the next probe's
/// "Entering" music) play with their movies.
fn split_sounds(fx: Vec<Effect>) -> (Vec<String>, Vec<Effect>) {
    let lead = fx.iter().take_while(|e| matches!(e, Effect::Sound(_))).count();
    let mut fx = fx.into_iter();
    let now = fx.by_ref().take(lead).filter_map(|e| if let Effect::Sound(s) = e { Some(s) } else { None }).collect();
    (now, fx.collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_leading_sounds_play_at_once() {
        let blip = Effect::Sound("Blip".into());
        assert_eq!(split_sounds(vec![blip.clone(), blip.clone()]), (vec!["Blip".to_string(), "Blip".to_string()], vec![]));
        // A catch during a tick: its music waits for the movie before it.
        let death = vec![
            Effect::Sound("Warning One".into()),
            Effect::Video { folder: "Navigation/L1".into(), name: "1.02.N.1".into(), sound: None },
            Effect::Wait(2.0),
            Effect::Sound("Entering".into()),
            Effect::Segue { folder: "Opening".into(), name: "Entering Ship".into() },
        ];
        let (now, later) = split_sounds(death.clone());
        assert_eq!(now, ["Warning One"]);
        assert_eq!(later, death[1..]);
    }
}
