//! Per-document interface state: selection, scroll position, column widths.

use std::collections::HashMap;

use csv_editor_table::{CellRef, Document, Selection};

/// Blank rows kept scrollable past the table's last row, so there is always room to type into.
pub const EXTRA_ROWS: usize = 30;
/// Blank columns kept past the table's last column.
pub const EXTRA_COLS: usize = 5;

pub struct DocView {
    pub doc: Document,
    pub sel: Selection,
    /// The table row shown at the top of the scrolling body.
    pub scroll_row: usize,
    /// The column shown at the left of the body.
    pub scroll_col: usize,
    /// Column widths in unzoomed points, where the user or autofit set one.
    pub col_widths: HashMap<usize, f32>,
}

impl DocView {
    pub fn new(doc: Document) -> DocView {
        let first = usize::from(doc.dialect.header);
        DocView { doc, sel: Selection::at(CellRef::new(first, 0)), scroll_row: first, scroll_col: 0, col_widths: HashMap::new() }
    }

    /// The first table row shown in the body: 1 when the first row names the columns and is
    /// shown in the column header strip instead.
    pub fn body_start(&self) -> usize {
        usize::from(self.doc.dialect.header)
    }

    /// Rows the grid lets you scroll to and select, blank ones past the data included.
    pub fn extent_rows(&self) -> usize {
        self.doc.table.row_count().max(self.body_start()).saturating_add(EXTRA_ROWS)
    }

    pub fn extent_cols(&self) -> usize {
        self.doc.table.width().max(1).saturating_add(EXTRA_COLS)
    }

    /// The bottom-right selectable cell.
    pub fn max_cell(&self) -> CellRef {
        CellRef::new(self.extent_rows() - 1, self.extent_cols() - 1)
    }

    /// The column's name from the header row, or empty when there is no header row.
    pub fn header_name(&self, col: usize) -> &str {
        if self.doc.dialect.header { self.doc.table.get(0, col) } else { "" }
    }

    /// Turns the "first row is header" setting on or off and keeps the selection and scroll
    /// position inside the body.
    pub fn set_header(&mut self, header: bool) {
        self.doc.dialect.header = header;
        let start = self.body_start();
        if self.scroll_row < start {
            self.scroll_row = start;
        }
        if self.sel.active.row < start || self.sel.anchor.row < start || self.sel.ranges.iter().any(|r| r.start.row < start) {
            self.sel = Selection::at(CellRef::new(start.max(self.sel.active.row), self.sel.active.col));
        }
    }

    /// Keeps the selection inside the grid after the table shrank (undo, deletions).
    pub fn clamp_selection(&mut self) {
        let max = self.max_cell();
        let start = self.body_start();
        let clamp = |c: CellRef| CellRef::new(c.row.clamp(start, max.row), c.col.min(max.col));
        self.sel.active = clamp(self.sel.active);
        self.sel.anchor = clamp(self.sel.anchor);
        for r in &mut self.sel.ranges {
            *r = csv_editor_table::Range::new(clamp(r.start), clamp(r.end));
        }
        self.scroll_row = self.scroll_row.max(start);
    }

    /// Whether this is an untouched, unnamed, empty document (the one a first Open replaces).
    pub fn is_blank(&self) -> bool {
        self.doc.path.is_none() && self.doc.table.is_empty() && !self.doc.is_dirty() && !self.doc.history.can_undo()
    }
}
