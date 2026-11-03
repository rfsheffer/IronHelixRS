//! The defender's graph.
//!
//! The Def table is the defender's own map of the ship, arranged as a binary
//! search tree over node numbers so it can path toward a target node. All
//! jumps are relative line offsets within this table. Row layout, as the
//! defender script reads it:
//!
//! ```text
//! item 1   node id ("1.05"; chars 3-4 are the node number searched on)
//! item 2   jump to the parent
//! item 3   jump to the left child          item 5  highest node number on the left
//! item 4   jump to the right child         item 6  highest node number on the right
//! item 7   line of sight: "*1.01*1.02*3-1.20*" (nodes it can see the probe at;
//!          "3-1.20" = only when the probe faces direction 3)
//! item 8   the node on the other side of a door ("##" = none)
//! item 9   jump to the nearest ladder
//! item 10  jump used when the target is on a lower level
//! item 11  jump used to leave toward another level
//! item 12  jump out of a ladder
//! ```

use crate::nav::Direction;
use crate::table::{Table, item};

/// One entry of a line-of-sight list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sight {
    pub node: String,
    /// Only counts when the probe faces this way.
    pub facing: Option<Direction>,
}

#[derive(Debug, Clone)]
pub struct DefNode {
    pub line: usize,
    pub id: String,
    /// All fields as stored, for the ones not interpreted below.
    pub fields: Vec<Option<String>>,
    pub sight: Vec<Sight>,
}

fn int(s: Option<&str>) -> Option<i32> {
    s?.trim().parse().ok()
}

impl DefNode {
    pub fn parent(&self) -> Option<i32> {
        int(item(&self.fields, 2))
    }
    pub fn left(&self) -> Option<i32> {
        int(item(&self.fields, 3))
    }
    pub fn right(&self) -> Option<i32> {
        int(item(&self.fields, 4))
    }
    pub fn left_max(&self) -> Option<i32> {
        int(item(&self.fields, 5))
    }
    pub fn right_max(&self) -> Option<i32> {
        int(item(&self.fields, 6))
    }
    pub fn other_side_of_door(&self) -> Option<&str> {
        item(&self.fields, 8).filter(|s| *s != "##")
    }
    pub fn to_ladder(&self) -> Option<i32> {
        int(item(&self.fields, 9))
    }
    pub fn level_down(&self) -> Option<i32> {
        int(item(&self.fields, 10))
    }
    pub fn level_change(&self) -> Option<i32> {
        int(item(&self.fields, 11))
    }
    pub fn out_of_ladder(&self) -> Option<i32> {
        int(item(&self.fields, 12))
    }
}

/// "*1.01*1.02*3-1.20*" -> [1.01, 1.02, 1.20 (facing West)].
pub fn parse_sight(s: &str) -> Vec<Sight> {
    s.split('*')
        .filter(|p| !p.trim().is_empty())
        .map(|p| match p.split_once('-') {
            Some((d, node)) if d.len() == 1 => Sight {
                node: node.to_string(),
                facing: d.parse().ok().and_then(Direction::from_index),
            },
            _ => Sight { node: p.to_string(), facing: None },
        })
        .collect()
}

pub fn parse(table: &Table) -> Vec<DefNode> {
    table
        .rows()
        .filter_map(|(line, f)| {
            Some(DefNode {
                line,
                id: item(f, 1)?.to_string(),
                sight: item(f, 7).map(parse_sight).unwrap_or_default(),
                fields: f.to_vec(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sight_lists() {
        let s = parse_sight("*A.01*2-A.03*");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1], Sight { node: "A.03".into(), facing: Some(Direction::South) });
    }
}
