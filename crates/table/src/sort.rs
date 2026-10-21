//! Sorting rows by one column. The sort is a permutation applied as one undo step; rows before
//! the body (the header) stay where they are, and empty cells sort last either way.

use std::cmp::Ordering;

use crate::document::Document;
use crate::edit::Edit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKind {
    /// Natural text order, ignoring case: `row2` before `row10`.
    Text,
    /// By numeric value; cells that are not numbers follow, in text order.
    Numeric,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub col: usize,
    pub ascending: bool,
    pub kind: SortKind,
}

/// Natural, case-insensitive comparison: digit runs compare by value, the rest by characters.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();
    loop {
        match (ia.peek().copied(), ib.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ia.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ia.next();
                }
                let mut nb = String::new();
                while let Some(c) = ib.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    ib.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| na.len().cmp(&nb.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(ca), Some(cb)) => {
                ia.next();
                ib.next();
                let (la, lb) = (ca.to_lowercase().collect::<String>(), cb.to_lowercase().collect::<String>());
                let ord = la.cmp(&lb);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

/// A cell's number for numeric sorting: plain decimals, with optional thousands commas and a
/// percent sign; anything else is not a number.
pub fn numeric_value(text: &str) -> Option<f64> {
    let s = text.trim().trim_end_matches('%');
    let s: String = s.chars().filter(|c| *c != ',').collect();
    if s.is_empty() || !s.bytes().next().is_some_and(|b| b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.') {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn compare(a: &str, b: &str, kind: SortKind) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    match kind {
        SortKind::Text => natural_cmp(a, b),
        SortKind::Numeric => match (numeric_value(a), numeric_value(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => natural_cmp(a, b),
        },
    }
}

/// The row order after sorting the rows from `body_start` on: `perm[i]` is the old index of the
/// row that ends up at `i`. Stable; empty cells last in both directions.
pub fn sort_permutation(table: &crate::table::Table, body_start: usize, spec: SortSpec) -> Vec<usize> {
    let rows = table.row_count();
    let body_start = body_start.min(rows);
    let mut body: Vec<usize> = (body_start..rows).collect();
    body.sort_by(|&a, &b| {
        let (ta, tb) = (table.get(a, spec.col), table.get(b, spec.col));
        // Empty cells stay last whatever the direction, so the direction flips only non-empty pairs.
        match (ta.is_empty(), tb.is_empty()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => {
                let ord = compare(ta, tb, spec.kind);
                if spec.ascending { ord } else { ord.reverse() }
            }
        }
    });
    (0..body_start).chain(body).collect()
}

impl Document {
    /// Sorts the body rows by one column as one undo step. `false` when the order did not change.
    pub fn sort(&mut self, body_start: usize, spec: SortSpec) -> bool {
        let perm = sort_permutation(&self.table, body_start, spec);
        if perm.iter().enumerate().all(|(i, &p)| i == p) {
            return false;
        }
        let label = format!("Sort {}", if spec.ascending { "ascending" } else { "descending" });
        self.apply(label, Edit::Permute { perm })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::ParseOptions;

    #[test]
    fn natural_order_handles_digits_case_and_zeros() {
        assert_eq!(natural_cmp("row2", "row10"), Ordering::Less);
        assert_eq!(natural_cmp("Row10", "row2"), Ordering::Greater);
        assert_eq!(natural_cmp("a", "B"), Ordering::Less);
        assert_eq!(natural_cmp("b", "B"), Ordering::Equal, "equal ignoring case: a stable sort keeps the file order");
        assert_eq!(natural_cmp("007", "7"), Ordering::Greater, "equal value: the longer form sorts after");
        assert_eq!(natural_cmp("x", "x1"), Ordering::Less);
        assert_eq!(natural_cmp("", ""), Ordering::Equal);
    }

    #[test]
    fn numeric_values() {
        assert_eq!(numeric_value(" 1,234.5 "), Some(1234.5));
        assert_eq!(numeric_value("12%"), Some(12.0));
        assert_eq!(numeric_value("-3e2"), Some(-300.0));
        assert_eq!(numeric_value("abc"), None);
        assert_eq!(numeric_value("inf"), None);
        assert_eq!(numeric_value(""), None);
    }

    #[test]
    fn sorting_keeps_the_header_puts_blanks_last_and_undoes() {
        let mut d = Document::from_bytes(b"name,n\nb,10\n,3\na,9\nc,\nB,100\n", &ParseOptions::default()).unwrap();
        assert!(d.sort(1, SortSpec { col: 0, ascending: true, kind: SortKind::Text }));
        let col0: Vec<&str> = d.table.rows().iter().map(|r| r[0].as_str()).collect();
        assert_eq!(col0, vec!["name", "a", "b", "B", "c", ""]);
        assert!(d.sort(1, SortSpec { col: 0, ascending: false, kind: SortKind::Text }));
        let col0: Vec<&str> = d.table.rows().iter().map(|r| r[0].as_str()).collect();
        assert_eq!(col0, vec!["name", "c", "b", "B", "a", ""], "blanks stay last when descending; equal keys keep their order");
        assert!(d.sort(1, SortSpec { col: 1, ascending: true, kind: SortKind::Numeric }));
        let col1: Vec<&str> = d.table.rows().iter().map(|r| r[1].as_str()).collect();
        assert_eq!(col1, vec!["n", "3", "9", "10", "100", ""]);
        assert!(!d.sort(1, SortSpec { col: 1, ascending: true, kind: SortKind::Numeric }), "already sorted");
        assert_eq!(d.undo(), Some("Sort ascending".to_string()));
        d.undo();
        d.undo();
        let col0: Vec<&str> = d.table.rows().iter().map(|r| r[0].as_str()).collect();
        assert_eq!(col0, vec!["name", "b", "", "a", "c", "B"]);
    }

    #[test]
    fn text_sort_is_natural() {
        let mut d = Document::from_bytes(b"f10\nf9\nf100\n", &ParseOptions { header: Some(false), ..Default::default() }).unwrap();
        assert!(d.sort(0, SortSpec { col: 0, ascending: true, kind: SortKind::Text }));
        let col0: Vec<&str> = d.table.rows().iter().map(|r| r[0].as_str()).collect();
        assert_eq!(col0, vec!["f9", "f10", "f100"]);
    }
}
