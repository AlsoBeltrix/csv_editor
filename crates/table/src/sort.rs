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
/// Allocation-free: a sort calls this tens of millions of times.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut i, mut j) = (0usize, 0usize);
    loop {
        let (ra, rb) = (a.get(i..).unwrap_or(""), b.get(j..).unwrap_or(""));
        match (ra.chars().next(), rb.chars().next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let (da, db) = (digit_run(ra), digit_run(rb));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| da.len().cmp(&db.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
                i += da.len();
                j += db.len();
            }
            (Some(ca), Some(cb)) => {
                i += ca.len_utf8();
                j += cb.len_utf8();
                if ca != cb {
                    let ord = ca.to_lowercase().cmp(cb.to_lowercase());
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
            }
        }
    }
}

/// The leading run of ASCII digits.
fn digit_run(s: &str) -> &str {
    let end = s.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(s.len());
    s.get(..end).unwrap_or("")
}

/// A cell's number for numeric sorting: plain decimals, with optional thousands commas and a
/// percent sign; anything else is not a number.
pub fn numeric_value(text: &str) -> Option<f64> {
    let s = text.trim().trim_end_matches('%');
    if !s.bytes().next().is_some_and(|b| b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.') {
        return None;
    }
    let parsed = if s.contains(',') { s.replace(',', "").parse::<f64>() } else { s.parse::<f64>() };
    parsed.ok().filter(|v| v.is_finite())
}

/// What a row sorts by: its number when the column is numeric and the cell parses, else its
/// text. Computed once per row, not once per comparison.
#[derive(Clone, Copy)]
enum Key<'a> {
    Number(f64),
    Text(&'a str),
}

fn key(text: &str, kind: SortKind) -> Key<'_> {
    match kind {
        SortKind::Numeric => numeric_value(text).map_or(Key::Text(text), Key::Number),
        SortKind::Text => Key::Text(text),
    }
}

/// Non-empty cells: numbers before text, numbers by value, text naturally.
fn compare_keys(a: Key<'_>, b: Key<'_>) -> Ordering {
    match (a, b) {
        (Key::Number(x), Key::Number(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Key::Number(_), Key::Text(_)) => Ordering::Less,
        (Key::Text(_), Key::Number(_)) => Ordering::Greater,
        (Key::Text(x), Key::Text(y)) => natural_cmp(x, y),
    }
}

/// The row order after sorting the rows from `body_start` on: `perm[i]` is the old index of the
/// row that ends up at `i`. Stable; empty cells last in both directions.
pub fn sort_permutation(table: &crate::table::Table, body_start: usize, spec: SortSpec) -> Vec<usize> {
    let rows = table.row_count();
    let body_start = body_start.min(rows);
    let keys: Vec<Option<Key<'_>>> = (body_start..rows)
        .map(|r| {
            let text = table.get(r, spec.col);
            (!text.is_empty()).then(|| key(text, spec.kind))
        })
        .collect();
    let mut body: Vec<usize> = (0..keys.len()).collect();
    body.sort_by(|&a, &b| {
        // Empty cells stay last whatever the direction, so the direction flips only non-empty pairs.
        match (keys.get(a).copied().flatten(), keys.get(b).copied().flatten()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(ka), Some(kb)) => {
                let ord = compare_keys(ka, kb);
                if spec.ascending { ord } else { ord.reverse() }
            }
        }
    });
    (0..body_start).chain(body.into_iter().map(|i| i + body_start)).collect()
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
