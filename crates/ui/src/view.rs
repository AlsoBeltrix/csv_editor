//! Per-document interface state: selection, scroll position, column widths, row filters.
//!
//! Row filters are a view: hidden rows stay in the table (edits, saves and sorts see them), the
//! grid just does not show them. The grid scrolls through *display rows*: the visible body rows
//! in order, followed by blank rows past the table.

use std::collections::HashMap;
use std::rc::Rc;

use csv_editor_table::{CellRef, Document, Range, Selection};

/// Blank rows kept scrollable past the table's last row, so there is always room to type into.
pub const EXTRA_ROWS: usize = 30;
/// Blank columns kept past the table's last column.
pub const EXTRA_COLS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Contains,
    NotContains,
    Equals,
    Blank,
    NotBlank,
}

impl FilterKind {
    pub const ALL: [FilterKind; 5] = [FilterKind::Contains, FilterKind::NotContains, FilterKind::Equals, FilterKind::Blank, FilterKind::NotBlank];

    pub fn label(self) -> &'static str {
        match self {
            FilterKind::Contains => "contains",
            FilterKind::NotContains => "does not contain",
            FilterKind::Equals => "equals",
            FilterKind::Blank => "is blank",
            FilterKind::NotBlank => "is not blank",
        }
    }

    pub fn takes_text(self) -> bool {
        matches!(self, FilterKind::Contains | FilterKind::NotContains | FilterKind::Equals)
    }
}

/// One column's row filter. Text comparisons ignore case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub col: usize,
    pub kind: FilterKind,
    pub text: String,
}

impl Filter {
    pub fn passes(&self, cell: &str) -> bool {
        match self.kind {
            FilterKind::Blank => cell.trim().is_empty(),
            FilterKind::NotBlank => !cell.trim().is_empty(),
            FilterKind::Contains => cell.to_lowercase().contains(&self.text.to_lowercase()),
            FilterKind::NotContains => !cell.to_lowercase().contains(&self.text.to_lowercase()),
            FilterKind::Equals => cell.trim().eq_ignore_ascii_case(self.text.trim()) || cell.trim().to_lowercase() == self.text.trim().to_lowercase(),
        }
    }

    pub fn describe(&self) -> String {
        if self.kind.takes_text() { format!("{} \"{}\"", self.kind.label(), self.text) } else { self.kind.label().to_string() }
    }
}

pub struct DocView {
    pub doc: Document,
    pub sel: Selection,
    /// The display row shown at the top of the body (0 = the first visible body row).
    pub scroll: usize,
    /// The column shown at the left of the body.
    pub scroll_col: usize,
    /// Column widths in unzoomed points, where the user or autofit set one.
    pub col_widths: HashMap<usize, f32>,
    pub filters: Vec<Filter>,
    /// Body rows that pass the filters, in order; `None` without filters.
    visible: Option<Rc<Vec<usize>>>,
    /// What `visible` was computed for: the document version and the filter generation.
    visible_key: (u64, u64),
    filter_gen: u64,
}

impl DocView {
    pub fn new(doc: Document) -> DocView {
        let first = usize::from(doc.dialect.header);
        DocView {
            doc,
            sel: Selection::at(CellRef::new(first, 0)),
            scroll: 0,
            scroll_col: 0,
            col_widths: HashMap::new(),
            filters: Vec::new(),
            visible: None,
            visible_key: (0, 0),
            filter_gen: 0,
        }
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

    /// The column's label for menus: its letter, and its header name when it has one.
    pub fn column_title(&self, col: usize) -> String {
        let letter = csv_editor_table::column_label(col);
        let name = self.header_name(col);
        if name.is_empty() { letter } else { format!("{letter} ({name})") }
    }

    /// Turns the "first row is header" setting on or off and keeps the selection inside the body.
    pub fn set_header(&mut self, header: bool) {
        self.doc.dialect.header = header;
        self.filter_gen = self.filter_gen.wrapping_add(1);
        let start = self.body_start();
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
            *r = Range::new(clamp(r.start), clamp(r.end));
        }
    }

    /// Whether this is an untouched, unnamed, empty document (the one a first Open replaces).
    pub fn is_blank(&self) -> bool {
        self.doc.path.is_none() && self.doc.table.is_empty() && !self.doc.is_dirty() && !self.doc.history.can_undo()
    }

    // ----- Filters and display rows -----------------------------------------------------------

    /// Replaces the filter on `filter.col` (or adds one).
    pub fn set_filter(&mut self, filter: Filter) {
        self.filters.retain(|f| f.col != filter.col);
        self.filters.push(filter);
        self.filter_gen = self.filter_gen.wrapping_add(1);
    }

    pub fn clear_filter(&mut self, col: usize) {
        self.filters.retain(|f| f.col != col);
        self.filter_gen = self.filter_gen.wrapping_add(1);
    }

    pub fn clear_filters(&mut self) {
        self.filters.clear();
        self.filter_gen = self.filter_gen.wrapping_add(1);
    }

    pub fn filtered(&self) -> bool {
        !self.filters.is_empty()
    }

    /// Brings the visible-row list up to date with the table and the filters. Cheap when nothing
    /// changed; the grid calls it every frame.
    pub fn refresh_visible(&mut self) {
        if self.filters.is_empty() {
            self.visible = None;
            return;
        }
        let key = (self.doc.history.version(), self.filter_gen);
        if self.visible.is_some() && self.visible_key == key {
            return;
        }
        let table = &self.doc.table;
        let rows = (self.body_start()..table.row_count()).filter(|&r| self.filters.iter().all(|f| f.passes(table.get(r, f.col)))).collect();
        self.visible = Some(Rc::new(rows));
        self.visible_key = key;
    }

    /// Data rows the grid shows (all body rows without a filter).
    pub fn visible_count(&self) -> usize {
        match &self.visible {
            Some(v) => v.len(),
            None => self.doc.table.row_count().saturating_sub(self.body_start()),
        }
    }

    /// Display rows: the visible body rows, then the blank rows past the table.
    pub fn display_count(&self) -> usize {
        self.visible_count().saturating_add(EXTRA_ROWS)
    }

    /// The table row shown at display row `i`; past the visible rows, the blank rows after the
    /// table in order.
    pub fn row_at_display(&self, i: usize) -> usize {
        match &self.visible {
            Some(v) => match v.get(i) {
                Some(&r) => r,
                None => self.doc.table.row_count().max(self.body_start()).saturating_add(i - v.len()),
            },
            None => self.body_start().saturating_add(i),
        }
    }

    /// Where table row `r` is displayed; for a hidden row, where the next visible row is.
    pub fn display_of_row(&self, r: usize) -> usize {
        match &self.visible {
            Some(v) => {
                let rows = self.doc.table.row_count().max(self.body_start());
                if r >= rows {
                    return v.len().saturating_add(r - rows);
                }
                match v.binary_search(&r) {
                    Ok(i) => i,
                    Err(i) => i.min(v.len()),
                }
            }
            None => r.saturating_sub(self.body_start()),
        }
    }

    /// A row-visibility test that owns what it needs, for calls that also borrow the document
    /// mutably (replace all).
    pub fn visibility(&self) -> Box<dyn Fn(usize) -> bool> {
        let rows = self.doc.table.row_count();
        let start = self.body_start();
        match &self.visible {
            Some(v) => {
                let v = Rc::clone(v);
                Box::new(move |r| r >= rows || v.binary_search(&r).is_ok())
            }
            None => Box::new(move |r| r >= start),
        }
    }

    pub fn is_row_visible(&self, r: usize) -> bool {
        match &self.visible {
            Some(v) => r >= self.doc.table.row_count() || v.binary_search(&r).is_ok(),
            None => r >= self.body_start(),
        }
    }

    /// `r` itself if it is shown, otherwise the nearest displayed row after it (or before, at
    /// the end).
    pub fn snap_visible(&self, r: usize) -> usize {
        if self.is_row_visible(r) {
            return r;
        }
        let d = self.display_of_row(r);
        self.row_at_display(d.min(self.display_count().saturating_sub(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv_editor_table::ParseOptions;

    fn view(text: &str) -> DocView {
        DocView::new(Document::from_bytes(text.as_bytes(), &ParseOptions::default()).unwrap())
    }

    #[test]
    fn filters_match_text_and_blanks() {
        let f = |kind, text: &str| Filter { col: 0, kind, text: text.to_string() };
        assert!(f(FilterKind::Contains, "APP").passes("pineapple"));
        assert!(!f(FilterKind::NotContains, "APP").passes("pineapple"));
        assert!(f(FilterKind::Equals, " Oslo ").passes("oslo"));
        assert!(!f(FilterKind::Equals, "Oslo").passes("Oslo city"));
        assert!(f(FilterKind::Blank, "").passes("  ") && !f(FilterKind::Blank, "").passes("x"));
        assert!(f(FilterKind::NotBlank, "").passes("x"));
        assert_eq!(f(FilterKind::Contains, "a").describe(), "contains \"a\"");
        assert_eq!(f(FilterKind::Blank, "").describe(), "is blank");
    }

    #[test]
    fn display_rows_skip_hidden_rows_and_continue_past_the_table() {
        let mut v = view("city\nOslo\nBerlin\nOslo\nLisbon\n");
        assert_eq!(v.display_count(), 4 + EXTRA_ROWS);
        assert_eq!(v.row_at_display(0), 1);
        assert_eq!(v.display_of_row(3), 2);
        v.set_filter(Filter { col: 0, kind: FilterKind::Contains, text: "os".to_string() });
        v.refresh_visible();
        assert!(v.filtered());
        assert_eq!(v.visible_count(), 2, "Oslo twice");
        assert_eq!((v.row_at_display(0), v.row_at_display(1)), (1, 3));
        assert_eq!(v.row_at_display(2), 5, "then the blank rows after the table");
        assert_eq!(v.display_of_row(3), 1);
        assert_eq!(v.display_of_row(2), 1, "a hidden row maps to the next visible one");
        assert_eq!(v.display_of_row(6), 3);
        assert!(!v.is_row_visible(2) && v.is_row_visible(3) && v.is_row_visible(9));
        assert_eq!(v.snap_visible(2), 3);
        assert_eq!(v.snap_visible(4), 5, "after the last visible row comes the first blank one");
        // Editing a filtered column re-evaluates the filter.
        v.doc.set_cell(2, 0, "Oslo");
        v.refresh_visible();
        assert_eq!(v.visible_count(), 3);
        v.clear_filters();
        v.refresh_visible();
        assert!(!v.filtered());
        assert_eq!(v.row_at_display(1), 2);
    }

    #[test]
    fn column_titles_include_header_names() {
        let v = view("id,name\n1,Ann\n");
        assert_eq!(v.column_title(1), "B (name)");
        assert_eq!(v.column_title(5), "F");
    }
}
