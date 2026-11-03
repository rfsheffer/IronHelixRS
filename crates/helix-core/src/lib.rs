//! Iron Helix's rules, with no graphics, audio or timing of their own.
//!
//! The front end sends player actions in; the core updates its state and
//! returns [`Effect`]s (play this movie, this sound, wait) for the front end
//! to perform in order. Everything follows `docs/design.md`.
//!
//! So far: navigation (§4 of the design doc).

pub mod nav;

pub use nav::{NavAction, NavButtons, Navigator, Panel};

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
    /// Pause, in seconds (elevator rides).
    Wait(f32),
}

impl Effect {
    pub(crate) fn video(folder: impl Into<String>, name: impl Into<String>, sound: Option<&str>) -> Self {
        Effect::Video { folder: folder.into(), name: name.into(), sound: sound.map(str::to_string) }
    }
}
