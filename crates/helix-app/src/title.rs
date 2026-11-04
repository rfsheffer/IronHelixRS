//! The opening, the title screen and leaving (design doc §12.1).
//!
//! As `Game.lingo` runs them: the credits and title movie over the whole
//! stage, the opening movie on the bridge's monitor, then the title menu
//! over the bridge (score frame `BootUp`) with the theme playing: five
//! skill buttons and Begin New Game, Load Saved Game, Replay Intro, Quit
//! and About. About shows its card; quitting plays the credits.
//!
//! As the PC release does: About and the credits play in a framed window
//! over the menu (its `SEGUE_A` frame; the Mac showed them on black), the
//! theme plays on under About and a refused Load beeps over it, and the
//! credits are the PC's scrolling `SEGUES\CREDITS` with `CREDTUNE.WAV`
//! looping (the Mac's are pages, silent; used without the PC files). The
//! movies stream, since the title's are large.

use std::rc::Rc;

use director::Picture;
use director::score::Frame;
use macroquad::input::KeyCode;

use crate::assets::GameAssets;
use crate::sound::Sounds;
use crate::sprites::{Canvas, Cast};
use crate::video::Stream;

/// The volume knob's sprite channel.
const CH_VOLUME: usize = 5;
/// Sprite channels of the title menu: skill levels 1-5, then the buttons.
const CH_SKILL: usize = 10;
const CH_BEGIN: usize = 15;
const CH_LOAD: usize = 16;
const CH_REPLAY: usize = 17;
const CH_QUIT: usize = 18;
const CH_ABOUT: usize = 19;
/// Where the opening plays on the bridge (score frame `BridgeBG`).
const OPENING_AT: (i32, i32) = (177, 66);
/// About and the credits (320x240): centered in the area the score's
/// `AboutScreen` and `QuitFrame` give them (88,61, 346x257).
const WINDOW_MOVIE_AT: (i32, i32) = (101, 69);
/// The window's frame around them, the size of the PC's `SEGUE_A`
/// (328x246): a bevel, light along the top and left, dark below and right.
const WINDOW: (i32, i32, i32, i32) = (97, 66, 328, 246);
const WINDOW_LIGHT: [u8; 3] = [0x77, 0x77, 0x77];
const WINDOW_MID: [u8; 3] = [0x66, 0x66, 0x66];
const WINDOW_DARK: [u8; 3] = [0x33, 0x33, 0x33];
/// A pressed menu button stays lit this long before acting (Delayof 15).
const PRESS_TIME: f32 = 0.25;

/// What the player picked.
pub enum Choice {
    Begin(u8),
    Load(String),
    Quit,
}

enum Stage {
    /// The credits and title, over the whole stage.
    Title(Stream),
    /// The opening on the bridge's monitor.
    Opening(Stream),
    Menu,
    /// About's card in the window, until a key or click.
    About(Stream),
    /// The credits in the window, then quit.
    Credits(Stream),
    Done,
}

pub struct TitleScreen {
    cast: Rc<Cast>,
    /// The bridge (cast 257, a PICT).
    bridge: Option<Picture>,
    /// The menu's frame (the one after `BootUp`, with the button scripts).
    menu: Frame,
    stage: Stage,
    skill: u8,
    /// A menu button lit after a press, and how long until it acts.
    pressed: Option<(usize, f32)>,
    sounds: Sounds,
    choice: Option<Choice>,
    /// About was closed: the menu on the next update.
    back_to_menu: bool,
    /// The volume slider's knob (on the menu, as in the game).
    slider: crate::volume::Slider,
    /// The knob was let go: the sound check plays.
    sound_check: bool,
}

impl TitleScreen {
    pub fn new(assets: &GameAssets) -> Result<Self, String> {
        let score = assets.movie.score().map_err(|e| e.to_string())?;
        let boot = assets.movie.label_frame("BootUp").map_err(|e| e.to_string())?.ok_or("no \"BootUp\" frame in the score")?;
        let menu = score.get(boot as usize).cloned().ok_or("score is shorter than expected")?;
        let bridge = menu.sprites.iter().find_map(|s| assets.movie.picture(s.cast).ok().flatten());
        Ok(Self {
            cast: assets.cast.clone(),
            bridge,
            menu,
            stage: Stage::Done,
            skill: assets.skill,
            pressed: None,
            sounds: Sounds::new(assets.mac_sounds.clone(), assets.cast_sounds.clone()),
            choice: None,
            back_to_menu: false,
            slider: Default::default(),
            sound_check: false,
        })
    }

    pub fn sounds(&mut self) -> &mut Sounds {
        &mut self.sounds
    }

    /// Load a movie and start its sound track.
    async fn start(&mut self, assets: &GameAssets, folder: &str, name: &str) -> Option<Stream> {
        let movie = Stream::load(&assets.file(folder, name)).ok()?;
        if let Some((key, wav)) = &movie.audio {
            self.sounds.play_wav(key, wav).await;
        }
        Some(movie)
    }

    /// The game's start: the credits and title, then the opening.
    pub async fn intro(&mut self, assets: &GameAssets) {
        match self.start(assets, "Opening", "OpenCreditsAndTitle").await {
            Some(movie) => self.stage = Stage::Title(movie),
            None => self.opening(assets).await,
        }
    }

    async fn opening(&mut self, assets: &GameAssets) {
        self.sounds.stop();
        match self.start(assets, "Opening", "OPENING").await {
            Some(movie) => self.stage = Stage::Opening(movie),
            None => self.menu(assets).await,
        }
    }

    /// The title menu, with the theme (back from a game, too).
    pub async fn menu(&mut self, assets: &GameAssets) {
        self.stage = Stage::Menu;
        self.pressed = None;
        self.choice = None;
        if let Some(wav) = &assets.theme {
            self.sounds.play_wav("theme", wav).await;
        }
    }

    /// Leaving: the credits, then quit (Lingo's CleanUpAndQuit). The PC's
    /// scrolling credits with their tune looping, else the Mac's.
    pub async fn leave(&mut self, assets: &GameAssets) {
        self.sounds.stop();
        self.choice = None;
        let pc = assets.files.pc_file("SEGUES", "CREDITS").and_then(|p| Stream::load(&p).ok());
        let movie = match pc {
            Some(movie) => {
                if let Some(wav) = assets.files.pc_file("OPENING", "CREDTUNE.WAV").and_then(|p| std::fs::read(p).ok()) {
                    self.sounds.loop_wav("CREDTUNE.WAV", &wav).await;
                }
                Some(movie)
            }
            None => self.start(assets, "Segues", "Credits").await,
        };
        self.stage = movie.map_or(Stage::Done, Stage::Credits);
    }

    /// About's card; the theme plays on.
    async fn about(&mut self, assets: &GameAssets) {
        match self.start(assets, "Segues", "About").await {
            Some(card) => self.stage = Stage::About(card),
            None => self.sounds.play_over("SysBeeb").await,
        }
    }

    pub fn take_choice(&mut self) -> Option<Choice> {
        let choice = self.choice.take();
        if choice.is_none() && matches!(self.stage, Stage::Done) {
            return Some(Choice::Quit);
        }
        choice
    }

    fn sprite_hit(&self, ch: usize, x: f32, y: f32) -> bool {
        self.menu
            .sprite(ch)
            .and_then(|s| Some((self.cast.get(s.cast)?, (s.loc_h as i32, s.loc_v as i32))))
            .is_some_and(|(img, at)| img.hit(at, x, y))
    }

    /// Skip a movie, or press a menu button (or the volume knob).
    pub fn press(&mut self, x: f32, y: f32) {
        if matches!(self.stage, Stage::Menu) && self.slider.press(&self.cast, x, y) {
            return;
        }
        match self.stage {
            Stage::Menu if self.pressed.is_none() => {
                if let Some(level) = (0..5).find(|&i| self.sprite_hit(CH_SKILL + i, x, y)) {
                    self.skill = level as u8 + 1;
                } else if let Some(button) = (CH_BEGIN..=CH_ABOUT).find(|&ch| self.sprite_hit(ch, x, y)) {
                    self.pressed = Some((button, PRESS_TIME));
                }
            }
            _ => self.skip(),
        }
    }

    /// The pointer moving while down: the volume knob.
    pub fn drag(&mut self, x: f32) {
        if self.slider.drag(x) {
            self.sounds.volume_changed();
        }
    }

    pub fn release(&mut self) {
        if self.slider.release() {
            self.sound_check = true;
        }
    }

    /// A key or click: movies end; About closes.
    fn skip(&mut self) {
        match &mut self.stage {
            Stage::Title(m) | Stage::Opening(m) => m.finish(),
            // Quitting quits at once.
            Stage::Credits(_) => self.stage = Stage::Done,
            Stage::About(_) => self.back_to_menu = true,
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyCode) {
        if !matches!(self.stage, Stage::Menu) {
            self.skip();
            return;
        }
        if self.pressed.is_some() {
            return;
        }
        let button = match key {
            KeyCode::Key1 | KeyCode::Key2 | KeyCode::Key3 | KeyCode::Key4 | KeyCode::Key5 => {
                self.skill = (key as u16 - KeyCode::Key1 as u16 + 1) as u8;
                return;
            }
            KeyCode::Enter | KeyCode::KpEnter | KeyCode::B => CH_BEGIN,
            KeyCode::L => CH_LOAD,
            KeyCode::R => CH_REPLAY,
            KeyCode::A => CH_ABOUT,
            KeyCode::Escape | KeyCode::Q => CH_QUIT,
            _ => return,
        };
        self.pressed = Some((button, PRESS_TIME));
    }

    pub async fn update(&mut self, assets: &GameAssets, dt: f32) {
        if std::mem::take(&mut self.sound_check) {
            self.sounds.play_over("SoundCheck").await;
        }
        // Back from About: the theme was playing all along.
        if std::mem::take(&mut self.back_to_menu) {
            self.stage = Stage::Menu;
            return;
        }
        match &mut self.stage {
            Stage::Title(m) => {
                if m.update(dt) {
                    self.opening(assets).await;
                }
            }
            Stage::Opening(m) => {
                if m.update(dt) {
                    self.menu(assets).await;
                }
            }
            Stage::About(card) => {
                card.update(dt);
            }
            Stage::Credits(movie) => {
                if movie.update(dt) {
                    self.stage = Stage::Done;
                }
            }
            Stage::Menu => {
                if let Some((button, t)) = &mut self.pressed {
                    *t -= dt;
                    if *t <= 0.0 {
                        let button = *button;
                        self.pressed = None;
                        self.act(assets, button).await;
                    }
                }
            }
            Stage::Done => {}
        }
    }

    async fn act(&mut self, assets: &GameAssets, button: usize) {
        match button {
            CH_BEGIN => {
                self.sounds.stop();
                self.choice = Some(Choice::Begin(self.skill));
            }
            CH_LOAD => match assets.saved_game() {
                Some(text) => {
                    self.sounds.stop();
                    self.choice = Some(Choice::Load(text));
                }
                // No saved game: a beep over the music.
                None => self.sounds.play_over("SysBeeb").await,
            },
            CH_REPLAY => self.opening(assets).await,
            CH_QUIT => self.leave(assets).await,
            CH_ABOUT => self.about(assets).await,
            _ => {}
        }
    }

    fn draw_bridge(&self, canvas: &mut Canvas) {
        if let Some(p) = &self.bridge {
            canvas.blit(&p.rgba, p.width, (0, 0, p.width, p.height), p.left, p.top, true);
        }
    }

    fn draw_movie(canvas: &mut Canvas, m: &Stream, at: (i32, i32)) {
        if let Some(frame) = m.current() {
            let w = m.width();
            let h = frame.len() / 4 / w.max(1);
            canvas.blit(frame, w, (0, 0, w, h), at.0, at.1, false);
        }
    }

    /// The menu over the bridge (with the selected skill and a pressed
    /// button lit).
    fn draw_menu(&self, canvas: &mut Canvas) {
        self.draw_bridge(canvas);
        for (i, s) in self.menu.sprites.iter().enumerate() {
            let ch = i + 1;
            if s.kind != 1 {
                continue;
            }
            if ch == CH_VOLUME {
                crate::volume::Slider::draw(&self.cast, canvas);
                continue;
            }
            let lit = (ch == CH_SKILL + self.skill as usize - 1) || self.pressed.is_some_and(|(b, _)| b == ch);
            if let Some(img) = self.cast.get(s.cast + lit as u16)
                && img.width > 1
            {
                canvas.draw(img, (s.loc_h as i32, s.loc_v as i32), s.ink != 0);
            }
        }
    }

    /// About or the credits in their window over the menu.
    fn draw_window(&self, canvas: &mut Canvas, m: &Stream) {
        self.draw_menu(canvas);
        let (x, y, w, h) = WINDOW;
        canvas.fill(x, y, x + w, y + h, [0, 0, 0]);
        canvas.fill(x, y, x + w, y + 1, WINDOW_LIGHT);
        canvas.fill(x, y + 1, x + w, y + 2, WINDOW_MID);
        canvas.fill(x, y, x + 2, y + h, WINDOW_MID);
        canvas.fill(x, y + h - 1, x + w, y + h, WINDOW_DARK);
        canvas.fill(x + w - 1, y, x + w, y + h, WINDOW_DARK);
        Self::draw_movie(canvas, m, WINDOW_MOVIE_AT);
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.fill(0, 0, canvas.width as i32, canvas.height as i32, [0, 0, 0]);
        match &self.stage {
            Stage::Title(m) => Self::draw_movie(canvas, m, (0, 0)),
            Stage::Opening(m) => {
                self.draw_bridge(canvas);
                Self::draw_movie(canvas, m, OPENING_AT);
            }
            Stage::About(m) | Stage::Credits(m) => self.draw_window(canvas, m),
            Stage::Menu => self.draw_menu(canvas),
            Stage::Done => {}
        }
    }
}
