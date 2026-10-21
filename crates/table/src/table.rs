//! The table: rows of text cells. Rows may be ragged (shorter than the table's width); a missing
//! cell reads as empty. Every method validates its indices and returns `None`/`false` instead of
//! panicking: the interface and the file are both untrusted sources of numbers.

use std::ops::Deref;

use compact_str::CompactString;

/// One cell: its text, and whether the file had it in quotes. Short texts live inline (no heap
/// allocation), which is most CSV fields.
///
/// The quote flag is what makes a mixed-quoting file round-trip: an untouched cell is written
/// back the way it came. Any new text (typing, paste, cleanup) starts unquoted and is quoted on
/// save only when it needs to be. Equality and hashing look at the text alone; the flag is a
/// note about the file, not part of the value.
///
/// Layout: one `CompactString` whose first byte is the flag (`"` quoted, a space otherwise) and
/// whose rest is the text. A separate `bool` would pad the cell from 24 to 32 bytes, a quarter
/// of a large table; this way the text is still a plain string slice. An empty string (the
/// default) reads as an unquoted empty cell.
#[derive(Clone, Default)]
pub struct Cell(CompactString);

const QUOTED: u8 = b'"';
const PLAIN: u8 = b' ';

impl Cell {
    pub fn new(text: impl AsRef<str>) -> Cell {
        Cell::with_flag(PLAIN, text.as_ref())
    }

    /// A cell the file had in quotes.
    pub fn new_quoted(text: impl AsRef<str>) -> Cell {
        Cell::with_flag(QUOTED, text.as_ref())
    }

    fn with_flag(flag: u8, text: &str) -> Cell {
        let mut s = CompactString::with_capacity(text.len() + 1);
        s.push_str(if flag == QUOTED { "\"" } else { " " });
        s.push_str(text);
        Cell(s)
    }

    pub fn as_str(&self) -> &str {
        self.0.get(1..).unwrap_or("")
    }

    /// Whether the file had this cell in quotes.
    pub fn quoted(&self) -> bool {
        self.0.as_bytes().first() == Some(&QUOTED)
    }

    /// The same text, marked as not from the file (written with quotes only when needed).
    pub fn unquoted(self) -> Cell {
        if self.quoted() { Cell::new(self.as_str()) } else { self }
    }
}

impl std::fmt::Debug for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cell").field("text", &self.as_str()).field("quoted", &self.quoted()).finish()
    }
}

impl Deref for Cell {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for Cell {
    fn eq(&self, other: &Cell) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Cell {}

impl std::hash::Hash for Cell {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Cell {
        Cell::new(s)
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Cell {
        Cell::new(s)
    }
}

impl From<CompactString> for Cell {
    fn from(s: CompactString) -> Cell {
        Cell::new(s)
    }
}

impl PartialEq<str> for Cell {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

pub type Row = Vec<Cell>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    rows: Vec<Row>,
    /// Column count: at least the longest row, possibly more (an empty column added at the end).
    width: usize,
}

impl Table {
    pub fn new() -> Table {
        Table::default()
    }

    pub fn from_rows(rows: Vec<Row>) -> Table {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        Table { rows, width }
    }

    /// A blank table of empty cells.
    pub fn blank(rows: usize, cols: usize) -> Table {
        Table { rows: (0..rows).map(|_| vec![Cell::default(); cols]).collect(), width: cols }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn row(&self, r: usize) -> Option<&[Cell]> {
        self.rows.get(r).map(Vec::as_slice)
    }

    /// The cell's text; empty for a missing cell (ragged row, or out of range).
    pub fn get(&self, r: usize, c: usize) -> &str {
        self.rows.get(r).and_then(|row| row.get(c)).map_or("", |s| s.as_str())
    }

    /// Whether a row has a cell at this column (as opposed to reading empty because it is short).
    pub fn has_cell(&self, r: usize, c: usize) -> bool {
        self.rows.get(r).is_some_and(|row| c < row.len())
    }

    /// Whether any row is shorter than the width.
    pub fn is_ragged(&self) -> bool {
        self.rows.iter().any(|r| r.len() != self.width)
    }

    /// Sets a cell, padding the row with empty cells up to it. Returns the old text and the row's
    /// old length (so an undo can trim the padding again), or `None` if the row does not exist.
    pub fn set(&mut self, r: usize, c: usize, text: impl Into<Cell>) -> Option<(Cell, usize)> {
        let row = self.rows.get_mut(r)?;
        let old_len = row.len();
        if c >= row.len() {
            row.resize(c.checked_add(1)?, Cell::default());
        }
        let slot = row.get_mut(c)?;
        let old = std::mem::replace(slot, text.into());
        self.width = self.width.max(c + 1);
        Some((old, old_len))
    }

    /// Truncates a row to `len` cells if it is longer. Used by undo to remove padding `set` added.
    pub fn truncate_row(&mut self, r: usize, len: usize) {
        if let Some(row) = self.rows.get_mut(r)
            && row.len() > len
        {
            row.truncate(len);
        }
    }

    /// Sets the width. It never drops below the longest row.
    pub fn set_width(&mut self, width: usize) {
        let longest = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        self.width = width.max(longest);
    }

    pub fn insert_rows(&mut self, at: usize, rows: Vec<Row>) -> bool {
        if at > self.rows.len() {
            return false;
        }
        let widest = rows.iter().map(Vec::len).max().unwrap_or(0);
        self.width = self.width.max(widest);
        self.rows.splice(at..at, rows);
        true
    }

    pub fn remove_rows(&mut self, at: usize, count: usize) -> Option<Vec<Row>> {
        let end = at.checked_add(count)?;
        if end > self.rows.len() {
            return None;
        }
        Some(self.rows.drain(at..end).collect())
    }

    /// Inserts `count` columns before column `at`. With `cells`, row `i` receives `cells[i]`
    /// (what a column deletion removed from it); without, each row that reaches `at` receives
    /// empty cells and shorter rows are left as they are.
    pub fn insert_cols(&mut self, at: usize, count: usize, cells: Option<&[Row]>) -> bool {
        if at > self.width || count == 0 {
            return false;
        }
        let Some(new_width) = self.width.checked_add(count) else { return false };
        for (i, row) in self.rows.iter_mut().enumerate() {
            match cells {
                Some(cells) => {
                    let Some(restore) = cells.get(i).filter(|c| !c.is_empty()) else { continue };
                    if row.len() < at {
                        row.resize(at, Cell::default());
                    }
                    row.splice(at..at, restore.iter().cloned());
                }
                None => {
                    if row.len() > at {
                        row.splice(at..at, std::iter::repeat_n(Cell::default(), count));
                    }
                }
            }
        }
        self.width = new_width.max(self.rows.iter().map(Vec::len).max().unwrap_or(0));
        true
    }

    /// Removes `count` columns from `at`. Returns what each row lost (fewer cells for short rows).
    pub fn remove_cols(&mut self, at: usize, count: usize) -> Option<Vec<Row>> {
        let end = at.checked_add(count)?;
        if end > self.width || count == 0 {
            return None;
        }
        let removed = self.rows.iter_mut().map(|row| if row.len() > at { row.drain(at..end.min(row.len())).collect() } else { Vec::new() }).collect();
        self.width -= count;
        Some(removed)
    }

    /// Moves `count` rows from `from` to sit before the row that is at `to` now (`to` outside the
    /// block). Returns the block's new first index.
    pub fn move_rows(&mut self, from: usize, count: usize, to: usize) -> Option<usize> {
        let n = self.rows.len();
        let insert_at = move_target(n, from, count, to)?;
        let block: Vec<Row> = self.rows.drain(from..from + count).collect();
        self.rows.splice(insert_at..insert_at, block);
        Some(insert_at)
    }

    /// Moves `count` columns from `from` to sit before the column at `to`. Rows are padded to the
    /// width first (a column move makes the table rectangular). Returns the new first index and
    /// each row's length before the padding, for undo.
    pub fn move_cols(&mut self, from: usize, count: usize, to: usize) -> Option<(usize, Vec<usize>)> {
        let w = self.width;
        let insert_at = move_target(w, from, count, to)?;
        let mut old_lens = Vec::with_capacity(self.rows.len());
        for row in &mut self.rows {
            old_lens.push(row.len());
            if row.len() < w {
                row.resize(w, Cell::default());
            }
            let block: Row = row.drain(from..from + count).collect();
            row.splice(insert_at..insert_at, block);
        }
        Some((insert_at, old_lens))
    }

    /// Reorders rows: new row `i` is old row `perm[i]`. `perm` must be a permutation of the row
    /// indices, otherwise nothing changes and `false` is returned.
    pub fn permute_rows(&mut self, perm: &[usize]) -> bool {
        if !is_permutation(perm, self.rows.len()) {
            return false;
        }
        let mut old: Vec<Option<Row>> = self.rows.drain(..).map(Some).collect();
        self.rows = perm.iter().map(|&i| old.get_mut(i).and_then(Option::take).unwrap_or_default()).collect();
        true
    }
}

/// Where a block of `count` items starting at `from` lands when moved before `to`, in a sequence
/// of `len`; `None` if the request is out of range or `to` falls inside the block.
pub(crate) fn move_target(len: usize, from: usize, count: usize, to: usize) -> Option<usize> {
    let end = from.checked_add(count)?;
    if count == 0 || end > len || to > len || (to >= from && to <= end) {
        return None;
    }
    Some(if to > from { to - count } else { to })
}

pub(crate) fn is_permutation(perm: &[usize], len: usize) -> bool {
    if perm.len() != len {
        return false;
    }
    let mut seen = vec![false; len];
    perm.iter().all(|&i| match seen.get_mut(i) {
        Some(slot) if !*slot => {
            *slot = true;
            true
        }
        _ => false,
    })
}

/// Inverse of a permutation (`inv[perm[i]] = i`).
pub(crate) fn invert_permutation(perm: &[usize]) -> Vec<usize> {
    let mut inv = vec![0usize; perm.len()];
    for (i, &p) in perm.iter().enumerate() {
        if let Some(slot) = inv.get_mut(p) {
            *slot = i;
        }
    }
    inv
}

#[cfg(test)]
pub(crate) fn table_of(rows: &[&[&str]]) -> Table {
    Table::from_rows(rows.iter().map(|r| r.iter().map(|s| Cell::from(*s)).collect()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_is_one_compact_string() {
        assert_eq!(std::mem::size_of::<Cell>(), std::mem::size_of::<CompactString>());
        let c = Cell::new_quoted("a\"b");
        assert_eq!(c.as_str(), "a\"b");
        assert!(c.quoted());
        assert_eq!(c, Cell::new("a\"b"), "equality ignores the flag");
        assert!(!c.clone().unquoted().quoted());
        assert_eq!(Cell::default().as_str(), "");
        assert!(!Cell::default().quoted());
        assert_eq!(format!("{:?}", Cell::new_quoted("x")), "Cell { text: \"x\", quoted: true }");
    }

    #[test]
    fn get_reads_empty_for_missing_cells() {
        let t = table_of(&[&["a", "b"], &["c"]]);
        assert_eq!((t.row_count(), t.width()), (2, 2));
        assert_eq!(t.get(1, 0), "c");
        assert_eq!(t.get(1, 1), "");
        assert_eq!(t.get(9, 9), "");
        assert!(t.is_ragged());
        assert!(t.has_cell(0, 1) && !t.has_cell(1, 1));
    }

    #[test]
    fn set_pads_and_reports_the_old_length() {
        let mut t = table_of(&[&["a"]]);
        assert_eq!(t.set(0, 2, "x"), Some((Cell::default(), 1)));
        assert_eq!(t.row(0).unwrap().len(), 3);
        assert_eq!(t.width(), 3);
        assert_eq!(t.set(5, 0, "x"), None);
        t.truncate_row(0, 1);
        assert_eq!(t.row(0).unwrap().len(), 1);
        t.set_width(1);
        assert_eq!(t.width(), 1);
    }

    #[test]
    fn rows_insert_remove_move() {
        let mut t = table_of(&[&["a"], &["b"], &["c"], &["d"], &["e"]]);
        assert_eq!(t.move_rows(1, 2, 4), Some(2));
        assert_eq!(t.rows().iter().map(|r| r[0].as_str()).collect::<String>(), "adbce");
        assert_eq!(t.move_rows(2, 2, 1), Some(1));
        assert_eq!(t.rows().iter().map(|r| r[0].as_str()).collect::<String>(), "abcde");
        assert_eq!(t.move_rows(1, 2, 2), None);
        assert_eq!(t.move_rows(1, 2, 3), None, "a move onto its own end changes nothing");
        assert_eq!(t.move_rows(1, 2, 1), None);
        assert_eq!(t.move_rows(4, 2, 0), None);
        let removed = t.remove_rows(1, 2).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(t.insert_rows(1, removed));
        assert_eq!(t.rows().iter().map(|r| r[0].as_str()).collect::<String>(), "abcde");
        assert!(!t.insert_rows(99, vec![]));
        assert_eq!(t.remove_rows(4, 2), None);
    }

    #[test]
    fn cols_insert_remove_keep_ragged_rows_ragged() {
        let mut t = table_of(&[&["a", "b", "c"], &["d"]]);
        assert!(t.insert_cols(1, 1, None));
        assert_eq!(t.row(0).unwrap().len(), 4);
        assert_eq!(t.row(1).unwrap().len(), 1, "a row that ends before the insertion point stays short");
        assert_eq!(t.width(), 4);
        let removed = t.remove_cols(1, 2).unwrap();
        assert_eq!(removed, vec![vec![Cell::default(), Cell::from("b")], vec![]]);
        assert_eq!(t.width(), 2);
        assert!(t.insert_cols(1, 2, Some(&removed)));
        assert_eq!(t.get(0, 2), "b");
        assert_eq!(t.row(1).unwrap().len(), 1);
        // Appending a column past every row only widens the table.
        assert!(t.insert_cols(4, 1, None));
        assert_eq!((t.width(), t.row(0).unwrap().len()), (5, 4));
        assert!(!t.insert_cols(9, 1, None));
        assert_eq!(t.remove_cols(4, 2), None);
    }

    #[test]
    fn col_moves_make_rows_rectangular() {
        let mut t = table_of(&[&["a", "b", "c"], &["d"]]);
        assert_eq!(t.move_cols(0, 1, 3), Some((2, vec![3, 1])));
        assert_eq!(t.row(0).unwrap().iter().map(|c| c.as_str()).collect::<String>(), "bca");
        assert_eq!(t.row(1).unwrap().iter().map(|c| c.as_str()).collect::<String>(), "d");
        assert_eq!(t.row(1).unwrap().len(), 3);
    }

    #[test]
    fn permutations_are_validated_and_inverted() {
        let mut t = table_of(&[&["a"], &["b"], &["c"]]);
        assert!(!t.permute_rows(&[0, 0, 1]));
        assert!(!t.permute_rows(&[0, 1]));
        assert!(t.permute_rows(&[2, 0, 1]));
        assert_eq!(t.rows().iter().map(|r| r[0].as_str()).collect::<String>(), "cab");
        assert!(t.permute_rows(&invert_permutation(&[2, 0, 1])));
        assert_eq!(t.rows().iter().map(|r| r[0].as_str()).collect::<String>(), "abc");
    }
}
