//! When the defender catches the probe.
//!
//! The DeathDB table has rows of a node id followed by groups such as
//! `/1.03-1.02/D1.20/`: tokens between slashes, where `A-B` pairs two nodes
//! and `D<node>` names a door node. The game reads the row at a computed
//! line and tests each group in turn.

use crate::table::{Table, item};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// `A-B`: a pair of nodes.
    Pair(String, String),
    /// `D<node>`: a door node.
    Door(String),
    Other(String),
}

#[derive(Debug, Clone)]
pub struct DeathRow {
    pub line: usize,
    pub node: String,
    pub groups: Vec<Vec<Token>>,
}

fn token(s: &str) -> Token {
    if let Some(node) = s.strip_prefix('D') {
        Token::Door(node.to_string())
    } else if let Some((a, b)) = s.split_once('-') {
        Token::Pair(a.to_string(), b.to_string())
    } else {
        Token::Other(s.to_string())
    }
}

pub fn parse(table: &Table) -> Vec<DeathRow> {
    table
        .rows()
        .filter_map(|(line, f)| {
            let node = item(f, 1)?.to_string();
            let groups = f
                .iter()
                .skip(1)
                .flatten()
                .map(|g| g.split('/').filter(|t| !t.is_empty()).map(token).collect())
                .collect();
            Some(DeathRow { line, node, groups })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        let t = Table::parse("*Death\nA.01,/A.03-A.02/DA.04/,/B.01-A.02/,");
        let rows = parse(&t);
        assert_eq!(rows[0].groups.len(), 2);
        assert_eq!(rows[0].groups[0], vec![Token::Pair("A.03".into(), "A.02".into()), Token::Door("A.04".into())]);
    }
}
