//! The opening, the title screen and leaving (design doc §12.1).
//!
//! As `Game.lingo` runs them: the credits and title movie over the whole
//! stage, the opening movie on the bridge's monitor, then the title menu
//! over the bridge (score frame `BootUp`) with the theme playing: five
//! skill buttons and Begin New Game, Load Saved Game, Replay Intro, Quit
//! and About. About shows its movie until a key or click; quitting plays
//! the credits segue.

use std::rc::Rc;

use director::Picture;
use director::score::Frame;
use macroquad::input::KeyCode;

use crate::assets::GameAssets;
use crate::sound::Sounds;
use crate::sprites::{Canvas, Cast};
use crate::video::{Clip, Player};

/// Sprite channels of the title menu: skill levels 1-5, then the buttons.
const CH_SKILL: usize = 10;
const CH_BEGIN: usize = 15;
const CH_LOAD: usize = 16;
const CH_REPLAY: usize = 17;
const CH_QUIT: usize = 18;
const CH_ABOUT: usize = 19;
/// Where the opening plays on the bridge (score frame `BridgeBG`).
const OPENING_AT: (i32, i32) = (177, 66);
/// Where About and the credits play (frames `AboutScreen`, `QuitFrame`).
const SEGUE_AT: (i32, i32) = (88, 61);
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
    Credits(Player),
    /// The opening on the bridge's monitor.
    Opening(Player),
    Menu,
    /// About (its last frame stays up until a key or click).
    About(Player),
    /// The credits segue, then quit.
    Leaving(Player),
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
        })
    }

    pub fn sounds(&mut self) -> &mut Sounds {
        &mut self.sounds
    }

    /// The game's start: the credits and title, then the opening.
    pub async fn intro(&mut self, assets: &GameAssets) {
        match Clip::load(&assets.file("Opening", "OpenCreditsAndTitle")) {
            Ok(clip) => {
                self.start_audio(&clip).await;
                self.stage = Stage::Credits(Player::new(clip));
            }
            Err(_) => self.opening(assets).await,
        }
    }

    async fn opening(&mut self, assets: &GameAssets) {
        self.sounds.stop();
        match Clip::load(&assets.file("Opening", "OPENING")) {
            Ok(clip) => {
                self.start_audio(&clip).await;
                self.stage = Stage::Opening(Player::new(clip));
            }
            Err(_) => self.menu(assets).await,
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

    /// Leaving: the credits segue, then quit (Lingo's CleanUpAndQuit).
    pub async fn leave(&mut self, assets: &GameAssets) {
        self.sounds.stop();
        self.choice = None;
        match Clip::load(&assets.file("Segues", "Credits")) {
            Ok(clip) => {
                self.start_audio(&clip).await;
                self.stage = Stage::Leaving(Player::new(clip));
            }
            Err(_) => self.stage = Stage::Done,
        }
    }

    pub fn take_choice(&mut self) -> Option<Choice> {
        let choice = self.choice.take();
        if choice.is_none() && matches!(self.stage, Stage::Done) {
            return Some(Choice::Quit);
        }
        choice
    }

    async fn start_audio(&mut self, clip: &Clip) {
        if let Some((key, wav)) = &clip.audio {
            self.sounds.play_wav(key, wav).await;
        }
    }

    fn sprite_hit(&self, ch: usize, x: f32, y: f32) -> bool {
        self.menu
            .sprite(ch)
            .and_then(|s| Some((self.cast.get(s.cast)?, (s.loc_h as i32, s.loc_v as i32))))
            .is_some_and(|(img, at)| img.hit(at, x, y))
    }

    /// Skip a movie, close About, or press a menu button.
    pub fn press(&mut self, x: f32, y: f32) {
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

    /// Movies and About give way to any key or click.
    fn skip(&mut self) {
        match &mut self.stage {
            Stage::Credits(p) | Stage::Opening(p) | Stage::Leaving(p) => p.finish(),
            Stage::About(_) => self.stage = Stage::Menu,
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyCode) {
        if !matches!(self.stage, Stage::Menu) {
            // Esc while leaving quits at once.
            if key == KeyCode::Escape && matches!(self.stage, Stage::Leaving(_)) {
                self.stage = Stage::Done;
            }
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
        match &mut self.stage {
            Stage::Credits(p) => {
                if p.update(dt) {
                    self.opening(assets).await;
                }
            }
            Stage::Opening(p) => {
                if p.update(dt) {
                    self.menu(assets).await;
                }
            }
            Stage::About(p) => {
                p.update(dt);
            }
            Stage::Leaving(p) => {
                if p.update(dt) {
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
                None => self.sounds.play("SysBeeb").await,
            },
            CH_REPLAY => self.opening(assets).await,
            CH_QUIT => self.leave(assets).await,
            CH_ABOUT => match Clip::load(&assets.file("Segues", "About")) {
                Ok(clip) => {
                    self.start_audio(&clip).await;
                    self.stage = Stage::About(Player::new(clip));
                }
                Err(_) => self.sounds.play("SysBeeb").await,
            },
            _ => {}
        }
    }

    fn draw_bridge(&self, canvas: &mut Canvas) {
        if let Some(p) = &self.bridge {
            canvas.blit(&p.rgba, p.width, (0, 0, p.width, p.height), p.left, p.top, true);
        }
    }

    fn draw_movie(canvas: &mut Canvas, p: &Player, at: (i32, i32)) {
        if let Some(frame) = p.current() {
            let w = p.clip.width;
            let h = frame.len() / 4 / w.max(1);
            canvas.blit(frame, w, (0, 0, w, h), at.0, at.1, false);
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.fill(0, 0, canvas.width as i32, canvas.height as i32, [0, 0, 0]);
        match &self.stage {
            Stage::Credits(p) => Self::draw_movie(canvas, p, (0, 0)),
            Stage::Opening(p) => {
                self.draw_bridge(canvas);
                Self::draw_movie(canvas, p, OPENING_AT);
            }
            Stage::About(p) => Self::draw_movie(canvas, p, SEGUE_AT),
            Stage::Leaving(p) => Self::draw_movie(canvas, p, SEGUE_AT),
            Stage::Menu => {
                self.draw_bridge(canvas);
                for (i, s) in self.menu.sprites.iter().enumerate() {
                    let ch = i + 1;
                    if s.kind != 1 {
                        continue;
                    }
                    // The selected skill and a pressed button show lit (+1).
                    let lit = (ch == CH_SKILL + self.skill as usize - 1) || self.pressed.is_some_and(|(b, _)| b == ch);
                    if let Some(img) = self.cast.get(s.cast + lit as u16)
                        && img.width > 1
                    {
                        canvas.draw(img, (s.loc_h as i32, s.loc_v as i32), s.ink != 0);
                    }
                }
            }
            Stage::Done => {}
        }
    }
}
