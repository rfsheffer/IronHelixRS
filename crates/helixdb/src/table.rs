//! The game's text tables, line by line.
//!
//! The tables are plain text fields: a header line such as `** Node`, then
//! comma-separated rows, with the odd comment line (`--- Level Two ---`).
//! `#` marks an empty field. The game finds rows by *line number*, and many
//! tables store relative line offsets, so comment lines must be kept: every
//! [`Line`] here sits at the same index the game uses.

/// One line of a table. Line 1 is the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// The `*`-prefixed header line.
    Header(String),
    /// A line with no commas (separators, notes, blank lines).
    Comment(String),
    /// Comma-separated fields; `#` becomes `None`.
    Row(Vec<Option<String>>),
}

#[derive(Debug, Clone)]
pub struct Table {
    lines: Vec<Line>,
}

impl Table {
    pub fn parse(text: &str) -> Self {
        let lines = text
            .split('\n')
            .enumerate()
            .map(|(i, raw)| {
                let l = raw.trim_end_matches('\r');
                if i == 0 && l.trim_start().starts_with('*') {
                    Line::Header(l.trim().to_string())
                } else if l.contains(',') {
                    let mut fields: Vec<&str> = l.split(',').collect();
                    // Rows end with a trailing comma.
                    if fields.last().is_some_and(|f| f.trim().is_empty()) {
                        fields.pop();
                    }
                    Line::Row(
                        fields
                            .into_iter()
                            .map(|f| {
                                let f = f.trim();
                                (f != "#").then(|| f.to_string())
                            })
                            .collect(),
                    )
                } else {
                    Line::Comment(l.to_string())
                }
            })
            .collect();
        Self { lines }
    }

    /// The line at the game's 1-based line number.
    pub fn line(&self, number: usize) -> Option<&Line> {
        number.checked_sub(1).and_then(|i| self.lines.get(i))
    }

    /// The fields of a row at a 1-based line number.
    pub fn row(&self, number: usize) -> Option<&[Option<String>]> {
        match self.line(number)? {
            Line::Row(fields) => Some(fields),
            _ => None,
        }
    }

    /// `(line number, fields)` for every row.
    pub fn rows(&self) -> impl Iterator<Item = (usize, &[Option<String>])> {
        self.lines.iter().enumerate().filter_map(|(i, l)| match l {
            Line::Row(f) => Some((i + 1, f.as_slice())),
            _ => None,
        })
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Field `n` (1-based, as Lingo's `item n of`) as a string slice.
pub(crate) fn item(fields: &[Option<String>], n: usize) -> Option<&str> {
    fields.get(n.checked_sub(1)?)?.as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_line_numbers_and_empty_fields() {
        let t = Table::parse("** Test\n1.01,2,#,x,\n--- note ---\n1.02,3,");
        assert_eq!(t.len(), 4);
        assert!(matches!(t.line(1), Some(Line::Header(h)) if h == "** Test"));
        assert_eq!(t.row(2).unwrap(), &[Some("1.01".into()), Some("2".into()), None, Some("x".into())]);
        assert!(matches!(t.line(3), Some(Line::Comment(_))));
        assert_eq!(item(t.row(4).unwrap(), 2), Some("3"));
        assert_eq!(t.rows().count(), 2);
    }
}
