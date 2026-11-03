//! The PC release's node tables, for the rooms only it has.
//!
//! The Windows version is a native 16-bit program (not Director) with its
//! tables compiled into `HELIX.EXE`. Two door pairs that are permanently
//! locked (`00`) on the Mac open there, onto four extra nodes whose movies
//! only the PC disc has. This module reads the PC node records and adds those
//! rooms to the Mac's navigation graph (design doc §14.4).
//!
//! The data segment holds one array of 64-byte node records per deck, with
//! node `n` at index `n` (index 0 unused):
//!
//! ```text
//! bytes 0-7    exits N, E, S, W: i16 node number on this deck (0 none, -1 sealed)
//! bytes 12-15  door type per facing: 0x02 door, 0x01 elevator door,
//!              0x04 garbage chute, 0x22 sealed, 0xC4 ladder marker
//! bytes 32-35  door clearance code per facing; bit 0x20 = locked by battle
//!              damage until a later phase
//! bytes 44-47  deck-map point (i16 x, y) in the PC's map art
//! ```
//!
//! Overview-map points are a separate table of (x, y) pairs per deck, which
//! equal the Mac's overview points minus (293, 185).

use std::collections::{BTreeMap, VecDeque};

use crate::nav::{Direction, Exit, NavGraph, NavNode};
use crate::{DataError, Result};

/// Data-segment offsets of each deck's node records (decks 1-6) and how many
/// nodes it has.
const DECKS: [(usize, usize); 6] = [(0x1E46, 23), (0x2446, 43), (0x2F46, 5), (0x30C6, 9), (0x3346, 16), (0x3786, 10)];
/// Data-segment offsets of each deck's overview points (first node first).
const OVERVIEW: [usize; 6] = [0x3A86, 0x3AE6, 0x3B96, 0x3BAE, 0x3BDA, 0x3C1E];
const RECORD: usize = 64;

pub const DOOR: u8 = 0x02;
pub const SEALED: u8 = 0x22;
/// Clearance-code flag: locked by battle damage until a later phase.
pub const DAMAGED: u8 = 0x20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcNode {
    pub exits: [i16; 4],
    pub door_types: [u8; 4],
    pub door_codes: [u8; 4],
    pub map: (i32, i32),
    pub overview: (i32, i32),
}

#[derive(Debug, Clone)]
pub struct PcShip {
    /// `decks[d - 1][n - 1]` is node `d.nn`.
    decks: Vec<Vec<PcNode>>,
}

impl PcShip {
    /// Read the tables from the PC's `HELIX.EXE`.
    pub fn parse(exe: &[u8]) -> Result<Self> {
        let ds = data_segment(exe).ok_or_else(|| DataError::Pc("not the PC release's HELIX.EXE".into()))?;
        let i16_at = |o: usize| -> Option<i16> { Some(i16::from_le_bytes(ds.get(o..o + 2)?.try_into().ok()?)) };
        let truncated = || DataError::Pc("HELIX.EXE's data segment is too short".into());

        let mut decks = Vec::new();
        for (deck, &(start, count)) in DECKS.iter().enumerate() {
            let mut nodes = Vec::new();
            for n in 1..=count {
                let r = ds.get(start + RECORD * n..start + RECORD * (n + 1)).ok_or_else(truncated)?;
                let ov = OVERVIEW[deck] + 4 * (n - 1);
                let node = PcNode {
                    exits: std::array::from_fn(|d| i16::from_le_bytes([r[2 * d], r[2 * d + 1]])),
                    door_types: r[12..16].try_into().unwrap(),
                    door_codes: r[32..36].try_into().unwrap(),
                    map: (i16::from_le_bytes([r[44], r[45]]) as i32, i16::from_le_bytes([r[46], r[47]]) as i32),
                    overview: (i16_at(ov).ok_or_else(truncated)? as i32, i16_at(ov + 2).ok_or_else(truncated)? as i32),
                };
                if node.exits.iter().any(|&e| e < -1 || e > count as i16) {
                    return Err(DataError::Pc(format!("node {}.{n:02} has an exit off its deck; unknown HELIX.EXE version", deck + 1)));
                }
                nodes.push(node);
            }
            decks.push(nodes);
        }
        Ok(Self { decks })
    }

    pub fn node(&self, deck: u8, n: u8) -> Option<&PcNode> {
        self.decks.get((deck as usize).checked_sub(1)?)?.get((n as usize).checked_sub(1)?)
    }
}

/// The automatic data segment of a 16-bit Windows (NE) program.
fn data_segment(exe: &[u8]) -> Option<&[u8]> {
    let u16_at = |o: usize| -> Option<u16> { Some(u16::from_le_bytes(exe.get(o..o + 2)?.try_into().ok()?)) };
    let ne = u32::from_le_bytes(exe.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if exe.get(ne..ne + 2)? != b"NE" {
        return None;
    }
    let auto_data = u16_at(ne + 0x0E)? as usize;
    let segments = ne + u16_at(ne + 0x22)? as usize;
    let shift = u16_at(ne + 0x32)?;
    let entry = segments + 8 * auto_data.checked_sub(1)?;
    let offset = (u16_at(entry)? as usize) << shift;
    let len = match u16_at(entry + 2)? {
        0 => 0x10000,
        n => n as usize,
    };
    exe.get(offset..offset + len)
}

/// `"2.08"` -> (2, 8); ladder, elevator and other special ids don't match.
fn plain_id(id: &str) -> Option<(u8, u8)> {
    let (deck, n) = id.split_once('.')?;
    (deck.len() == 1 && n.len() == 2).then_some(())?;
    Some((deck.parse().ok()?, n.parse().ok()?))
}

fn id(deck: u8, n: u8) -> String {
    format!("{deck}.{n:02}")
}

/// The door as the Mac tables write it, and whether it's battle-damaged.
fn door(node: &PcNode, d: usize) -> (Option<String>, bool) {
    match node.door_types[d] {
        DOOR => (Some(format!("{:02}", node.door_codes[d] & !DAMAGED)), node.door_codes[d] & DAMAGED != 0),
        SEALED => (Some("00".into()), false),
        _ => (None, false),
    }
}

/// Add the PC's hidden rooms: behind every Mac `00` door that the PC lets
/// open onto a node the Mac doesn't have, and everything beyond. New nodes
/// get lines after the last table line, so the line-aligned tables (Def,
/// DeathDB, Scan) have nothing for them. Returns the ids added.
pub fn add_hidden_rooms(graph: &mut NavGraph, pc: &PcShip) -> Vec<String> {
    let mut lines: BTreeMap<String, usize> = graph.nodes().map(|n| (n.id.clone(), n.line)).collect();
    let mac_ids: Vec<String> = lines.keys().cloned().collect();
    let mut next_line = graph.nodes().map(|n| n.line).max().unwrap_or(0) + 1;
    // New node -> (line, the placed neighbor its map points are relative to).
    let mut added: Vec<(String, usize, String)> = Vec::new();
    let mut queue = VecDeque::new();
    let mut entrances = Vec::new();

    for n in graph.nodes() {
        let Some((deck, num)) = plain_id(&n.id) else { continue };
        let Some(p) = pc.node(deck, num) else { continue };
        for d in Direction::ALL {
            let i = d.index();
            if n.door(d) != Some("00") || p.door_types[i] != DOOR || p.exits[i] <= 0 {
                continue;
            }
            let target = id(deck, p.exits[i] as u8);
            if mac_ids.contains(&target) {
                continue;
            }
            entrances.push((n.line, d, target.clone()));
            if !lines.contains_key(&target) {
                lines.insert(target.clone(), next_line);
                added.push((target.clone(), next_line, n.id.clone()));
                queue.push_back(target);
                next_line += 1;
            }
        }
    }
    while let Some(new) = queue.pop_front() {
        let (deck, num) = plain_id(&new).unwrap();
        for &e in &pc.node(deck, num).unwrap().exits {
            if e <= 0 {
                continue;
            }
            let target = id(deck, e as u8);
            if !lines.contains_key(&target) {
                lines.insert(target.clone(), next_line);
                added.push((target.clone(), next_line, new.clone()));
                queue.push_back(target);
                next_line += 1;
            }
        }
    }

    // Map points: the anchor's Mac point plus the PC's offset between them.
    let mut maps: BTreeMap<String, Vec<Option<i32>>> = graph.nodes().map(|n| (n.id.clone(), n.map.clone())).collect();
    for (new, line, anchor) in &added {
        let (deck, num) = plain_id(new).unwrap();
        let p = pc.node(deck, num).unwrap();
        let a = plain_id(anchor).and_then(|(d, n)| pc.node(d, n)).unwrap();
        let base = &maps[anchor];
        let at = |i: usize| base.get(i).copied().flatten();
        let map = match (at(0), at(1), at(2), at(3)) {
            (Some(x), Some(y), Some(ox), Some(oy)) => vec![
                Some(x + p.map.0 - a.map.0),
                Some(y + p.map.1 - a.map.1),
                Some(ox + p.overview.0 - a.overview.0),
                Some(oy + p.overview.1 - a.overview.1),
            ],
            _ => Vec::new(),
        };
        maps.insert(new.clone(), map.clone());

        let exits = std::array::from_fn(|d| {
            let e = p.exits[d];
            (e > 0).then(|| lines.get(&id(deck, e as u8))).flatten().map(|&to| Exit { offset: to as i32 - *line as i32, sound: 1 })
        });
        let doors: [(Option<String>, bool); 4] = std::array::from_fn(|d| door(p, d));
        graph.insert(NavNode {
            line: *line,
            id: new.clone(),
            exits,
            up: None,
            down: None,
            doors: doors.clone().map(|(code, _)| code),
            damaged: doors.map(|(_, damaged)| damaged),
            elevator_up: None,
            elevator_down: None,
            map,
            ports: [None; 4],
            scan: Vec::new(),
        });
    }

    for (line, d, target) in entrances {
        let to = lines[&target];
        let node = graph.node_mut(line).unwrap();
        let (deck, num) = plain_id(&node.id).unwrap();
        let (code, damaged) = door(pc.node(deck, num).unwrap(), d.index());
        node.exits[d.index()] = Some(Exit { offset: to as i32 - line as i32, sound: 1 });
        node.doors[d.index()] = code;
        node.damaged[d.index()] = damaged;
    }
    added.into_iter().map(|(id, _, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;

    fn pc_node(exits: [i16; 4], door_types: [u8; 4], door_codes: [u8; 4], map: (i32, i32)) -> PcNode {
        PcNode { exits, door_types, door_codes, map, overview: map }
    }

    /// Made-up decks (not game data): Mac nodes 1.01-1.02 with a sealed door
    /// south of 1.02; the PC opens it (damaged) onto 1.03, which leads to 1.04.
    #[test]
    fn hidden_rooms_are_appended() {
        let node = Table::parse("** Node\n1.01,1,0,0,0,0,0,#,#,#,#,\n1.02,0,0,-1,0,0,0,#,#,00,#,");
        let map = Table::parse("** Map\n1.01,10,20,30,40,\n1.02,10,10,30,35,");
        let empty = Table::parse("** Ports");
        let mut g = NavGraph::build(&node, &map, &empty, &[]);
        let pc = PcShip {
            decks: vec![vec![
                pc_node([2, 0, 0, 0], [0; 4], [0; 4], (0, 10)),
                pc_node([0, 0, 3, 1], [0, 0, DOOR, 0], [0, 0, DAMAGED | 6, 0], (0, 0)),
                pc_node([2, 0, 4, 0], [DOOR, 0, 0, 0], [6, 0, 0, 0], (0, -5)),
                pc_node([3, 0, 0, 0], [0; 4], [0; 4], (2, -9)),
            ]],
        };
        assert_eq!(add_hidden_rooms(&mut g, &pc), ["1.03", "1.04"]);
        assert!(g.check().is_empty());

        let entrance = g.find("1.02").unwrap();
        assert_eq!(entrance.door(Direction::South), Some("06"));
        assert!(entrance.damaged[Direction::South.index()]);
        let room = g.step(entrance, entrance.exit(Direction::South).unwrap().offset).unwrap().clone();
        assert_eq!(room.id, "1.03");
        assert_eq!(room.door(Direction::North), Some("06"));
        assert!(!room.damaged[Direction::North.index()]);
        assert_eq!(room.map, vec![Some(10), Some(5), Some(30), Some(30)]);
        let far = g.step(&room, room.exit(Direction::South).unwrap().offset).unwrap();
        assert_eq!((far.id.as_str(), far.map[0], far.map[1]), ("1.04", Some(12), Some(1)));
        assert_eq!(g.step(far, far.exit(Direction::North).unwrap().offset).unwrap().id, "1.03");
    }

    #[test]
    fn rejects_other_files() {
        assert!(PcShip::parse(b"MZ not an NE program").is_err());
    }
}
