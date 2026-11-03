//! The probe's navigation graph.
//!
//! Four line-aligned tables describe every place the probe can stand: line
//! N of the Node, Map, Ports and Scan tables all describe the same node, and
//! moves are relative line offsets into them (the game keeps one "current
//! line" and adds the offset).
//!
//! Node table row:
//!
//! ```text
//! item 1      node id, "<level>.<node>" (ladder nodes contain "L", e.g. "2.1.2.L")
//! items 2-5   exit when facing N, E, S, W: a line offset ("0" = blocked),
//!             optionally ".<sound>" picking the ProbeMove sound ("7.2")
//! item 6      ladder up:   "D.OOO.F.L" or "0"
//! item 7      ladder down: "D.OOO.F.L" or "0"
//!             (D = facing that can climb, OOO = line offset, F = facing
//!             after the climb, L = destination level)
//! items 8-11  door when facing N, E, S, W ("#" = none)
//! items 12-13 elevator nodes ("<level>.<n>.E"): line offset to the elevator
//!             node one level up / down
//! ```
//!
//! Map row: item 1 node id, then pairs of map coordinates; the game picks a
//! pair by item number (2 for the level map, 4 and 6 for others).
//!
//! Ports row: item 1 node id, items 2-5 the probe arm's target point when
//! facing N, E, S, W, as "HHH.VVV".
//!
//! Scan row (one table per skill level): item 1 node id, then for each
//! direction two items; the first is a scan point "HHH.VVV.II.DD"
//! (screen position, item number, DNA group).
//!
//! Movement videos are named `<node>.<move>` in the level's navigation
//! folder: `F`/`B` plus the facing letter for forward/back (`1.01.FE`),
//! `L`/`R` plus the facing letter for turns, `U`/`D` for ladders.

use std::collections::BTreeMap;
use std::fmt;

use crate::table::{Table, item};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    North,
    East,
    South,
    West,
}

impl Direction {
    pub const ALL: [Direction; 4] = [Direction::North, Direction::East, Direction::South, Direction::West];

    pub fn from_index(i: u32) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// The letter used in video names.
    pub fn letter(self) -> char {
        ['N', 'E', 'S', 'W'][self.index()]
    }

    pub fn opposite(self) -> Self {
        Self::ALL[(self.index() + 2) % 4]
    }

    pub fn left(self) -> Self {
        Self::ALL[(self.index() + 3) % 4]
    }

    pub fn right(self) -> Self {
        Self::ALL[(self.index() + 1) % 4]
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.letter())
    }
}

/// A way out of a node: the line offset to the next node and which
/// movement sound to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    pub offset: i32,
    pub sound: u8,
}

impl Exit {
    /// "7.2" -> offset 7, sound 2; "-1" -> offset -1, sound 1; "0" -> none.
    pub fn parse(s: &str) -> Option<Self> {
        let (offset, sound) = match s.split_once('.') {
            Some((o, snd)) => (o.trim().parse().ok()?, snd.trim().chars().next()?.to_digit(10)? as u8),
            None => (s.trim().parse().ok()?, 1),
        };
        (offset != 0).then_some(Self { offset, sound })
    }
}

/// A ladder climb from a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder {
    /// The facing that offers the climb.
    pub from: Direction,
    pub offset: i32,
    /// The facing after the climb.
    pub facing: Direction,
    pub level: u8,
}

impl Ladder {
    /// "3.060.1.2" or "1.-10.3.1" (fixed width: chars 1, 3-5, 7, 9).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s == "0" {
            return None;
        }
        let c: Vec<char> = s.chars().collect();
        let digit = |i: usize| c.get(i)?.to_digit(10);
        let offset: String = c.get(2..5)?.iter().collect();
        Some(Self {
            from: Direction::from_index(digit(0)?)?,
            offset: offset.trim().parse().ok()?,
            facing: Direction::from_index(digit(6)?)?,
            level: digit(8).unwrap_or(0) as u8,
        })
    }
}

/// A scan point: where on screen the scanner hits, and what it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanPoint {
    pub h: u16,
    pub v: u16,
    pub item: u8,
    pub dna: u8,
}

impl ScanPoint {
    /// "080.054.37.12": chars 1-3 h, 5-7 v, 9-10 item, 12-13 DNA group.
    pub fn parse(s: &str) -> Option<Self> {
        Some(Self {
            h: fixed(s, 0, 3)?,
            v: fixed(s, 4, 7)?,
            item: fixed(s, 8, 10)?,
            dna: fixed(s, 11, 13).unwrap_or(0),
        })
    }
}

/// The number in a fixed-width slice of `s`.
fn fixed<T: std::str::FromStr>(s: &str, from: usize, to: usize) -> Option<T> {
    s.get(from..to)?.trim().parse().ok()
}

impl ScanPoint {
    pub fn position(&self) -> (u16, u16) {
        (self.h, self.v)
    }
}

/// A point on screen written "HHH.VVV".
fn parse_point(s: &str) -> Option<(u16, u16)> {
    let (h, v) = s.split_once('.')?;
    Some((h.trim().parse().ok()?, v.trim().parse().ok()?))
}

#[derive(Debug, Clone)]
pub struct NavNode {
    /// 1-based line in the Node table (the game's current-position index).
    pub line: usize,
    pub id: String,
    pub exits: [Option<Exit>; 4],
    pub up: Option<Ladder>,
    pub down: Option<Ladder>,
    pub doors: [Option<String>; 4],
    /// Doors locked by battle damage until a later phase (from the PC
    /// release's tables; see [`crate::pc`]).
    pub damaged: [bool; 4],
    /// Elevator nodes: line offsets to the elevator one level up / down.
    pub elevator_up: Option<i32>,
    pub elevator_down: Option<i32>,
    /// Map coordinates as stored (item 2 onward); `None` for `#`.
    pub map: Vec<Option<i32>>,
    /// Arm target per facing.
    pub ports: [Option<(u16, u16)>; 4],
    /// Scan points per facing, one entry per skill level table loaded.
    pub scan: Vec<[Option<ScanPoint>; 4]>,
}

impl NavNode {
    pub fn level(&self) -> u8 {
        self.id.chars().next().and_then(|c| c.to_digit(10)).unwrap_or(0) as u8
    }

    pub fn is_ladder(&self) -> bool {
        self.id.contains('L')
    }

    /// Elevator nodes end in ".E" and share one set of turn videos (node "0").
    pub fn is_elevator(&self) -> bool {
        self.id.contains('E')
    }

    pub fn exit(&self, facing: Direction) -> Option<Exit> {
        self.exits[facing.index()]
    }

    pub fn door(&self, facing: Direction) -> Option<&str> {
        self.doors[facing.index()].as_deref()
    }

    /// Name of the movie for a move from this node, e.g. `1.01.FE`.
    pub fn video(&self, mv: Move, facing: Direction) -> String {
        match mv {
            Move::Forward => format!("{}.F{}", self.id, facing),
            Move::Back => format!("{}.B{}", self.id, facing),
            Move::TurnLeft => format!("{}.L{}", self.id, facing),
            Move::TurnRight => format!("{}.R{}", self.id, facing),
            Move::Up => format!("{}.U", self.id),
            Move::Down => format!("{}.D", self.id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Forward,
    Back,
    TurnLeft,
    TurnRight,
    Up,
    Down,
}

#[derive(Debug, Clone, Default)]
pub struct NavGraph {
    nodes: BTreeMap<usize, NavNode>,
}

/// A problem found by [`NavGraph::check`].
#[derive(Debug, Clone)]
pub struct LinkProblem {
    pub from: String,
    pub what: String,
}

impl NavGraph {
    /// Build from the Node, Map and Ports tables and any number of Scan
    /// tables (in skill-level order).
    pub fn build(node: &Table, map: &Table, ports: &Table, scans: &[Table]) -> Self {
        let mut nodes = BTreeMap::new();
        for (line, f) in node.rows() {
            let Some(id) = item(f, 1) else { continue };
            let exits = std::array::from_fn(|d| item(f, d + 2).and_then(Exit::parse));
            let doors = std::array::from_fn(|d| item(f, d + 8).map(str::to_string));
            let map_coords = map
                .row(line)
                .map(|r| r.iter().skip(1).map(|c| c.as_deref().and_then(|s| s.trim().parse().ok())).collect())
                .unwrap_or_default();
            let port_row = ports.row(line);
            let ports = std::array::from_fn(|d| port_row.and_then(|r| item(r, d + 2)).and_then(parse_point));
            let scan = scans
                .iter()
                .map(|t| {
                    let r = t.row(line);
                    std::array::from_fn(|d| r.and_then(|r| item(r, d * 2 + 2)).and_then(ScanPoint::parse))
                })
                .collect();
            nodes.insert(
                line,
                NavNode {
                    line,
                    id: id.to_string(),
                    exits,
                    up: item(f, 6).and_then(Ladder::parse),
                    down: item(f, 7).and_then(Ladder::parse),
                    doors,
                    damaged: [false; 4],
                    elevator_up: item(f, 12).and_then(|s| s.trim().parse().ok()).filter(|&o| o != 0),
                    elevator_down: item(f, 13).and_then(|s| s.trim().parse().ok()).filter(|&o| o != 0),
                    map: map_coords,
                    ports,
                    scan,
                },
            );
        }
        Self { nodes }
    }

    pub fn nodes(&self) -> impl Iterator<Item = &NavNode> {
        self.nodes.values()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn at_line(&self, line: usize) -> Option<&NavNode> {
        self.nodes.get(&line)
    }

    pub fn find(&self, id: &str) -> Option<&NavNode> {
        self.nodes.values().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, line: usize) -> Option<&mut NavNode> {
        self.nodes.get_mut(&line)
    }

    pub fn insert(&mut self, node: NavNode) {
        self.nodes.insert(node.line, node);
    }

    /// The node reached by applying a line offset.
    pub fn step(&self, from: &NavNode, offset: i32) -> Option<&NavNode> {
        let line = from.line as i64 + offset as i64;
        usize::try_from(line).ok().and_then(|l| self.at_line(l))
    }

    /// Every exit and ladder must land on a node line.
    pub fn check(&self) -> Vec<LinkProblem> {
        let mut problems = Vec::new();
        for n in self.nodes.values() {
            for d in Direction::ALL {
                if let Some(e) = n.exit(d)
                    && self.step(n, e.offset).is_none()
                {
                    problems.push(LinkProblem {
                        from: n.id.clone(),
                        what: format!("exit {d} offset {} lands on line {}, not a node", e.offset, n.line as i64 + e.offset as i64),
                    });
                }
            }
            for (name, ladder) in [("up", n.up), ("down", n.down)] {
                if let Some(l) = ladder
                    && self.step(n, l.offset).is_none()
                {
                    problems.push(LinkProblem {
                        from: n.id.clone(),
                        what: format!("ladder {name} offset {} lands on a non-node line", l.offset),
                    });
                }
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_formats() {
        assert_eq!(Exit::parse("7.2"), Some(Exit { offset: 7, sound: 2 }));
        assert_eq!(Exit::parse("-1"), Some(Exit { offset: -1, sound: 1 }));
        assert_eq!(Exit::parse("0"), None);
        assert_eq!(
            Ladder::parse("1.-10.3.1"),
            Some(Ladder { from: Direction::East, offset: -10, facing: Direction::West, level: 1 })
        );
        assert_eq!(ScanPoint::parse("080.054.37.12"), Some(ScanPoint { h: 80, v: 54, item: 37, dna: 12 }));
    }

    /// A made-up three-node corridor (not game data).
    #[test]
    fn graph_links() {
        let node = Table::parse("** Node\nA.01,0,1,0,0,0,0,#,#,#,#,\n-- note --\nA.02,0,1,-2,0,0,0,#,7,#,#,\nA.03,0,0,-1.3,0,0,0,#,#,#,#,");
        let map = Table::parse("** Map\nA.01,1,2,\n\nA.02,3,4,\nA.03,5,6,");
        let empty = Table::parse("** Ports");
        let g = NavGraph::build(&node, &map, &empty, &[]);
        assert_eq!(g.len(), 3);
        let a1 = g.find("A.01").unwrap();
        // Offset 1 from line 2 lands on the comment line 3: a broken link.
        assert_eq!(g.check().len(), 1);
        let a2 = g.find("A.02").unwrap();
        assert_eq!(g.step(a2, -2).unwrap().id, "A.01");
        assert_eq!(a2.door(Direction::East), Some("7"));
        assert_eq!(g.find("A.03").unwrap().exit(Direction::South), Some(Exit { offset: -1, sound: 3 }));
        assert_eq!(a1.map, vec![Some(1), Some(2)]);
        assert_eq!(a1.video(Move::Forward, Direction::East), "A.01.FE");
    }
}
