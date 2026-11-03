//! Iron Helix game data.
//!
//! The Mac release is a Director 3 game whose rules live in text tables
//! inside the projector's cast. This crate reads those tables from the
//! player's own copy of the game (nothing is bundled) and gives them shape:
//!
//! - [`nav`]: the probe's navigation graph (Node, Map, Ports and Scan tables)
//! - [`defender`]: the defender's search tree and line of sight (Def table)
//! - [`death`]: when the defender catches the probe (DeathDB table)
//! - [`computer`]: the ship's computer screens (Cmp table)
//! - [`table`]: the line-preserving parser underneath them all

pub mod computer;
pub mod death;
pub mod defender;
pub mod nav;
pub mod table;

use director::Movie;
use thiserror::Error;

pub use nav::{Direction, Move, NavGraph, NavNode};
pub use table::Table;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("the movie has no text member {0}")]
    Missing(String),
    #[error(transparent)]
    Director(#[from] director::DirectorError),
}

pub type Result<T> = std::result::Result<T, DataError>;

/// Cast numbers of the tables in the Mac release (the scripts refer to the
/// first five as F81-F85, Director 3's grid notation).
pub mod cast {
    pub const DEF: u16 = 377;
    pub const NODE: u16 = 378;
    pub const MAP: u16 = 379;
    pub const PORTS: u16 = 380;
    pub const CMP: u16 = 381;
}

/// Everything loaded from the game's Director movie.
pub struct GameData {
    pub nav: NavGraph,
    pub defender: Vec<defender::DefNode>,
    pub deaths: Vec<death::DeathRow>,
    pub screens: Vec<computer::Screen>,
    /// `<node>.<direction>` places where the probe can randomly fail.
    pub probe_failures: Vec<String>,
    /// Places the probe arm can't be used (`*`-separated node.direction list).
    pub no_arm: Vec<String>,
    /// The raw tables, for anything not modeled yet.
    pub tables: Tables,
}

pub struct Tables {
    pub def: Table,
    pub node: Table,
    pub map: Table,
    pub ports: Table,
    pub cmp: Table,
    pub death: Table,
    /// Scan tables for skill levels 1-5.
    pub scans: Vec<Table>,
}

impl GameData {
    pub fn from_movie(movie: &Movie) -> Result<Self> {
        let texts = movie.texts()?;
        let by_cast = |cast: u16| {
            texts
                .iter()
                .find(|m| m.cast == cast)
                .map(|m| m.text.clone())
                .ok_or_else(|| DataError::Missing(format!("#{cast}")))
        };
        let by_name = |name: &str| {
            texts
                .iter()
                .find(|m| m.name.as_deref() == Some(name))
                .map(|m| m.text.clone())
                .ok_or_else(|| DataError::Missing(format!("\"{name}\"")))
        };

        let tables = Tables {
            def: Table::parse(&by_cast(cast::DEF)?),
            node: Table::parse(&by_cast(cast::NODE)?),
            map: Table::parse(&by_cast(cast::MAP)?),
            ports: Table::parse(&by_cast(cast::PORTS)?),
            cmp: Table::parse(&by_cast(cast::CMP)?),
            death: Table::parse(&by_name("DeathDB")?),
            scans: (1..=5).map(|n| by_name(&format!("Scan {n}")).map(|t| Table::parse(&t))).collect::<Result<_>>()?,
        };

        let list = |text: String, sep: char| -> Vec<String> {
            text.split(sep).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
        };

        Ok(Self {
            nav: NavGraph::build(&tables.node, &tables.map, &tables.ports, &tables.scans),
            defender: defender::parse(&tables.def),
            deaths: death::parse(&tables.death),
            screens: computer::parse(&tables.cmp),
            probe_failures: list(by_name("ProbeFail")?, '/'),
            no_arm: list(by_name("NoArm")?, '*'),
            tables,
        })
    }
}
