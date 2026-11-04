//! Iron Helix's rules, with no graphics, audio or timing of their own.
//!
//! The front end sends player actions in; the core updates its state and
//! returns [`Effect`]s (play this movie, this sound, wait) for the front end
//! to perform in order. Everything follows `docs/design.md`.
//!
//! So far: navigation (§4 of the design doc), clearance (§5), scanning
//! (§6), the probe arm (§7), computers (§8), the defender (§9), energy
//! (§10.1), losing probes (§10.3), the probe controls and the phase changes.

pub mod access;
pub mod arm;
pub mod computer;
pub mod defender;
pub mod game;
pub mod nav;
pub mod rng;
pub mod scan;

pub use access::Access;
pub use computer::{ComputerButton, ComputerData, ComputerView};
pub use defender::{DefenderData, Lamp};
pub use game::{Action, Button, ControlsPage, Function, Game, Interface};
pub use nav::{NavAction, NavButtons, Navigator, Panel};
pub use scan::{BoxLook, ScanText, ScanView};

/// Something the front end should do, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Play a movie in the view window, starting `sound` (an installed
    /// sound's name) with it. `folder` is relative to the game's files
    /// (`Navigation/L1`).
    Video { folder: String, name: String, sound: Option<String> },
    /// Play a short clip in the info strip (location labels, messages).
    InfoClip { folder: String, name: String },
    /// Play an installed sound.
    Sound(String),
    /// Stop the sound playing.
    StopSound,
    /// Show the probe on the map at this node line while the effects after
    /// it play (the floors passing during an elevator ride).
    MapLine(usize),
    /// Pause, in seconds (elevator rides).
    Wait(f32),
    /// Show the view window as it was before the scanner's movies.
    RestoreView,
    /// Play a movie on the bridge screen (the score's `BridgeBG` frame),
    /// then return to the game interface: the phase movies.
    Bridge { folder: String, name: String },
    /// Play a movie over the control screen (computer clips, video
    /// messages). With `keep`, its last frame stays until
    /// [`Effect::ErasePanel`] or the screen changes.
    Panel { folder: String, name: String, keep: bool },
    /// Clear a kept panel movie.
    ErasePanel,
    /// Play a 320x240 segue over the interface (disabling the defender, the
    /// end of the ship, losing a probe).
    Segue { folder: String, name: String },
    /// Show the minutes left on the segue screen (after losing a probe).
    Minutes(u32),
    /// Switch the map mode (2 = the defender's deck, when it comes back).
    MapMode(u8),
    /// Show (true) or leave the score's Entering frame: the interface with
    /// an empty control screen and no map, buttons or indicators, where a
    /// new probe enters and the game-over speech plays.
    Entering(bool),
    /// All probes lost (or the game won): back to the title.
    GameOver,
}

impl Effect {
    pub(crate) fn video(folder: impl Into<String>, name: impl Into<String>, sound: Option<&str>) -> Self {
        Effect::Video { folder: folder.into(), name: name.into(), sound: sound.map(str::to_string) }
    }
}
