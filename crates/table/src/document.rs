//! A document: one table, the dialect it is saved in, its history and its file.
//!
//! The operations at the bottom are what the interface calls: each builds one undoable edit,
//! growing the table when the target lies past its last row.

use std::path::{Path, PathBuf};

use crate::dialect::Dialect;
use crate::edit::{Edit, History};
use crate::io::{Error, ParseOptions, parse, write};
use crate::selection::{CellRef, Range};
use crate::table::{Cell, Row, Table};

#[derive(Clone, Debug, Default)]
pub struct Document {
    pub table: Table,
    pub dialect: Dialect,
    pub history: History,
    pub path: Option<PathBuf>,
    /// The dialect as last read or written: a changed dialect makes the document dirty too.
    saved_dialect: Dialect,
}

impl Document {
    /// An empty, unnamed document with no header row yet. The interface shows blank cells past
    /// the table's extent and grows the table when one is edited, so nothing blank is written.
    pub fn new() -> Document {
        let dialect = Dialect { header: false, ..Dialect::default() };
        Document { dialect, saved_dialect: dialect, ..Document::default() }
    }

    pub fn from_bytes(bytes: &[u8], opts: &ParseOptions) -> Result<Document, Error> {
        let (table, dialect) = parse(bytes, opts)?;
        Ok(Document { table, dialect, history: History::default(), path: None, saved_dialect: dialect })
    }

    pub fn open(path: impl AsRef<Path>, opts: &ParseOptions) -> Result<Document, Error> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| Error::Io { path: path.display().to_string(), source })?;
        let mut doc = Document::from_bytes(&bytes, opts)?;
        doc.path = Some(path.to_path_buf());
        Ok(doc)
    }

    /// The file name, or "Untitled".
    pub fn display_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".to_string())
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty() || self.dialect_changed()
    }

    /// Whether the dialect differs from the saved one in a way that changes the file. The header
    /// flag only changes how the grid shows the first row, so it does not count.
    pub fn dialect_changed(&self) -> bool {
        Dialect { header: self.dialect.header, ..self.saved_dialect } != self.dialect
    }

    pub fn to_bytes(&self) -> (Vec<u8>, usize) {
        write(&self.table, &self.dialect)
    }

    /// Writes the document to `path` (through a temporary file in the same folder, so a failed
    /// write never leaves a truncated file), makes it the document's file and clears the dirty
    /// flag. Returns how many characters the encoding could not represent.
    pub fn save_to(&mut self, path: impl AsRef<Path>) -> Result<usize, Error> {
        let path = path.as_ref();
        let (bytes, lossy) = self.to_bytes();
        let io = |source: std::io::Error| Error::Io { path: path.display().to_string(), source };
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".csv-editor-tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, &bytes).map_err(io)?;
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io(e));
        }
        self.path = Some(path.to_path_buf());
        self.history.mark_saved();
        self.saved_dialect = self.dialect;
        Ok(lossy)
    }

    /// Applies an edit as one undo step. Returns whether anything changed.
    pub fn apply(&mut self, label: impl Into<String>, edit: Edit) -> bool {
        self.history.apply(&mut self.table, label, edit)
    }

    pub fn undo(&mut self) -> Option<String> {
        self.history.undo(&mut self.table)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.history.redo(&mut self.table)
    }

    // ----- Operations -----------------------------------------------------------------------

    /// Rows to add so the table reaches `rows` rows, as an edit (or nothing to add).
    fn grow_rows(&self, rows: usize) -> Option<Edit> {
        let have = self.table.row_count();
        (rows > have).then(|| Edit::InsertRows { at: have, rows: vec![Vec::new(); rows - have] })
    }

    fn with_growth(&self, rows_needed: usize, edit: Edit) -> Edit {
        match self.grow_rows(rows_needed) {
            Some(grow) => Edit::Batch(vec![grow, edit]),
            None => edit,
        }
    }

    /// Sets one cell, adding rows if it lies past the table.
    pub fn set_cell(&mut self, row: usize, col: usize, text: impl Into<Cell>) -> bool {
        let text = text.into();
        let exists = self.table.has_cell(row, col);
        if (exists && self.table.get(row, col) == text.as_str()) || (!exists && text.is_empty()) {
            return false;
        }
        let edit = self.with_growth(row.saturating_add(1), Edit::set_cell(row, col, text));
        self.apply("Typing", edit)
    }

    /// Empties the cells in the ranges that exist (short rows are not padded).
    pub fn clear(&mut self, ranges: &[Range]) -> bool {
        let mut edits = Vec::new();
        for r in ranges {
            for row in r.rows() {
                let Some(cells) = self.table.row(row) else { break };
                let end = r.end.col.min(cells.len().saturating_sub(1));
                if r.start.col >= cells.len() || cells.get(r.start.col..=end).is_none_or(|c| c.iter().all(|x| x.is_empty())) {
                    continue;
                }
                edits.push(Edit::set_rect(row, r.start.col, vec![vec![Cell::default(); end - r.start.col + 1]]));
            }
        }
        self.apply("Clear", Edit::Batch(edits))
    }

    /// Copies the range's first row into the rows below it (Ctrl+D).
    pub fn fill_down(&mut self, r: Range) -> bool {
        if r.row_count() < 2 {
            return false;
        }
        let source: Row = r.cols().map(|c| Cell::from(self.table.get(r.start.row, c))).collect();
        let cells = vec![source; r.row_count() - 1];
        let edit = self.with_growth(r.end.row + 1, Edit::set_rect(r.start.row + 1, r.start.col, cells));
        self.apply("Fill down", edit)
    }

    /// Inserts `count` empty rows before `at` (at the end when `at` is past the table).
    pub fn insert_rows(&mut self, at: usize, count: usize) -> bool {
        let at = at.min(self.table.row_count());
        let width = self.table.width();
        self.apply("Insert rows", Edit::InsertRows { at, rows: vec![vec![Cell::default(); width]; count] })
    }

    /// Deletes up to `count` rows from `at`; rows past the table are ignored.
    pub fn delete_rows(&mut self, at: usize, count: usize) -> bool {
        let count = count.min(self.table.row_count().saturating_sub(at));
        self.apply("Delete rows", Edit::DeleteRows { at, count })
    }

    /// Inserts copies of the rows `at..at+count` right after them.
    pub fn duplicate_rows(&mut self, at: usize, count: usize) -> bool {
        let end = at.saturating_add(count).min(self.table.row_count());
        let rows: Vec<Row> = self.table.rows().get(at..end).map(<[Row]>::to_vec).unwrap_or_default();
        if rows.is_empty() {
            return false;
        }
        self.apply("Duplicate rows", Edit::InsertRows { at: end, rows })
    }

    pub fn move_rows(&mut self, from: usize, count: usize, to: usize) -> bool {
        self.apply("Move rows", Edit::MoveRows { from, count, to })
    }

    /// Inserts blank rows before (or after) each run of consecutive indices in `rows`, as many as
    /// the run has. Runs past the table insert at its end.
    pub fn insert_rows_at_runs(&mut self, rows: &[usize], after: bool) -> bool {
        let mut rows: Vec<usize> = rows.to_vec();
        rows.sort_unstable();
        rows.dedup();
        if rows.is_empty() {
            return false;
        }
        let width = self.table.width();
        let n = self.table.row_count();
        // Later runs first, so earlier insertions do not shift them.
        let edits: Vec<Edit> = crate::cleanup::runs(&rows)
            .into_iter()
            .rev()
            .map(|(at, count)| Edit::InsertRows { at: (if after { at + count } else { at }).min(n), rows: vec![vec![Cell::default(); width]; count] })
            .collect();
        self.apply("Insert rows", Edit::Batch(edits))
    }

    /// Inserts blank columns left of (or right of) each run of consecutive indices in `cols`.
    pub fn insert_cols_at_runs(&mut self, cols: &[usize], after: bool) -> bool {
        let mut cols: Vec<usize> = cols.to_vec();
        cols.sort_unstable();
        cols.dedup();
        if cols.is_empty() {
            return false;
        }
        let w = self.table.width();
        let edits: Vec<Edit> = crate::cleanup::runs(&cols)
            .into_iter()
            .rev()
            .map(|(at, count)| Edit::InsertCols { at: (if after { at + count } else { at }).min(w), count, cells: None })
            .collect();
        self.apply("Insert columns", Edit::Batch(edits))
    }

    /// Deletes exactly these rows (any order, duplicates ignored, rows past the table ignored).
    pub fn delete_row_set(&mut self, rows: &[usize]) -> bool {
        let n = self.table.row_count();
        let rows: Vec<usize> = rows.iter().copied().filter(|&r| r < n).collect();
        if rows.is_empty() {
            return false;
        }
        self.apply("Delete rows", Edit::Batch(crate::cleanup::delete_rows_edits(rows)))
    }

    /// Inserts a copy of each run of these rows right after it.
    pub fn duplicate_row_set(&mut self, rows: &[usize]) -> bool {
        let n = self.table.row_count();
        let mut rows: Vec<usize> = rows.iter().copied().filter(|&r| r < n).collect();
        rows.sort_unstable();
        rows.dedup();
        if rows.is_empty() {
            return false;
        }
        // Later runs first, so earlier insertions do not shift them.
        let edits: Vec<Edit> = crate::cleanup::runs(&rows)
            .into_iter()
            .rev()
            .map(|(at, count)| Edit::InsertRows {
                at: at + count,
                rows: self.table.rows().get(at..at + count).map(<[Row]>::to_vec).unwrap_or_default(),
            })
            .collect();
        self.apply("Duplicate rows", Edit::Batch(edits))
    }

    /// Deletes exactly these columns.
    pub fn delete_col_set(&mut self, cols: &[usize]) -> bool {
        let w = self.table.width();
        let mut cols: Vec<usize> = cols.iter().copied().filter(|&c| c < w).collect();
        cols.sort_unstable();
        cols.dedup();
        if cols.is_empty() {
            return false;
        }
        let edits: Vec<Edit> = crate::cleanup::runs(&cols).into_iter().rev().map(|(at, count)| Edit::DeleteCols { at, count }).collect();
        self.apply("Delete columns", Edit::Batch(edits))
    }

    /// Inserts `count` empty columns before `at` (at the end when `at` is past the width).
    pub fn insert_cols(&mut self, at: usize, count: usize) -> bool {
        let at = at.min(self.table.width());
        self.apply("Insert columns", Edit::InsertCols { at, count, cells: None })
    }

    pub fn delete_cols(&mut self, at: usize, count: usize) -> bool {
        let count = count.min(self.table.width().saturating_sub(at));
        self.apply("Delete columns", Edit::DeleteCols { at, count })
    }

    pub fn move_cols(&mut self, from: usize, count: usize, to: usize) -> bool {
        self.apply("Move columns", Edit::MoveCols { from, count, to, trim: None })
    }

    /// Pastes a block of cells. A single-cell `target` takes the block at its top-left corner;
    /// a range whose size is a multiple of the block's is tiled with it; any other range takes
    /// the block at its top-left corner. With `cut`, the source cells are emptied in the same
    /// undo step. Rows are added as needed. Returns the range written.
    pub fn paste(&mut self, target: Range, cells: &[Row], cut: Option<Range>) -> Option<Range> {
        let height = cells.len();
        let width = cells.iter().map(Vec::len).max().unwrap_or(0);
        if height == 0 || width == 0 {
            return None;
        }
        let tiles = |span: usize, size: usize| if target.is_single() || !span.is_multiple_of(size) { 1 } else { span / size };
        let (down, across) = (tiles(target.row_count(), height), tiles(target.col_count(), width));
        let mut block: Vec<Row> = Vec::with_capacity(height * down);
        for _ in 0..down {
            for row in cells {
                let mut out = Vec::with_capacity(width * across);
                for _ in 0..across {
                    out.extend(row.iter().cloned());
                    out.extend(std::iter::repeat_n(Cell::default(), width - row.len()));
                }
                block.push(out);
            }
        }
        let at = target.start;
        let end = CellRef::new(at.row + block.len() - 1, at.col + width * across - 1);
        let mut edits = Vec::new();
        if let Some(src) = cut {
            for row in src.rows().filter(|r| *r < self.table.row_count()) {
                let len = self.table.row(row).map_or(0, <[Cell]>::len);
                if src.start.col < len {
                    let count = src.end.col.min(len - 1) - src.start.col + 1;
                    edits.push(Edit::set_rect(row, src.start.col, vec![vec![Cell::default(); count]]));
                }
            }
        }
        if let Some(grow) = self.grow_rows(end.row + 1) {
            edits.push(grow);
        }
        edits.push(Edit::set_rect(at.row, at.col, block));
        let label = if cut.is_some() { "Move" } else { "Paste" };
        self.apply(label, Edit::Batch(edits)).then_some(Range::new(at, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::from_bytes(text.as_bytes(), &ParseOptions::default()).unwrap()
    }

    fn text(d: &Document) -> String {
        String::from_utf8(d.to_bytes().0).unwrap()
    }

    #[test]
    fn open_edit_save_reopen() {
        let dir = std::env::temp_dir().join(format!("csv-editor-doc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.csv");
        std::fs::write(&path, b"a,b\r\n1,2\r\n").unwrap();
        let mut doc = Document::open(&path, &ParseOptions::default()).unwrap();
        assert_eq!(doc.display_name(), "t.csv");
        assert!(!doc.is_dirty());
        assert!(doc.apply("Typing", Edit::set_cell(1, 1, "9")));
        assert!(doc.is_dirty());
        assert_eq!(doc.save_to(&path).unwrap(), 0);
        assert!(!doc.is_dirty());
        assert_eq!(std::fs::read(&path).unwrap(), b"a,b\r\n1,9\r\n");
        assert!(!dir.join("t.csv.csv-editor-tmp").exists());
        doc.undo();
        assert!(doc.is_dirty());
        let missing = Document::open(dir.join("missing.csv"), &ParseOptions::default());
        assert!(matches!(missing, Err(Error::Io { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_dialect_dirties_the_document_until_saved() {
        let dir = std::env::temp_dir().join(format!("csv-editor-dialect-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = doc("a,b\n1,2\n");
        d.dialect.header = !d.dialect.header;
        assert!(!d.is_dirty(), "the header flag is a view setting");
        d.dialect.delimiter = b';';
        assert!(d.is_dirty() && d.dialect_changed());
        let path = dir.join("d.csv");
        d.save_to(&path).unwrap();
        assert!(!d.is_dirty());
        assert_eq!(std::fs::read(&path).unwrap(), b"a;b\n1;2\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_document_is_empty_and_untitled() {
        let doc = Document::new();
        assert_eq!(doc.display_name(), "Untitled");
        assert!(doc.table.is_empty());
        assert_eq!(doc.to_bytes().0, b"");
    }

    #[test]
    fn set_cell_grows_the_table_and_skips_no_ops() {
        let mut d = doc("a,b\n1,2\n");
        assert!(!d.set_cell(1, 0, "1"), "same text: nothing to record");
        assert!(!d.set_cell(5, 5, ""), "an empty text past the table adds nothing");
        assert!(d.set_cell(3, 1, "x"));
        // The file was rectangular, so the added rows are written padded.
        assert_eq!(text(&d), "a,b\n1,2\n,\n,x\n");
        assert_eq!(d.undo(), Some("Typing".to_string()));
        assert_eq!(text(&d), "a,b\n1,2\n");
        assert_eq!(d.table.row_count(), 2);
    }

    #[test]
    fn clear_only_touches_existing_cells() {
        let mut d = doc("a,b,c\n1\n,,\n");
        assert!(d.clear(&[Range::new(CellRef::new(0, 1), CellRef::new(2, 2))]));
        assert_eq!(text(&d), "a,,\n1\n,,\n");
        assert!(!d.clear(&[Range::new(CellRef::new(0, 1), CellRef::new(9, 9))]), "already empty");
        d.undo();
        assert_eq!(text(&d), "a,b,c\n1\n,,\n");
    }

    #[test]
    fn fill_down_repeats_the_first_row() {
        let mut d = doc("h\nx\n");
        assert!(d.fill_down(Range::new(CellRef::new(1, 0), CellRef::new(3, 0))));
        assert_eq!(text(&d), "h\nx\nx\nx\n");
        assert!(!d.fill_down(Range::cell(CellRef::new(1, 0))));
        d.undo();
        assert_eq!(text(&d), "h\nx\n");
    }

    #[test]
    fn row_operations() {
        let mut d = doc("a,b\n1,2\n3,4\n");
        assert!(d.insert_rows(1, 2));
        assert_eq!(text(&d), "a,b\n,\n,\n1,2\n3,4\n");
        assert!(d.delete_rows(1, 2));
        assert!(d.duplicate_rows(2, 5), "count past the end is clamped");
        assert_eq!(text(&d), "a,b\n1,2\n3,4\n3,4\n");
        assert!(d.move_rows(3, 1, 1));
        assert_eq!(text(&d), "a,b\n3,4\n1,2\n3,4\n");
        assert!(!d.delete_rows(10, 1));
        assert!(!d.duplicate_rows(10, 1));
        assert!(d.insert_rows(99, 1), "past the end inserts at the end");
        assert_eq!(d.table.row_count(), 5);
        for _ in 0..5 {
            d.undo();
        }
        assert_eq!(text(&d), "a,b\n1,2\n3,4\n");
    }

    #[test]
    fn paste_places_tiles_grows_and_moves() {
        let block = vec![vec![Cell::from("x"), Cell::from("y")], vec![Cell::from("1")]];
        let mut d = doc("a,b,c\n1,2,3\n");
        // Single-cell target: top-left placement, short rows padded within the block's width.
        let r = d.paste(Range::cell(CellRef::new(1, 1)), &block, None).unwrap();
        assert_eq!(r, Range::new(CellRef::new(1, 1), CellRef::new(2, 2)));
        assert_eq!(text(&d), "a,b,c\n1,x,y\n,1,\n");
        d.undo();
        // A target twice the block in both directions is tiled.
        let r = d.paste(Range::new(CellRef::new(0, 0), CellRef::new(3, 3)), &block, None).unwrap();
        assert_eq!(r, Range::new(CellRef::new(0, 0), CellRef::new(3, 3)));
        assert_eq!(text(&d), "x,y,x,y\n1,,1,\nx,y,x,y\n1,,1,\n");
        d.undo();
        // A target that is not a multiple takes one copy at its corner.
        d.paste(Range::new(CellRef::new(0, 0), CellRef::new(2, 2)), &block, None).unwrap();
        assert_eq!(text(&d), "x,y,c\n1,,3\n", "the block's missing cell pastes as a blank");
        d.undo();
        // Cut: the source is emptied in the same step.
        let src = Range::new(CellRef::new(1, 0), CellRef::new(1, 1));
        let cells = vec![vec![Cell::from("1"), Cell::from("2")]];
        d.paste(Range::cell(CellRef::new(0, 1)), &cells, Some(src)).unwrap();
        assert_eq!(text(&d), "a,1,2\n,,3\n");
        assert_eq!(d.history.undo_label(), Some("Move"));
        d.undo();
        assert_eq!(text(&d), "a,b,c\n1,2,3\n");
        assert!(d.paste(Range::cell(CellRef::new(0, 0)), &[], None).is_none());
    }

    #[test]
    fn inserts_land_at_every_run() {
        let mut d = doc("h\na\nb\nc\n");
        assert!(d.insert_rows_at_runs(&[1, 3], false));
        assert_eq!(text(&d), "h\n\na\nb\n\nc\n");
        d.undo();
        assert!(d.insert_rows_at_runs(&[1, 2, 3], true));
        assert_eq!(text(&d), "h\na\nb\nc\n\n\n\n", "one run of three: three rows after it");
        d.undo();
        assert!(d.insert_rows_at_runs(&[40], false), "past the table: at the end");
        assert_eq!(d.table.row_count(), 5);
        let mut d = doc("a,b,c\n1,2,3\n");
        assert!(d.insert_cols_at_runs(&[0, 2], true));
        assert_eq!(text(&d), "a,,b,c,\n1,,2,3,\n");
    }

    #[test]
    fn row_and_column_sets() {
        let mut d = doc("h\na\nb\nc\nd\n");
        assert!(d.delete_row_set(&[4, 2, 2, 99]));
        assert_eq!(text(&d), "h\na\nc\n");
        d.undo();
        assert!(d.duplicate_row_set(&[1, 2, 4]));
        assert_eq!(text(&d), "h\na\nb\na\nb\nc\nd\nd\n", "runs are copied as blocks, after themselves");
        d.undo();
        assert!(!d.delete_row_set(&[50]));
        let mut d = doc("a,b,c,d\n1,2,3,4\n");
        assert!(d.delete_col_set(&[3, 0]));
        assert_eq!(text(&d), "b,c\n2,3\n");
        d.undo();
        assert_eq!(text(&d), "a,b,c,d\n1,2,3,4\n");
    }

    #[test]
    fn column_operations() {
        let mut d = doc("a,b\n1,2\n");
        assert!(d.insert_cols(1, 1));
        assert_eq!(text(&d), "a,,b\n1,,2\n");
        assert!(d.delete_cols(1, 1));
        assert!(d.move_cols(0, 1, 2));
        assert_eq!(text(&d), "b,a\n2,1\n");
        assert!(d.insert_cols(50, 1), "past the width appends");
        assert_eq!(text(&d), "b,a,\n2,1,\n");
        assert!(!d.delete_cols(50, 1));
        d.undo();
        d.undo();
        d.undo();
        d.undo();
        assert_eq!(text(&d), "a,b\n1,2\n");
    }
}
