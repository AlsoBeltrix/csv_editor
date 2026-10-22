//! Find and replace. Every query compiles to a regex (literal queries are escaped), so one code
//! path handles case folding, whole-cell matching and group references.

use crate::document::Document;
use crate::edit::Edit;
use crate::selection::{CellRef, Range};
use crate::table::{Cell, Table};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub query: String,
    pub match_case: bool,
    /// The whole cell must match, not just part of it.
    pub whole_cell: bool,
    /// The query is a regular expression (`regex` crate syntax; `$1` in replacements).
    pub regex: bool,
}

#[derive(Clone, Debug)]
pub struct Matcher {
    re: regex::Regex,
    literal: bool,
}

impl Matcher {
    /// Compiles the options; an invalid regular expression is reported as text for the user.
    pub fn new(opts: &FindOptions) -> Result<Matcher, String> {
        if opts.query.is_empty() {
            return Err("Nothing to search for".to_string());
        }
        let body = if opts.regex { opts.query.clone() } else { regex::escape(&opts.query) };
        let body = if opts.whole_cell { format!("^(?:{body})$") } else { body };
        let pattern = if opts.match_case { body } else { format!("(?i){body}") };
        // Patterns are user input: cap the compiled size so a hostile one cannot eat memory.
        regex::RegexBuilder::new(&pattern).size_limit(1 << 22).build().map(|re| Matcher { re, literal: !opts.regex }).map_err(|e| e.to_string())
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.re.is_match(text)
    }

    /// The text with every match replaced, or `None` when nothing matched.
    pub fn replace(&self, text: &str, replacement: &str) -> Option<String> {
        if !self.re.is_match(text) {
            return None;
        }
        let out = if self.literal { self.re.replace_all(text, regex::NoExpand(replacement)) } else { self.re.replace_all(text, replacement) };
        Some(out.into_owned())
    }
}

/// Cells of `scope` in reading order, starting after `from` (or before it, backwards) and
/// wrapping around; `visible` says which rows take part (filtered-out rows are skipped).
fn walk(table: &Table, scope: Range, from: Option<CellRef>, forward: bool, visible: &dyn Fn(usize) -> bool) -> impl Iterator<Item = CellRef> {
    let last_row = scope.end.row.min(table.row_count().saturating_sub(1));
    let (r0, r1, c0, c1) = (scope.start.row, last_row, scope.start.col, scope.end.col);
    let cols = c1.saturating_sub(c0) + 1;
    let total = if r1 < r0 { 0 } else { (r1 - r0 + 1).saturating_mul(cols) };
    let index = |c: CellRef| (c.row.saturating_sub(r0)).saturating_mul(cols) + c.col.saturating_sub(c0);
    let start = match from {
        Some(f) if scope.contains(f) && f.row <= r1 => {
            let i = index(f);
            if forward { (i + 1) % total.max(1) } else { (i + total.saturating_sub(1)) % total.max(1) }
        }
        _ => {
            if forward {
                0
            } else {
                total.saturating_sub(1)
            }
        }
    };
    (0..total)
        .map(move |k| {
            let i = if forward { (start + k) % total } else { (start + total - k % total) % total };
            CellRef::new(r0 + i / cols, c0 + i % cols)
        })
        .filter(move |c| visible(c.row))
}

/// The next matching cell after `from` within `scope`, wrapping around; `None` without a match.
pub fn find_next(table: &Table, m: &Matcher, scope: Range, from: Option<CellRef>, forward: bool, visible: &dyn Fn(usize) -> bool) -> Option<CellRef> {
    walk(table, scope, from, forward, visible).find(|c| m.is_match(table.get(c.row, c.col)))
}

/// How many cells in `scope` match, stopping at `cap`.
pub fn count_matches(table: &Table, m: &Matcher, scope: Range, cap: usize, visible: &dyn Fn(usize) -> bool) -> usize {
    walk(table, scope, None, true, visible).filter(|c| m.is_match(table.get(c.row, c.col))).take(cap).count()
}

impl Document {
    /// Replaces the matches in one cell; `false` when it did not match.
    pub fn replace_in_cell(&mut self, m: &Matcher, replacement: &str, cell: CellRef) -> bool {
        match m.replace(self.table.get(cell.row, cell.col), replacement) {
            Some(text) => self.apply("Replace", Edit::set_cell(cell.row, cell.col, Cell::from(text))),
            None => false,
        }
    }

    /// Replaces every match in `scope` as one undo step. Returns the number of cells changed.
    pub fn replace_all(&mut self, m: &Matcher, replacement: &str, scope: Range, visible: &dyn Fn(usize) -> bool) -> usize {
        let mut changes = Vec::new();
        for c in walk(&self.table, scope, None, true, visible) {
            if let Some(text) = m.replace(self.table.get(c.row, c.col), replacement) {
                changes.push((c.row, c.col, Cell::from(text)));
            }
        }
        let n = changes.len();
        if n > 0 && self.apply("Replace all", Edit::SetCells(changes)) { n } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::ParseOptions;

    fn doc() -> Document {
        Document::from_bytes(b"name,note\nAnn,apple pie\nBob,Pineapple\nCy,none\n", &ParseOptions::default()).unwrap()
    }

    fn all(_: usize) -> bool {
        true
    }

    fn opts(query: &str) -> FindOptions {
        FindOptions { query: query.to_string(), ..FindOptions::default() }
    }

    #[test]
    fn matcher_modes() {
        let m = Matcher::new(&opts("apple")).unwrap();
        assert!(m.is_match("Pineapple") && m.is_match("apple pie"));
        let m = Matcher::new(&FindOptions { match_case: true, ..opts("apple") }).unwrap();
        assert!(!m.is_match("APPLE") && m.is_match("apple pie"));
        let m = Matcher::new(&FindOptions { whole_cell: true, ..opts("apple") }).unwrap();
        assert!(!m.is_match("apple pie") && m.is_match("Apple"));
        let m = Matcher::new(&FindOptions { regex: true, ..opts("^a(\\w+)") }).unwrap();
        assert_eq!(m.replace("apple pie", "<$1>"), Some("<pple> pie".to_string()));
        let m = Matcher::new(&opts("a.c")).unwrap();
        assert!(!m.is_match("abc"), "a literal query is not a regex");
        assert_eq!(m.replace("a.c", "$1"), Some("$1".to_string()), "literal replacements keep their dollars");
        assert!(Matcher::new(&FindOptions { regex: true, ..opts("(") }).is_err());
        assert!(Matcher::new(&opts("")).is_err());
    }

    #[test]
    fn find_next_walks_and_wraps() {
        let d = doc();
        let m = Matcher::new(&opts("apple")).unwrap();
        let scope = Range::new(CellRef::new(1, 0), CellRef::new(99, 1));
        let first = find_next(&d.table, &m, scope, None, true, &all).unwrap();
        assert_eq!(first, CellRef::new(1, 1));
        let second = find_next(&d.table, &m, scope, Some(first), true, &all).unwrap();
        assert_eq!(second, CellRef::new(2, 1));
        assert_eq!(find_next(&d.table, &m, scope, Some(second), true, &all), Some(first), "wraps around");
        assert_eq!(find_next(&d.table, &m, scope, Some(first), false, &all), Some(second), "backwards wraps too");
        assert_eq!(count_matches(&d.table, &m, scope, 100, &all), 2);
        assert_eq!(count_matches(&d.table, &m, scope, 1, &all), 1, "the cap stops the count");
        let only_row_2 = |r: usize| r == 2;
        assert_eq!(find_next(&d.table, &m, scope, None, true, &only_row_2), Some(second));
        assert_eq!(find_next(&d.table, &m, Range::cell(CellRef::new(0, 0)), None, true, &all), None);
    }

    #[test]
    fn replace_all_is_one_undo_step() {
        let mut d = doc();
        let m = Matcher::new(&opts("apple")).unwrap();
        let scope = Range::new(CellRef::new(1, 0), CellRef::new(99, 1));
        assert_eq!(d.replace_all(&m, "pear", scope, &all), 2);
        assert_eq!(d.table.get(1, 1), "pear pie");
        assert_eq!(d.table.get(2, 1), "Pinepear");
        assert_eq!(d.undo(), Some("Replace all".to_string()));
        assert_eq!(d.table.get(2, 1), "Pineapple");
        assert!(d.replace_in_cell(&m, "X", CellRef::new(1, 1)));
        assert!(!d.replace_in_cell(&m, "X", CellRef::new(3, 1)));
        assert_eq!(d.replace_all(&m, "x", Range::cell(CellRef::new(3, 0)), &all), 0);
    }
}
