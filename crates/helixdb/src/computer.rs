//! The ship's computer screens.
//!
//! The Cmp table is a list of records, each starting with a `Menu,<name>,...`
//! line. The computer script reads the record's lines relative to that line:
//!
//! ```text
//! +0  Menu,<name>,<back>,
//! +1  menu options: "Hier(008).02" = go to the record at line 8, ...
//! +2  authorization codes          +3  "win" (success) data
//! +4  movies to play               +5  variables
//! ```
//!
//! The non-menu lines are kept as raw text until their meaning is pinned
//! down from the computer scripts.

use crate::table::{Line, Table};

#[derive(Debug, Clone)]
pub struct MenuOption {
    /// Raw text, e.g. `Hier(008).02` or `XCmp`.
    pub raw: String,
    /// For `Hier(NNN).MM`: the target record's line and the trailing number.
    pub link: Option<(usize, u8)>,
}

#[derive(Debug, Clone)]
pub struct Screen {
    /// Line of the `Menu,` line; menu links point here.
    pub line: usize,
    pub name: String,
    pub back: Option<String>,
    pub options: Vec<Option<MenuOption>>,
    pub auth_codes: String,
    pub auth_win: String,
    pub movies: String,
    pub vars: String,
}

fn raw(table: &Table, n: usize) -> String {
    match table.line(n) {
        Some(Line::Row(f)) => f.iter().map(|x| x.as_deref().unwrap_or("#")).collect::<Vec<_>>().join(","),
        Some(Line::Comment(s)) | Some(Line::Header(s)) => s.clone(),
        None => String::new(),
    }
}

fn option(s: &str) -> MenuOption {
    let link = s.strip_prefix("Hier(").and_then(|rest| {
        let (line, tail) = rest.split_once(')')?;
        let n = tail.trim_start_matches('.').parse().unwrap_or(0);
        Some((line.parse().ok()?, n))
    });
    MenuOption { raw: s.to_string(), link }
}

pub fn parse(table: &Table) -> Vec<Screen> {
    table
        .rows()
        .filter(|(_, f)| f.first().and_then(|x| x.as_deref()) == Some("Menu"))
        .map(|(line, f)| Screen {
            line,
            name: f.get(1).cloned().flatten().unwrap_or_default(),
            back: f.get(2).cloned().flatten(),
            options: table
                .row(line + 1)
                .map(|o| o.iter().map(|x| x.as_deref().map(option)).collect())
                .unwrap_or_default(),
            auth_codes: raw(table, line + 2),
            auth_win: raw(table, line + 3),
            movies: raw(table, line + 4),
            vars: raw(table, line + 5),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_records() {
        let t = Table::parse("**Cmp\nMenu,Top,Return,\nHier(009).02,#,XCmp,\n#,\n#,\nA/B/,\n#,\n-- rec --\nMenu,Sub,Return,\n#,\n#,\n#,\n#,\n#,");
        let s = parse(&t);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "Top");
        assert_eq!(s[0].options[0].as_ref().unwrap().link, Some((9, 2)));
        assert_eq!(s[1].line, 9);
        assert_eq!(s[0].movies, "A/B/");
    }
}
